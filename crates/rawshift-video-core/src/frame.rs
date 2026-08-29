//! Decoded frames.
//!
//! A [`VideoFrame`] owns its pixel data. Hardware decoders hand back planes
//! that live in a driver-owned surface, so the decoder copies them out before
//! the surface is recycled; a frame therefore never pins a decoder resource
//! and can outlive the decoder that produced it.

use gamut_color::{ColorRange, MatrixCoefficients};
use rawshift_core::Dimensions;

use crate::error::{VideoError, VideoResult};
use crate::id::VideoCodecId;

/// One plane of a decoded picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plane {
    /// Sample data, `stride` bytes per row.
    pub data: Vec<u8>,
    /// Bytes per row, which is at least the row's width in bytes and may
    /// exceed it: hardware decoders align rows.
    pub stride: usize,
}

/// The sample layout of a decoded picture.
///
/// These are the four 4:2:0 layouts hardware decoders produce. rawshift's own
/// type rather than the backend's, so swapping backends is not a breaking
/// change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FramePixelFormat {
    /// 8-bit, Y plane then interleaved CbCr. Two planes.
    Nv12,
    /// 10-bit in 16-bit words, Y plane then interleaved CbCr, value in the
    /// **most** significant bits. Two planes.
    P010,
    /// 8-bit planar Y, Cb, Cr. Three planes.
    I420,
    /// 10-bit planar Y, Cb, Cr in 16-bit words, value in the **least**
    /// significant bits. Three planes.
    I010,
}

impl FramePixelFormat {
    /// How many planes this layout has.
    #[must_use]
    pub const fn plane_count(self) -> usize {
        match self {
            Self::Nv12 | Self::P010 => 2,
            Self::I420 | Self::I010 => 3,
        }
    }

    /// Bytes per sample: 1 for the 8-bit layouts, 2 for the 10-bit ones.
    #[must_use]
    pub const fn bytes_per_sample(self) -> usize {
        match self {
            Self::Nv12 | Self::I420 => 1,
            Self::P010 | Self::I010 => 2,
        }
    }

    /// Whether chroma is stored interleaved (`NV12`/`P010`) rather than as two
    /// separate planes.
    #[must_use]
    pub const fn is_semi_planar(self) -> bool {
        matches!(self, Self::Nv12 | Self::P010)
    }

    /// How far to shift a stored 16-bit word right to recover the sample.
    ///
    /// `P010` parks the 10-bit value in the top of the word; `I010` stores it
    /// at the bottom. Zero for the 8-bit layouts.
    #[must_use]
    pub const fn sample_shift(self) -> u32 {
        match self {
            Self::P010 => 6,
            Self::Nv12 | Self::I420 | Self::I010 => 0,
        }
    }
}

/// Everything about a decoded picture except its pixels.
///
/// Exists so [`VideoFrame::new`] takes one descriptor rather than a row of
/// same-typed positional arguments, where transposing two is silent.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FrameDesc {
    /// The sample layout.
    pub format: FramePixelFormat,
    /// Picture dimensions.
    pub dimensions: Dimensions,
    /// Bits per sample.
    pub bit_depth: u8,
    /// Whether samples occupy the full code range or the studio subset.
    pub range: ColorRange,
    /// The matrix mapping YCbCr to RGB. Must already be resolved — never
    /// [`MatrixCoefficients::Unspecified`].
    pub matrix: MatrixCoefficients,
    /// Presentation timestamp of the packet this picture was coded from.
    pub pts: Option<i64>,
    /// Picture order count derived from the bitstream.
    pub poc: i32,
}

impl FrameDesc {
    /// A descriptor for a picture, with no timestamp and a zero picture order
    /// count.
    #[must_use]
    pub const fn new(
        format: FramePixelFormat,
        dimensions: Dimensions,
        bit_depth: u8,
        range: ColorRange,
        matrix: MatrixCoefficients,
    ) -> Self {
        Self {
            format,
            dimensions,
            bit_depth,
            range,
            matrix,
            pts: None,
            poc: 0,
        }
    }

    /// Set the presentation timestamp and picture order count.
    #[must_use]
    pub const fn with_timing(mut self, pts: Option<i64>, poc: i32) -> Self {
        self.pts = pts;
        self.poc = poc;
        self
    }
}

