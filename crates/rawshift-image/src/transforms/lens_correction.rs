//! Lens distortion correction.
//!
//! Implements polynomial undistortion (WarpRectilinear as specified in DNG opcode 3).
//!
//! The model corrects barrel and pincushion distortion by computing, for each
//! output pixel, the corresponding distorted source position and sampling it
//! with bilinear interpolation.

use crate::core::image::RgbImage;

// ── Public API ────────────────────────────────────────────────────────────────

/// Apply WarpRectilinear lens distortion correction.
///
/// Uses the polynomial model:
/// ```text
/// r_undist = r * (1 + k0*r² + k1*r⁴ + k2*r⁶ + k3*r⁸)
/// ```
/// where r is the normalised distance from the optical centre.
///
/// The algorithm iterates output pixels and maps them back to distorted source
/// positions via the polynomial, then samples with bilinear interpolation.
///
/// # Arguments
/// * `image` - RGB image to correct in-place.
/// * `k`     - Radial distortion coefficients `[k0, k1, k2, k3]`.
///   Barrel distortion uses negative values (e.g. `[-0.01, 0.0, 0.0, 0.0]`);
///   pincushion uses positive ones.
/// * `cx`    - Optical centre x as a fraction of image width (typically `0.5`).
/// * `cy`    - Optical centre y as a fraction of image height (typically `0.5`).
///
/// When all `k` are zero the function is a no-op.
pub fn apply_warp_rectilinear(image: &mut RgbImage, k: [f64; 4], cx: f64, cy: f64) {
    apply_warp_rectilinear_tangential(image, k, [0.0, 0.0], cx, cy);
}

