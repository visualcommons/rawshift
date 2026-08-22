//! `CVPixelBuffer` → [`DecodedFrame`].
//!
//! VideoToolbox hands back the decoder's native surface (this backend passes
//! no destination pixel-buffer attributes, so no conversion is imposed). The
//! surface's `OSType` is therefore the ground truth for pixel layout, colour
//! range and bit depth, and [`describe_format`] is the single table that
//! interprets it.
//!
//! ## Geometry
//!
//! `CVPixelBufferGetWidth`/`GetHeight` report the **clean** aperture — the
//! conformance window is already applied — so, unlike the VAAPI backend, this
//! one does no cropping arithmetic. `videotoolbox_crops_to_the_conformance_window`
//! in `tests/videotoolbox_device.rs` pins that assumption to real hardware.
//!
//! ## 10-bit layout
//!
//! CoreVideo's `x420`/`xf20` are documented in `CVPixelBuffer.h` as "2 plane
//! YCbCr10 4:2:0, each 10 bits in the **MSBs** of 16bits". That is exactly
//! [`PixelFormat::P010`]'s contract, so the copy is a plain per-row `memcpy`
//! with no shifting or byte-swapping (every Apple target is little-endian).
//! Do not "fix" this to shift right.
//!
//! [`PixelFormat::I010`] is unreachable here: CoreVideo has no planar 10-bit
//! 4:2:0 type. Its absence from the table below is deliberate.

use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelFormatType_420YpCbCr8Planar,
    kCVPixelFormatType_420YpCbCr8PlanarFullRange, kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange, kCVReturnSuccess,
};

use super::status::{OSType, decode_err};
use crate::{ColorRange, DecodedFrame, HwCodec, HwDecodeError, PixelFormat, Plane};

/// Read-only lock: we copy the planes out and never write to the surface.
const READ_ONLY: CVPixelBufferLockFlags = CVPixelBufferLockFlags::ReadOnly;

// CoreVideo's constants keep Apple's C spelling, which is not a legal Rust
// pattern name. Alias them once here so the table below reads idiomatically
// and each four-character code sits next to the constant it belongs to.

/// `'420v'` — 8-bit biplanar 4:2:0, video range (luma 16..235).
const NV12_VIDEO_RANGE: OSType = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
/// `'420f'` — 8-bit biplanar 4:2:0, full range.
const NV12_FULL_RANGE: OSType = kCVPixelFormatType_420YpCbCr8BiPlanarFullRange;
/// `'x420'` — 10-bit biplanar 4:2:0, video range, sample in the MSBs.
const P010_VIDEO_RANGE: OSType = kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange;
/// `'xf20'` — 10-bit biplanar 4:2:0, full range, sample in the MSBs.
const P010_FULL_RANGE: OSType = kCVPixelFormatType_420YpCbCr10BiPlanarFullRange;
/// `'y420'` — 8-bit planar 4:2:0, video range.
const I420_VIDEO_RANGE: OSType = kCVPixelFormatType_420YpCbCr8Planar;
/// `'f420'` — 8-bit planar 4:2:0, full range.
const I420_FULL_RANGE: OSType = kCVPixelFormatType_420YpCbCr8PlanarFullRange;

/// The pixel formats a decompression session is asked to produce.
///
/// **Why ask at all.** Left to itself, VideoToolbox hands back the decoder's
/// native surface, and on Apple silicon a 10-bit HEVC decode natively produces
/// `'p420'` — a format that appears in no public CoreVideo header, whose plane
/// contents demonstrably do **not** match the documented `x420` samples for
/// the same bitstream. There is no safe way to interpret it. Offering an
/// explicit list keeps the decoder on documented, linear layouts.
///
/// **Why a list rather than one format.** All four documented 4:2:0 surfaces
/// are offered — both depths and both ranges — so VideoToolbox still picks the
/// one that matches the stream instead of converting to a format we imposed.
/// That is what preserves the video/full range distinction, which
/// [`describe_format`] then reads back off the chosen surface.
pub(super) fn destination_attributes() -> CFRetained<CFDictionary<CFString, CFType>> {
    let formats = [
        NV12_VIDEO_RANGE,
        NV12_FULL_RANGE,
        P010_VIDEO_RANGE,
        P010_FULL_RANGE,
    ]
    .map(|fourcc| CFNumber::new_i32(fourcc as i32));
    let refs: Vec<&CFNumber> = formats.iter().map(|n| &**n).collect();
    let list = CFArray::from_objects(&refs);
    // SAFETY: an immortal constant `CFStringRef` exported by CoreVideo.
    let key = unsafe { kCVPixelBufferPixelFormatTypeKey };
    CFDictionary::from_slices(&[key], &[list.as_ref() as &CFType])
}

