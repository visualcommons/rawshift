//! Shared plane-lifting helpers for the hardware still-decode adapters.
//!
//! [`rawshift_hwdec`] surfaces every decoded picture as byte planes with
//! strides in one of four 4:2:0-container formats (NV12 / P010 / I420 /
//! I010). Both container adapters — HEIC's `HwHevcAdapter` and AVIF's
//! `HwAv1Adapter` — lift those planes into their gamut pipeline's uniform
//! `u16` planar frame; the raw plane readers are identical, so they live
//! here once. The frame *assembly* (which chroma format to present, how a
//! monochrome coded stream drops the surface's neutral chroma planes) stays
//! with each adapter, next to its codec's rules.
//!
//! Also hosts [`rgba8_to_rgb_image`], the shared final presentation step from
//! gamut's 8-bit RGBA output to rawshift's 16-bit [`RgbImage`].

use rawshift_hwdec::Plane;

use crate::RawResult;
use crate::RgbImage;

/// Copy an 8-bit plane into `u16` samples, honouring the row stride.
pub(crate) fn read_plane_8(plane: &Plane, width: usize, rows: usize) -> Vec<u16> {
    let mut out = Vec::with_capacity(width * rows);
    for row in 0..rows {
        let start = row * plane.stride;
        out.extend(
            plane.data[start..start + width]
                .iter()
                .map(|&b| u16::from(b)),
        );
    }
    out
}

/// Split an 8-bit interleaved CbCr plane into separate Cb/Cr samples.
pub(crate) fn deinterleave_8(plane: &Plane, width: usize, rows: usize) -> (Vec<u16>, Vec<u16>) {
    let mut cb = Vec::with_capacity(width * rows);
    let mut cr = Vec::with_capacity(width * rows);
    for row in 0..rows {
        let start = row * plane.stride;
        for pair in plane.data[start..start + width * 2].chunks_exact(2) {
            cb.push(u16::from(pair[0]));
            cr.push(u16::from(pair[1]));
        }
    }
    (cb, cr)
}

/// Copy a 16-bit-word plane into `u16` samples: little-endian words,
/// shifted down by `shift` (P010 keeps the value in the high bits) and
/// masked to `bit_depth`.
pub(crate) fn read_plane_16(
    plane: &Plane,
    width: usize,
    rows: usize,
    shift: u32,
    bit_depth: u8,
) -> Vec<u16> {
    let mask = sample_mask(bit_depth);
    let mut out = Vec::with_capacity(width * rows);
    for row in 0..rows {
        let start = row * plane.stride;
        for word in plane.data[start..start + width * 2].chunks_exact(2) {
            out.push((u16::from_le_bytes([word[0], word[1]]) >> shift) & mask);
        }
    }
    out
}

/// Split a 16-bit-word interleaved CbCr plane into Cb/Cr samples.
pub(crate) fn deinterleave_16(
    plane: &Plane,
    width: usize,
    rows: usize,
    shift: u32,
    bit_depth: u8,
) -> (Vec<u16>, Vec<u16>) {
    let mask = sample_mask(bit_depth);
    let mut cb = Vec::with_capacity(width * rows);
    let mut cr = Vec::with_capacity(width * rows);
    for row in 0..rows {
        let start = row * plane.stride;
        for pair in plane.data[start..start + width * 4].chunks_exact(4) {
            cb.push((u16::from_le_bytes([pair[0], pair[1]]) >> shift) & mask);
            cr.push((u16::from_le_bytes([pair[2], pair[3]]) >> shift) & mask);
        }
    }
    (cb, cr)
}

/// The all-ones mask for `bit_depth`-bit samples (identity for 16).
fn sample_mask(bit_depth: u8) -> u16 {
    if bit_depth >= 16 {
        u16::MAX
    } else {
        (1u16 << bit_depth) - 1
    }
}

/// Convert a gamut presentation output (8-bit RGBA, transforms applied) to
/// rawshift's 16-bit [`RgbImage`]: alpha dropped, samples scaled by `*257`
/// (exact at both endpoints).
pub(crate) fn rgba8_to_rgb_image(
    rgba: &gamut_core::ImageBuf<gamut_core::Rgba8>,
) -> RawResult<RgbImage> {
    let (width, height) = (rgba.width(), rgba.height());
    let samples = rgba.as_samples();
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for px in samples.chunks_exact(4) {
        rgb.push(u16::from(px[0]) * 257);
        rgb.push(u16::from(px[1]) * 257);
        rgb.push(u16::from(px[2]) * 257);
    }
    RgbImage::new(width, height, rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_readers_honour_strides_and_shifts() {
        // 8-bit 2x2 plane with a 4-byte stride (2 padding bytes per row).
        let p8 = Plane {
            data: vec![1, 2, 0xEE, 0xEE, 3, 4, 0xEE, 0xEE],
            stride: 4,
        };
        assert_eq!(read_plane_8(&p8, 2, 2), vec![1, 2, 3, 4]);

        // Interleaved 8-bit CbCr: one row of two pairs.
        let cbcr = Plane {
            data: vec![10, 20, 30, 40],
            stride: 4,
        };
        assert_eq!(deinterleave_8(&cbcr, 2, 1), (vec![10, 30], vec![20, 40]));

        // 10-bit value 612 stored P010-style (<< 6) in little-endian words.
        let word = (612u16 << 6).to_le_bytes();
        let p16 = Plane {
            data: [word, word].concat(),
            stride: 4,
        };
        assert_eq!(read_plane_16(&p16, 2, 1, 6, 10), vec![612, 612]);
        let (cb, cr) = deinterleave_16(&p16, 1, 1, 6, 10);
        assert_eq!((cb, cr), (vec![612], vec![612]));
    }
}