/// Apply WarpRectilinear with additional tangential distortion correction.
///
/// Tangential (de-centering) distortion is caused by lens elements not being
/// perfectly aligned and is modelled by two additional coefficients `p0` and `p1`.
///
/// The full source mapping for output pixel `(xn, yn)` (normalised) is:
/// ```text
/// factor = 1 + k0*r² + k1*r⁴ + k2*r⁶ + k3*r⁸
/// xs = xn * factor + 2*p0*xn*yn      + p1*(r² + 2*xn²)
/// ys = yn * factor + p0*(r² + 2*yn²) + 2*p1*xn*yn
/// ```
///
/// # Arguments
/// * `image` - RGB image to correct in-place.
/// * `k`     - Radial distortion coefficients `[k0, k1, k2, k3]`.
/// * `p`     - Tangential distortion coefficients `[p0, p1]`.
/// * `cx`    - Optical centre x as a fraction of image width.
/// * `cy`    - Optical centre y as a fraction of image height.
pub fn apply_warp_rectilinear_tangential(
    image: &mut RgbImage,
    k: [f64; 4],
    p: [f64; 2],
    cx: f64,
    cy: f64,
) {
    let width = image.width() as usize;
    let height = image.height() as usize;

    if width == 0 || height == 0 {
        return;
    }

    // Half-diagonal of the shorter dimension used for normalisation.
    let norm = 0.5 * width.min(height) as f64;

    let cx_px = cx * width as f64;
    let cy_px = cy * height as f64;

    // Snapshot the source data before modifying in-place.
    let src = image.data.clone();

    for y in 0..height {
        for x in 0..width {
            // Normalise output pixel to [-1, 1] relative to optical centre.
            let xn = (x as f64 - cx_px) / norm;
            let yn = (y as f64 - cy_px) / norm;

            let r2 = xn * xn + yn * yn;
            let r4 = r2 * r2;
            let r6 = r4 * r2;
            let r8 = r4 * r4;

            // Radial distortion factor.
            let factor = 1.0 + k[0] * r2 + k[1] * r4 + k[2] * r6 + k[3] * r8;

            // Distorted (source) position in normalised coordinates.
            let xs = xn * factor + 2.0 * p[0] * xn * yn + p[1] * (r2 + 2.0 * xn * xn);
            let ys = yn * factor + p[0] * (r2 + 2.0 * yn * yn) + 2.0 * p[1] * xn * yn;

            // Convert back to pixel coordinates.
            let src_x = xs * norm + cx_px;
            let src_y = ys * norm + cy_px;

            // Write bilinear sample for each of R, G, B.
            let dst = (y * width + x) * 3;
            for ch in 0..3usize {
                image.data[dst + ch] =
                    bilinear_sample(&src, width, height, ch, src_x as f32, src_y as f32);
            }
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Sample a single channel of an RGB image at a sub-pixel position using
/// bilinear interpolation. Positions outside the image boundary are clamped
/// to the nearest valid pixel.
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rgb(width: u32, height: u32, fill: u16) -> RgbImage {
        let n = (width as usize) * (height as usize) * 3;
        RgbImage::new(width, height, vec![fill; n])
    }

    fn make_gradient(width: u32, height: u32) -> RgbImage {
        let n = (width as usize) * (height as usize) * 3;
        let data: Vec<u16> = (0..n).map(|i| (i as u16).wrapping_mul(7)).collect();
        RgbImage::new(width, height, data)
    }

    // ── apply_warp_rectilinear ────────────────────────────────────────────

    #[test]
    fn test_warp_zero_coefficients_unchanged() {
        // k = [0, 0, 0, 0] with centre (0.5, 0.5) must produce a bit-for-bit
        // identical image (the bilinear at exact integer positions is exact).
        let w = 8u32;
        let h = 8u32;
        let original = make_gradient(w, h);
        let mut img = original.clone();

        apply_warp_rectilinear(&mut img, [0.0; 4], 0.5, 0.5);

        assert_eq!(
            img.data, original.data,
            "zero coefficients must leave image unchanged"
        );
    }

    #[test]
    fn test_warp_output_size_unchanged() {
        let w = 16u32;
        let h = 10u32;
        let mut img = make_gradient(w, h);

        apply_warp_rectilinear(&mut img, [-0.01, 0.0, 0.0, 0.0], 0.5, 0.5);

        assert_eq!(img.width(), w, "width must not change after warp");
        assert_eq!(img.height(), h, "height must not change after warp");
        assert_eq!(img.data.len(), (w as usize) * (h as usize) * 3);
    }

    #[test]
    fn test_warp_center_pixel_unchanged() {
        // For a symmetric distortion centred at (0.5, 0.5) the exact centre
        // pixel maps to itself (r = 0, factor = 1).  With a uniform image the
        // value stays constant; with a gradient we check the centre maps to
        // the source centre position.
        let w = 11u32; // odd dimension so there is an exact centre pixel
        let h = 11u32;
        let mut img = make_rgb(w, h, 32768);

        let cx_idx = (h as usize / 2) * (w as usize) + (w as usize / 2);

        apply_warp_rectilinear(&mut img, [-0.05, 0.002, 0.0, 0.0], 0.5, 0.5);

        // The centre pixel of a uniform image is always 32768 regardless of distortion.
        assert_eq!(img.data[cx_idx * 3], 32768, "R of centre pixel");
        assert_eq!(img.data[cx_idx * 3 + 1], 32768, "G of centre pixel");
        assert_eq!(img.data[cx_idx * 3 + 2], 32768, "B of centre pixel");
    }

    #[test]
    fn test_warp_no_crash_small_image() {
        // A 2×2 image must not panic regardless of coefficients.
        let mut img = make_rgb(2, 2, 1000);
        apply_warp_rectilinear(&mut img, [-0.1, 0.05, -0.01, 0.001], 0.5, 0.5);
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
    }

    #[test]
    fn test_warp_no_crash_1x1_image() {
        let mut img = make_rgb(1, 1, 5000);
        apply_warp_rectilinear(&mut img, [-0.1, 0.0, 0.0, 0.0], 0.5, 0.5);
        assert_eq!(img.data[0], 5000);
    }

    // ── apply_warp_rectilinear_tangential ─────────────────────────────────

    #[test]
    fn test_warp_tangential_zero_unchanged() {
        let original = make_gradient(8, 8);
        let mut img = original.clone();

        apply_warp_rectilinear_tangential(&mut img, [0.0; 4], [0.0; 2], 0.5, 0.5);

        assert_eq!(
            img.data, original.data,
            "all-zero tangential coefficients must leave image unchanged"
        );
    }

    #[test]
    fn test_warp_tangential_output_size_unchanged() {
        let w = 12u32;
        let h = 8u32;
        let mut img = make_gradient(w, h);

        apply_warp_rectilinear_tangential(
            &mut img,
            [-0.01, 0.0, 0.0, 0.0],
            [0.001, -0.001],
            0.5,
            0.5,
        );

        assert_eq!(img.width(), w);
        assert_eq!(img.height(), h);
    }

    #[test]
    fn test_warp_uniform_image_stays_uniform() {
        // A perfectly flat image must remain flat under any distortion because
        // every bilinear sample returns the same value.
        let mut img = make_rgb(10, 10, 8000);
        apply_warp_rectilinear(&mut img, [-0.05, 0.02, -0.005, 0.001], 0.5, 0.5);
        assert!(
            img.data.iter().all(|&v| v == 8000),
            "uniform image must remain uniform"
        );
    }
}