/// A decoded picture, in output (display) order.
///
/// Construct with [`VideoFrame::new`], which validates that the planes match
/// the format and dimensions — so every accessor below can be infallible.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoFrame {
    format: FramePixelFormat,
    dimensions: Dimensions,
    bit_depth: u8,
    range: ColorRange,
    matrix: MatrixCoefficients,
    planes: Vec<Plane>,
    pts: Option<i64>,
    poc: i32,
}

impl VideoFrame {
    /// Assemble a frame, validating it against its stated format.
    ///
    /// # Errors
    ///
    /// [`VideoError::Decode`] when the plane count does not match the format,
    /// or a plane's stride or length cannot hold the stated dimensions. This
    /// catches a backend that returned a surface inconsistent with what it
    /// claimed, which is otherwise an out-of-bounds read waiting to happen in
    /// every consumer.
    pub fn new(codec: VideoCodecId, desc: FrameDesc, planes: Vec<Plane>) -> VideoResult<Self> {
        let FrameDesc {
            format,
            dimensions,
            bit_depth,
            range,
            matrix,
            pts,
            poc,
        } = desc;

        if planes.len() != format.plane_count() {
            return Err(VideoError::decode(
                codec,
                format!(
                    "{format:?} needs {} planes, got {}",
                    format.plane_count(),
                    planes.len()
                ),
            ));
        }

        let bps = format.bytes_per_sample();
        let width = dimensions.width as usize;
        let height = dimensions.height as usize;
        // 4:2:0: chroma is half resolution on both axes, rounded up so odd
        // dimensions keep their final half-populated row and column.
        let chroma_width = width.div_ceil(2);
        let chroma_height = height.div_ceil(2);

        for (index, plane) in planes.iter().enumerate() {
            let (min_row_bytes, rows) = match (index, format.is_semi_planar()) {
                (0, _) => (width * bps, height),
                // Interleaved CbCr: one row holds both components.
                (_, true) => (chroma_width * 2 * bps, chroma_height),
                (_, false) => (chroma_width * bps, chroma_height),
            };

            if plane.stride < min_row_bytes {
                return Err(VideoError::decode(
                    codec,
                    format!(
                        "plane {index} stride {} is below the {min_row_bytes} bytes a row needs",
                        plane.stride
                    ),
                ));
            }
            // The last row need only be complete up to min_row_bytes; drivers
            // do not always pad it out to a full stride.
            let required = plane.stride * rows.saturating_sub(1) + min_row_bytes;
            if plane.data.len() < required {
                return Err(VideoError::decode(
                    codec,
                    format!(
                        "plane {index} holds {} bytes, needs {required} for {rows} rows",
                        plane.data.len()
                    ),
                ));
            }
        }

        Ok(Self {
            format,
            dimensions,
            bit_depth,
            range,
            matrix,
            planes,
            pts,
            poc,
        })
    }

    /// Picture width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.dimensions.width
    }

    /// Picture height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.dimensions.height
    }

    /// Picture dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> Dimensions {
        self.dimensions
    }

    /// The sample layout.
    #[must_use]
    pub const fn format(&self) -> FramePixelFormat {
        self.format
    }

    /// Bits per sample — 8 or 10 for the layouts rawshift decodes.
    #[must_use]
    pub const fn bit_depth(&self) -> u8 {
        self.bit_depth
    }

    /// Whether samples occupy the full code range or the studio subset.
    #[must_use]
    pub const fn range(&self) -> ColorRange {
        self.range
    }

    /// The matrix that maps this frame's YCbCr samples to RGB.
    ///
    /// Already resolved: never `Unspecified`.
    #[must_use]
    pub const fn matrix(&self) -> MatrixCoefficients {
        self.matrix
    }

    /// The planes, in the order the format defines.
    #[must_use]
    pub fn planes(&self) -> &[Plane] {
        &self.planes
    }

    /// The presentation timestamp of the packet this picture was coded from,
    /// in the track's `time_base` units.
    #[must_use]
    pub const fn pts(&self) -> Option<i64> {
        self.pts
    }

    /// The picture order count derived from the bitstream.
    ///
    /// Strictly increasing across the frames emitted between two random access
    /// points, which makes it the reliable ordering key even where a container
    /// carries absent or broken timestamps.
    #[must_use]
    pub const fn poc(&self) -> i32 {
        self.poc
    }

    /// Read one luma sample, or `None` if the coordinates are outside the
    /// picture.
    ///
    /// Returned on a 0–65535 scale regardless of the source bit depth, so
    /// callers need not branch on depth.
    #[must_use]
    pub fn luma(&self, x: u32, y: u32) -> Option<u16> {
        if x >= self.dimensions.width || y >= self.dimensions.height {
            return None;
        }
        let plane = self.planes.first()?;
        let (x, y) = (x as usize, y as usize);
        let sample = match self.format.bytes_per_sample() {
            1 => u16::from(*plane.data.get(y * plane.stride + x)?),
            _ => {
                let at = y * plane.stride + x * 2;
                let lo = *plane.data.get(at)?;
                let hi = *plane.data.get(at + 1)?;
                u16::from_le_bytes([lo, hi]) >> self.format.sample_shift()
            }
        };
        Some(scale_to_u16(sample, self.bit_depth))
    }
}