/// What a CoreVideo pixel format means in this crate's vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SurfaceFormat {
    pub(super) format: PixelFormat,
    pub(super) range: ColorRange,
    pub(super) bit_depth: u8,
}

/// The 4:2:0 surfaces a VideoToolbox HEVC/AV1 still decode can emit.
///
/// Anything else — 4:2:2/4:4:4, and the lossless (`&8v0`) / lossy (`-8v0`)
/// packed families, which have no CPU-addressable plane layout — is rejected
/// by name rather than misread.
pub(super) fn describe_format(fourcc: OSType) -> Option<SurfaceFormat> {
    let described = match fourcc {
        // '420v' — 8-bit biplanar, video range (luma 16..235).
        NV12_VIDEO_RANGE => SurfaceFormat {
            format: PixelFormat::Nv12,
            range: ColorRange::Limited,
            bit_depth: 8,
        },
        // '420f' — 8-bit biplanar, full range.
        NV12_FULL_RANGE => SurfaceFormat {
            format: PixelFormat::Nv12,
            range: ColorRange::Full,
            bit_depth: 8,
        },
        // 'x420' — 10-bit biplanar, video range, sample in the MSBs.
        P010_VIDEO_RANGE => SurfaceFormat {
            format: PixelFormat::P010,
            range: ColorRange::Limited,
            bit_depth: 10,
        },
        // 'xf20' — 10-bit biplanar, full range, sample in the MSBs.
        P010_FULL_RANGE => SurfaceFormat {
            format: PixelFormat::P010,
            range: ColorRange::Full,
            bit_depth: 10,
        },
        // 'y420' — 8-bit planar, video range.
        I420_VIDEO_RANGE => SurfaceFormat {
            format: PixelFormat::I420,
            range: ColorRange::Limited,
            bit_depth: 8,
        },
        // 'f420' — 8-bit planar, full range.
        I420_FULL_RANGE => SurfaceFormat {
            format: PixelFormat::I420,
            range: ColorRange::Full,
            bit_depth: 8,
        },
        _ => return None,
    };
    Some(described)
}

/// Render a four-character code the way the CoreVideo headers write it.
pub(super) fn fourcc_name(fourcc: OSType) -> String {
    let bytes = fourcc.to_be_bytes();
    if bytes.iter().all(|b| (0x20..=0x7e).contains(b)) {
        format!("'{}'", bytes.iter().map(|&b| b as char).collect::<String>())
    } else {
        format!("{fourcc:#010x}")
    }
}

/// The per-plane byte geometry [`DecodedFrame::new`] validates against.
///
/// Derived from the frame dimensions with the same ceiling rules the
/// constructor uses, never from `CVPixelBufferGetWidthOfPlane`, so
/// construction cannot disagree with validation.
fn plane_geometry(format: PixelFormat, width: u32, height: u32) -> Vec<(usize, usize)> {
    let bps = format.bytes_per_sample();
    let (cw, ch) = (width.div_ceil(2) as usize, height.div_ceil(2) as usize);
    let luma = (width as usize * bps, height as usize);
    match format.plane_count() {
        // Biplanar: full-size Y, then half-size interleaved CbCr (two samples
        // per chroma column).
        2 => vec![luma, (cw * 2 * bps, ch)],
        // Planar: full-size Y, then half-size Cb and Cr.
        _ => vec![luma, (cw * bps, ch), (cw * bps, ch)],
    }
}

