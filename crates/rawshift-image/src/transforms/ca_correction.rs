//! Chromatic aberration correction.
//!
//! Lateral (geometric) chromatic aberration causes the R and B channels to be
//! imaged at slightly different scales than the G channel, producing colour
//! fringing near high-contrast edges. This module corrects the artefact by
//! independently rescaling the R and B channels relative to the image centre,
//! using bilinear interpolation to sample the shifted source positions.

use crate::core::RgbImage;

/// Apply lateral chromatic aberration correction by rescaling colour channels.
///
/// Each colour plane is scaled (zoomed) relative to the image centre
/// independently. The G channel is used as the reference and is left untouched.
/// When `red_scale == 1.0` and `blue_scale == 1.0` the output is identical to
/// the input.
///
/// The mapping from output pixel (x, y) to source pixel (sx, sy) is:
///
/// ```text
/// sx = cx + (x - cx) / scale
/// sy = cy + (y - cy) / scale
/// ```
///
/// where (cx, cy) is the image centre. Bilinear interpolation is used to
/// evaluate the source at non-integer positions.
///
/// # Arguments
/// * `image`      - RGB image to correct in-place.
/// * `red_scale`  - Relative scale factor for the R channel (typical: 0.999–1.001).
/// * `blue_scale` - Relative scale factor for the B channel (typical: 0.999–1.001).
pub fn apply_ca_correction(image: &mut RgbImage, red_scale: f32, blue_scale: f32) {
    let width = image.width() as usize;
    let height = image.height() as usize;

    if width == 0 || height == 0 {
        return;
    }

    let cx = (width as f32 - 1.0) * 0.5;
    let cy = (height as f32 - 1.0) * 0.5;

    let input = image.data().to_vec();
    let data = image.data_mut();

    let scales = [(0usize, red_scale), (2usize, blue_scale)];

    for (channel, scale) in scales {
        if (scale - 1.0_f32).abs() < 1e-6 {
            continue; // identity – skip for speed
        }

        let inv_scale = 1.0_f32 / scale;

        for y in 0..height {
            for x in 0..width {
                let sx = cx + (x as f32 - cx) * inv_scale;
                let sy = cy + (y as f32 - cy) * inv_scale;

                let value = bilinear_sample(&input, width, height, channel, sx, sy);
                data[(y * width + x) * 3 + channel] = value;
            }
        }
    }
}

/// Sample a single channel of an RGB image at a sub-pixel position using
/// bilinear interpolation.
///
/// Positions outside the image boundary are clamped to the nearest valid pixel.
#[inline]
fn bilinear_sample(
    data: &[u16],
    width: usize,
    height: usize,
    channel: usize,
    sx: f32,
    sy: f32,
) -> u16 {
    // Clamp source coordinates to valid range.
    let sx = sx.clamp(0.0, (width as f32) - 1.0);
    let sy = sy.clamp(0.0, (height as f32) - 1.0);

    let x0 = sx.floor() as usize;
    let y0 = sy.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);

    let fx = sx - sx.floor();
    let fy = sy - sy.floor();

    let p00 = data[(y0 * width + x0) * 3 + channel] as f32;
    let p10 = data[(y0 * width + x1) * 3 + channel] as f32;
    let p01 = data[(y1 * width + x0) * 3 + channel] as f32;
    let p11 = data[(y1 * width + x1) * 3 + channel] as f32;

    let top = p00 * (1.0 - fx) + p10 * fx;
    let bot = p01 * (1.0 - fx) + p11 * fx;
    let result = top * (1.0 - fy) + bot * fy;

    result.round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rgb(width: u32, height: u32, fill: u16) -> RgbImage {
        let n = (width as usize) * (height as usize) * 3;
        RgbImage::new(width, height, vec![fill; n]).expect("valid RGB buffer")
    }

    #[test]
    fn test_ca_correction_scale_1_unchanged() {
        // Identity scales must produce bit-for-bit identical output.
        let w = 8u32;
        let h = 8u32;
        let n = (w as usize) * (h as usize) * 3;
        // Use a non-trivial pattern so any change would be visible.
        let data: Vec<u16> = (0..n).map(|i| (i as u16).wrapping_mul(7)).collect();
        let mut img = RgbImage::new(w, h, data.clone()).expect("valid RGB buffer");
        apply_ca_correction(&mut img, 1.0, 1.0);
        assert_eq!(img.data(), data, "scale 1.0 should leave image unchanged");
    }

    #[test]
    fn test_ca_correction_no_crash_small() {
        // Correction on very small images must not panic.
        let mut img = make_rgb(2, 2, 1000);
        apply_ca_correction(&mut img, 0.999, 1.001);
        // Just verify the image is still 2×2 and no panic occurred.
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
    }

    #[test]
    fn test_ca_correction_output_size_unchanged() {
        let w = 16u32;
        let h = 10u32;
        let mut img = make_rgb(w, h, 500);
        apply_ca_correction(&mut img, 1.002, 0.998);
        assert_eq!(img.width(), w, "width must not change");
        assert_eq!(img.height(), h, "height must not change");
        assert_eq!(img.data().len(), (w as usize) * (h as usize) * 3);
    }

    #[test]
    fn test_ca_correction_uniform_image_unchanged() {
        // A flat image should remain flat regardless of scale.
        let mut img = make_rgb(12, 12, 8000);
        apply_ca_correction(&mut img, 1.005, 0.995);
        assert!(img.data().iter().all(|&v| v == 8000));
    }

    #[test]
    fn test_ca_correction_green_channel_unmodified() {
        // The G channel (index 1) must always remain unchanged.
        let w = 8u32;
        let h = 8u32;
        let n = (w as usize) * (h as usize) * 3;
        let data: Vec<u16> = (0..n).map(|i| i as u16).collect();
        let original_green: Vec<u16> = data.chunks_exact(3).map(|px| px[1]).collect();
        let mut img = RgbImage::new(w, h, data).expect("valid RGB buffer");
        apply_ca_correction(&mut img, 1.005, 0.995);
        let corrected_green: Vec<u16> = img.data().chunks_exact(3).map(|px| px[1]).collect();
        assert_eq!(
            original_green, corrected_green,
            "G channel must not be modified"
        );
    }

    #[test]
    fn test_ca_correction_1x1_image() {
        let mut img = make_rgb(1, 1, 1234);
        apply_ca_correction(&mut img, 1.01, 0.99);
        // Only one pixel; it should remain at the clamped bilinear sample of itself.
        assert_eq!(img.data()[0], 1234);
        assert_eq!(img.data()[2], 1234);
    }
}