/// Scale a sample of `bit_depth` bits up to the full `u16` range.
///
/// Replicates the sample's own high bits into the vacated low bits rather than
/// shifting in zeros. A plain `<< 8` would map 8-bit 255 to 65280, so every
/// white in the picture would come out slightly grey and the scaling would not
/// be idempotent across depths.
fn scale_to_u16(sample: u16, bit_depth: u8) -> u16 {
    let depth = u32::from(bit_depth);
    if depth == 0 || depth >= 16 {
        return sample;
    }
    // Mask defensively: a backend that reports 10-bit while leaving junk in
    // the top 6 bits would otherwise scale that junk into the output.
    let value = u32::from(sample) & ((1u32 << depth) - 1);

    // Left-align, then tile the value downwards to fill the remaining bits.
    let mut out = value << (32 - depth);
    let mut filled = depth;
    while filled < 32 {
        out |= (value << (32 - depth)) >> filled;
        filled += depth;
    }
    (out >> 16) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims(w: u32, h: u32) -> Dimensions {
        Dimensions {
            width: w,
            height: h,
        }
    }

    fn nv12(width: usize, height: usize) -> Vec<Plane> {
        vec![
            Plane {
                data: vec![0u8; width * height],
                stride: width,
            },
            Plane {
                data: vec![0u8; width * height.div_ceil(2)],
                stride: width,
            },
        ]
    }

    fn frame_from(
        planes: Vec<Plane>,
        format: FramePixelFormat,
        w: u32,
        h: u32,
    ) -> VideoResult<VideoFrame> {
        let desc = FrameDesc::new(
            format,
            dims(w, h),
            8,
            ColorRange::Limited,
            MatrixCoefficients::Bt709,
        )
        .with_timing(Some(0), 0);
        VideoFrame::new(VideoCodecId::Hevc, desc, planes)
    }

    #[test]
    fn plane_counts_match_the_layouts() {
        assert_eq!(FramePixelFormat::Nv12.plane_count(), 2);
        assert_eq!(FramePixelFormat::P010.plane_count(), 2);
        assert_eq!(FramePixelFormat::I420.plane_count(), 3);
        assert_eq!(FramePixelFormat::I010.plane_count(), 3);
    }

    #[test]
    fn only_p010_parks_its_value_in_the_high_bits() {
        assert_eq!(FramePixelFormat::P010.sample_shift(), 6);
        assert_eq!(FramePixelFormat::I010.sample_shift(), 0);
        assert_eq!(FramePixelFormat::Nv12.sample_shift(), 0);
    }

    #[test]
    fn a_well_formed_frame_is_accepted() {
        let f = frame_from(nv12(64, 48), FramePixelFormat::Nv12, 64, 48)
            .expect("well-formed NV12 frame");
        assert_eq!(f.width(), 64);
        assert_eq!(f.height(), 48);
        assert_eq!(f.planes().len(), 2);
    }

    #[test]
    fn a_wrong_plane_count_is_rejected() {
        // NV12 needs two planes; hand it three.
        let mut planes = nv12(64, 48);
        planes.push(Plane {
            data: vec![0; 16],
            stride: 4,
        });
        let err = frame_from(planes, FramePixelFormat::Nv12, 64, 48)
            .expect_err("three planes is not NV12");
        assert!(err.to_string().contains("needs 2 planes"), "{err}");
    }

    #[test]
    fn a_stride_too_small_for_the_width_is_rejected() {
        let planes = vec![
            Plane {
                data: vec![0; 64 * 48],
                stride: 32, // half the 64 bytes a row needs
            },
            Plane {
                data: vec![0; 64 * 24],
                stride: 64,
            },
        ];
        let err =
            frame_from(planes, FramePixelFormat::Nv12, 64, 48).expect_err("understated stride");
        assert!(err.to_string().contains("stride"), "{err}");
    }

    #[test]
    fn a_truncated_plane_is_rejected() {
        let planes = vec![
            Plane {
                data: vec![0; 64 * 10], // 10 rows where 48 are claimed
                stride: 64,
            },
            Plane {
                data: vec![0; 64 * 24],
                stride: 64,
            },
        ];
        let err =
            frame_from(planes, FramePixelFormat::Nv12, 64, 48).expect_err("truncated luma plane");
        assert!(err.to_string().contains("needs"), "{err}");
    }

    #[test]
    fn an_aligned_stride_larger_than_the_width_is_fine() {
        // Drivers align rows; a 64-wide picture in a 128-byte stride is normal.
        let planes = vec![
            Plane {
                data: vec![0; 128 * 48],
                stride: 128,
            },
            Plane {
                data: vec![0; 128 * 24],
                stride: 128,
            },
        ];
        assert!(frame_from(planes, FramePixelFormat::Nv12, 64, 48).is_ok());
    }

    #[test]
    fn odd_dimensions_round_chroma_up() {
        // A 65x49 picture has 33x25 chroma, not 32x24 — rounding down would
        // drop the last column and row.
        let planes = vec![
            Plane {
                data: vec![0; 65 * 49],
                stride: 65,
            },
            Plane {
                data: vec![0; 66 * 25],
                stride: 66,
            },
        ];
        assert!(frame_from(planes, FramePixelFormat::Nv12, 65, 49).is_ok());
    }

    #[test]
    fn the_final_row_need_not_be_padded_to_a_full_stride() {
        // A driver may hand back exactly enough bytes to hold the last row's
        // pixels, with no trailing alignment padding.
        let stride = 128;
        let planes = vec![
            Plane {
                data: vec![0; stride * 47 + 64],
                stride,
            },
            Plane {
                data: vec![0; stride * 23 + 64],
                stride,
            },
        ];
        assert!(frame_from(planes, FramePixelFormat::Nv12, 64, 48).is_ok());
    }

    #[test]
    fn luma_reads_8_bit_samples_scaled_to_full_range() {
        let mut planes = nv12(4, 2);
        planes[0].data[0] = 255;
        planes[0].data[1] = 0;
        let f = frame_from(planes, FramePixelFormat::Nv12, 4, 2).expect("frame");
        // 8-bit 255 must reach u16::MAX, not 65280.
        assert_eq!(f.luma(0, 0), Some(u16::MAX));
        assert_eq!(f.luma(1, 0), Some(0));
    }

    #[test]
    fn luma_is_none_outside_the_picture() {
        let f = frame_from(nv12(4, 2), FramePixelFormat::Nv12, 4, 2).expect("frame");
        assert_eq!(f.luma(4, 0), None);
        assert_eq!(f.luma(0, 2), None);
    }

    #[test]
    fn scaling_maps_extremes_to_extremes_at_every_depth() {
        for depth in [1u8, 4, 8, 10, 12, 15, 16] {
            let max = ((1u32 << depth) - 1) as u16;
            assert_eq!(scale_to_u16(0, depth), 0, "depth {depth} floor");
            assert_eq!(scale_to_u16(max, depth), u16::MAX, "depth {depth} ceiling");
        }
    }

    #[test]
    fn scaling_replicates_bits_rather_than_shifting_in_zeros() {
        // The textbook cases: 8-bit replicates the byte, 10-bit borrows its
        // own top 6 bits, 12-bit its own top 4.
        assert_eq!(scale_to_u16(0xAB, 8), 0xABAB);
        assert_eq!(scale_to_u16(0b11_1111_0000, 10), 0b1111_1100_0011_1111);
        assert_eq!(scale_to_u16(0x800, 12), 0x8008);
    }

    #[test]
    fn scaling_is_monotonic() {
        // Ordering must survive scaling, or gradients would band.
        let mut previous = 0;
        for value in 0..=1023u16 {
            let scaled = scale_to_u16(value, 10);
            assert!(scaled >= previous, "value {value} went backwards");
            previous = scaled;
        }
    }

    #[test]
    fn scaling_ignores_junk_above_the_stated_depth() {
        // A backend claiming 10 bits but leaving the top 6 set must not have
        // that junk scaled into the result.
        assert_eq!(scale_to_u16(0x03FF, 10), scale_to_u16(0xFFFF, 10));
    }

    #[test]
    fn full_and_zero_depth_pass_through_untouched() {
        assert_eq!(scale_to_u16(12345, 16), 12345);
        assert_eq!(scale_to_u16(12345, 0), 12345);
    }
}