/// Copy one plane into a tightly packed [`Plane`] (`stride == row_bytes`).
///
/// Every offset is checked: a surface whose reported geometry does not cover
/// the rows we need yields `None` rather than reading out of bounds.
fn copy_plane(base: *const u8, stride: usize, row_bytes: usize, rows: usize) -> Option<Plane> {
    if base.is_null() || rows == 0 || stride < row_bytes {
        return None;
    }
    // The last row needs only `row_bytes`, not a full stride — reading a whole
    // stride past the final row would run off the end of the mapping.
    let span = stride
        .checked_mul(rows.checked_sub(1)?)?
        .checked_add(row_bytes)?;
    // SAFETY: the pixel buffer is locked read-only by the caller for the whole
    // of this call, so CoreVideo guarantees `bytesPerRow * (heightOfPlane - 1)
    // + rowBytes` initialised bytes at the plane base address. The caller has
    // checked `rows <= heightOfPlane` and `row_bytes <= stride`, so `span` is
    // within that guarantee. The slice is immutable, is not aliased by any
    // other Rust reference, and dies before the matching unlock.
    let src = unsafe { std::slice::from_raw_parts(base, span) };
    let mut data = Vec::with_capacity(row_bytes.checked_mul(rows)?);
    for row in 0..rows {
        let start = stride.checked_mul(row)?;
        data.extend_from_slice(src.get(start..start.checked_add(row_bytes)?)?);
    }
    Some(Plane {
        data,
        stride: row_bytes,
    })
}

/// Lock the surface, copy every plane out, unlock, and build the frame.
pub(super) fn to_decoded_frame(
    codec: HwCodec,
    pixel_buffer: &CVPixelBuffer,
) -> Result<DecodedFrame, HwDecodeError> {
    let fourcc = CVPixelBufferGetPixelFormatType(pixel_buffer);
    let described = describe_format(fourcc).ok_or_else(|| {
        decode_err(
            codec,
            format!(
                "VideoToolbox emitted unsupported pixel format {} \
                 (this backend handles 8/10-bit 4:2:0 only)",
                fourcc_name(fourcc)
            ),
        )
    })?;

    let width = u32::try_from(CVPixelBufferGetWidth(pixel_buffer))
        .map_err(|_| decode_err(codec, "decoded surface width does not fit in u32"))?;
    let height = u32::try_from(CVPixelBufferGetHeight(pixel_buffer))
        .map_err(|_| decode_err(codec, "decoded surface height does not fit in u32"))?;

    let geometry = plane_geometry(described.format, width, height);
    let plane_count = CVPixelBufferGetPlaneCount(pixel_buffer);
    if plane_count != geometry.len() {
        return Err(decode_err(
            codec,
            format!(
                "decoded surface {} reports {plane_count} planes, expected {}",
                fourcc_name(fourcc),
                geometry.len()
            ),
        ));
    }

    // SAFETY: `pixel_buffer` is a live, retained surface handed to us by the
    // decompression callback. Locking read-only makes the planes CPU-
    // addressable; it is paired with exactly one unlock below, which runs on
    // every path because the copy is done inside a closure whose result is
    // only returned after unlocking.
    let lock = unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, READ_ONLY) };
    if lock != kCVReturnSuccess {
        return Err(decode_err(
            codec,
            format!("CVPixelBufferLockBaseAddress failed (CVReturn {lock})"),
        ));
    }

    let result = (|| -> Result<DecodedFrame, HwDecodeError> {
        let mut planes = Vec::with_capacity(geometry.len());
        for (index, &(row_bytes, rows)) in geometry.iter().enumerate() {
            let stride = CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, index);
            let plane_rows = CVPixelBufferGetHeightOfPlane(pixel_buffer, index);
            if stride < row_bytes || plane_rows < rows {
                return Err(decode_err(
                    codec,
                    format!(
                        "decoded surface plane {index} is {stride}x{plane_rows}, \
                         too small for {row_bytes}x{rows}"
                    ),
                ));
            }
            let base = CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, index).cast::<u8>();
            let plane = copy_plane(base, stride, row_bytes, rows).ok_or_else(|| {
                decode_err(
                    codec,
                    format!("decoded surface plane {index} is not readable"),
                )
            })?;
            planes.push(plane);
        }
        DecodedFrame::new(
            described.format,
            width,
            height,
            described.bit_depth,
            described.range,
            planes,
        )
    })();

    // SAFETY: the surface was locked read-only immediately above with these
    // exact flags and is unlocked exactly once here. Every slice borrowed from
    // it is dead — the planes were copied into owned `Vec`s.
    let _ = unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, READ_ONLY) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fourcc_table_maps_the_six_supported_surfaces() {
        let cases = [
            (0x3432_3076_u32, PixelFormat::Nv12, ColorRange::Limited, 8), // '420v'
            (0x3432_3066, PixelFormat::Nv12, ColorRange::Full, 8),        // '420f'
            (0x7834_3230, PixelFormat::P010, ColorRange::Limited, 10),    // 'x420'
            (0x7866_3230, PixelFormat::P010, ColorRange::Full, 10),       // 'xf20'
            (0x7934_3230, PixelFormat::I420, ColorRange::Limited, 8),     // 'y420'
            (0x6634_3230, PixelFormat::I420, ColorRange::Full, 8),        // 'f420'
        ];
        for (fourcc, format, range, bit_depth) in cases {
            let described = describe_format(fourcc)
                .unwrap_or_else(|| panic!("{} must be supported", fourcc_name(fourcc)));
            assert_eq!(described.format, format, "{}", fourcc_name(fourcc));
            assert_eq!(described.range, range, "{}", fourcc_name(fourcc));
            assert_eq!(described.bit_depth, bit_depth, "{}", fourcc_name(fourcc));
            // The table must agree with the crate's own depth rules.
            assert!(format.supports_bit_depth(bit_depth));
        }
    }

    #[test]
    fn unsupported_surfaces_are_rejected() {
        for fourcc in [
            0x2638_7630_u32, // '&8v0' lossless biplanar — no readable layout
            0x2d38_7630,     // '-8v0' lossy biplanar
            0x7834_3232,     // 'x422' 4:2:2 10-bit
            0x4241_4752,     // 'BAGR'
            0,
        ] {
            assert!(describe_format(fourcc).is_none(), "{fourcc:#010x}");
        }
    }

    #[test]
    fn fourcc_name_renders_ascii_and_falls_back_to_hex() {
        assert_eq!(fourcc_name(0x7834_3230), "'x420'");
        assert_eq!(fourcc_name(0x3432_3076), "'420v'");
        assert_eq!(fourcc_name(0), "0x00000000");
    }

    /// The geometry table must satisfy `DecodedFrame::new` exactly, including
    /// the ceiling division for odd dimensions.
    #[test]
    fn plane_geometry_satisfies_decoded_frame_validation() {
        for (format, bit_depth) in [
            (PixelFormat::Nv12, 8),
            (PixelFormat::P010, 10),
            (PixelFormat::I420, 8),
        ] {
            for (width, height) in [(5u32, 3u32), (64, 64), (61, 37), (1, 1)] {
                let geometry = plane_geometry(format, width, height);
                assert_eq!(geometry.len(), format.plane_count());
                let planes = geometry
                    .iter()
                    .map(|&(row_bytes, rows)| Plane {
                        data: vec![0u8; row_bytes * rows],
                        stride: row_bytes,
                    })
                    .collect();
                DecodedFrame::new(
                    format,
                    width,
                    height,
                    bit_depth,
                    ColorRange::Limited,
                    planes,
                )
                .unwrap_or_else(|e| panic!("{format:?} {width}x{height} must validate: {e}"));
            }
        }
    }

    #[test]
    fn copy_plane_tightens_a_padded_stride() {
        // 4x2 luma in a buffer with an 8-byte stride: the padding must not
        // survive into the returned plane.
        let src: Vec<u8> = vec![
            1, 2, 3, 4, 0xFF, 0xFF, 0xFF, 0xFF, // row 0 + padding
            5, 6, 7, 8, 0xFF, 0xFF, 0xFF, 0xFF, // row 1 + padding
        ];
        let plane = copy_plane(src.as_ptr(), 8, 4, 2).expect("plane copies");
        assert_eq!(plane.stride, 4);
        assert_eq!(plane.data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn copy_plane_reads_only_row_bytes_from_the_final_row() {
        // Exactly `stride * (rows - 1) + row_bytes` bytes are available — one
        // byte less than `stride * rows`. A naive implementation would read
        // past the end here.
        let src: Vec<u8> = vec![1, 2, 3, 0xFF, 4, 5, 6];
        let plane = copy_plane(src.as_ptr(), 4, 3, 2).expect("plane copies");
        assert_eq!(plane.data, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn copy_plane_rejects_bad_geometry() {
        let src = [0u8; 64];
        // Stride narrower than the used row width.
        assert!(copy_plane(src.as_ptr(), 2, 4, 2).is_none());
        // Zero rows.
        assert!(copy_plane(src.as_ptr(), 4, 4, 0).is_none());
        // Null base (a plane CoreVideo could not make CPU-addressable).
        assert!(copy_plane(std::ptr::null(), 4, 4, 2).is_none());
        // Overflowing geometry must not wrap.
        assert!(copy_plane(src.as_ptr(), usize::MAX, 4, 4).is_none());
    }
}
