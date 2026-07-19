//! Noise reduction algorithms for RGB image data.
//!
//! This module provides spatial noise reduction filters that operate on fully
//! demosaiced RGB images. The primary algorithm is the bilateral filter, which
//! preserves edges while smoothing noise by weighting contributions from both
//! spatial proximity and intensity similarity.

use crate::core::RgbImage;

/// Apply bilateral noise reduction filter to an RGB image.
///
/// The bilateral filter replaces each pixel with a weighted average of its
/// neighbors. The weight for each neighbor is the product of a spatial Gaussian
/// (based on pixel distance) and a range Gaussian (based on intensity difference).
/// This allows the filter to smooth flat regions aggressively while leaving
/// sharp edges largely intact.
///
/// # Arguments
/// * `image`          - RGB image to filter in-place.
/// * `spatial_sigma`  - Spatial Gaussian sigma in pixels (typically 2–5).
/// * `range_sigma`    - Range/intensity sigma in u16 units (typically 1000–5000).
/// * `radius` - Filter half-width in pixels; the kernel covers `(2*radius+1)²` pixels.
///
/// # Performance
/// Complexity is O(width × height × (2·radius+1)²). For small radii (2–3) this
/// is fast; for larger radii consider a separable approximation.
pub fn apply_bilateral_filter(
    image: &mut RgbImage,
    spatial_sigma: f32,
    range_sigma: f32,
    radius: u32,
) {
    let width = image.width() as usize;
    let height = image.height() as usize;

    if width == 0 || height == 0 {
        return;
    }

    let r = radius as usize;

    // Pre-compute spatial Gaussian weights for all (dx, dy) offsets in the kernel.
    let ksize = 2 * r + 1;
    let two_ss_sq = 2.0_f32 * spatial_sigma * spatial_sigma;
    let two_rs_sq = 2.0_f32 * range_sigma * range_sigma;

    let mut spatial_lut = vec![0.0_f32; ksize * ksize];
    for dy in 0..ksize {
        for dx in 0..ksize {
            let fx = dx as f32 - r as f32;
            let fy = dy as f32 - r as f32;
            let d2 = fx * fx + fy * fy;
            spatial_lut[dy * ksize + dx] = (-d2 / two_ss_sq).exp();
        }
    }

    let input = image.data().to_vec();
    let data = image.data_mut();

    for y in 0..height {
        for x in 0..width {
            let center_idx = (y * width + x) * 3;

            for c in 0..3usize {
                let center_val = input[center_idx + c] as f32;
                let mut numerator = 0.0_f32;
                let mut denominator = 0.0_f32;

                let y_min = y.saturating_sub(r);
                let y_max = (y + r).min(height - 1);
                let x_min = x.saturating_sub(r);
                let x_max = (x + r).min(width - 1);

                for ny in y_min..=y_max {
                    for nx in x_min..=x_max {
                        let lut_dy = (ny as isize - y as isize + r as isize) as usize;
                        let lut_dx = (nx as isize - x as isize + r as isize) as usize;
                        let spatial_w = spatial_lut[lut_dy * ksize + lut_dx];

                        let neighbor_val = input[(ny * width + nx) * 3 + c] as f32;
                        let diff = center_val - neighbor_val;
                        let range_w = (-diff * diff / two_rs_sq).exp();

                        let w = spatial_w * range_w;
                        numerator += neighbor_val * w;
                        denominator += w;
                    }
                }

                let result = if denominator > 0.0 {
                    (numerator / denominator).round() as u16
                } else {
                    input[center_idx + c]
                };

                data[center_idx + c] = result;
            }
        }
    }
}

/// Apply a simple Gaussian blur for fast noise reduction (lower quality).
///
/// Unlike the bilateral filter this does not preserve edges, but is faster
/// and simpler. Useful as a preview step or for very high-ISO data where
/// edge sharpness matters less.
///
/// # Arguments
/// * `image`  - RGB image to filter in-place.
/// * `sigma`  - Gaussian sigma in pixels.
/// * `radius` - Filter half-width in pixels.
pub fn apply_gaussian_blur(image: &mut RgbImage, sigma: f32, radius: u32) {
    let width = image.width() as usize;
    let height = image.height() as usize;

    if width == 0 || height == 0 {
        return;
    }

    let r = radius as usize;
    let ksize = 2 * r + 1;
    let two_sq = 2.0_f32 * sigma * sigma;

    // Build 1-D Gaussian kernel (normalized).
    let mut kernel = vec![0.0_f32; ksize];
    for (i, v) in kernel.iter_mut().enumerate() {
        let x = i as f32 - r as f32;
        *v = (-x * x / two_sq).exp();
    }
    let k_sum: f32 = kernel.iter().sum();
    for v in kernel.iter_mut() {
        *v /= k_sum;
    }

    // Horizontal pass.
    let data = image.data_mut();
    let mut tmp = data.to_vec();
    for y in 0..height {
        for x in 0..width {
            for c in 0..3usize {
                let mut acc = 0.0_f32;
                let mut wsum = 0.0_f32;
                let x_min = x.saturating_sub(r);
                let x_max = (x + r).min(width - 1);
                for nx in x_min..=x_max {
                    let ki = (nx as isize - x as isize + r as isize) as usize;
                    let w = kernel[ki];
                    acc += data[(y * width + nx) * 3 + c] as f32 * w;
                    wsum += w;
                }
                tmp[(y * width + x) * 3 + c] = if wsum > 0.0 {
                    (acc / wsum).round() as u16
                } else {
                    data[(y * width + x) * 3 + c]
                };
            }
        }
    }

    // Vertical pass.
    for y in 0..height {
        for x in 0..width {
            for c in 0..3usize {
                let mut acc = 0.0_f32;
                let mut wsum = 0.0_f32;
                let y_min = y.saturating_sub(r);
                let y_max = (y + r).min(height - 1);
                for ny in y_min..=y_max {
                    let ki = (ny as isize - y as isize + r as isize) as usize;
                    let w = kernel[ki];
                    acc += tmp[(ny * width + x) * 3 + c] as f32 * w;
                    wsum += w;
                }
                data[(y * width + x) * 3 + c] = if wsum > 0.0 {
                    (acc / wsum).round() as u16
                } else {
                    tmp[(y * width + x) * 3 + c]
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a uniform RGB image.
    fn make_uniform(width: u32, height: u32, value: u16) -> RgbImage {
        let n = (width as usize) * (height as usize) * 3;
        RgbImage::new(width, height, vec![value; n]).expect("valid RGB buffer")
    }

    /// Compute the variance of the values in the given channel (0, 1, or 2).
    fn channel_variance(image: &RgbImage, channel: usize) -> f64 {
        let vals: Vec<f64> = image
            .data()
            .chunks_exact(3)
            .map(|px| px[channel] as f64)
            .collect();
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        vals.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64
    }

    #[test]
    fn test_bilateral_uniform_image_unchanged() {
        // A perfectly uniform image should remain unchanged.
        let mut img = make_uniform(8, 8, 1000);
        apply_bilateral_filter(&mut img, 2.0, 2000.0, 2);
        assert!(img.data().iter().all(|&v| v == 1000));
    }

    #[test]
    fn test_bilateral_reduces_noise() {
        // Add alternating noise: even pixels 1000, odd pixels 2000.
        let w = 16u32;
        let h = 16u32;
        let n = (w as usize) * (h as usize);
        let mut data = Vec::with_capacity(n * 3);
        for i in 0..n {
            let v = if i % 2 == 0 { 1000u16 } else { 2000u16 };
            data.extend_from_slice(&[v, v, v]);
        }
        let mut img = RgbImage::new(w, h, data.clone()).expect("valid RGB buffer");
        let var_before = channel_variance(&img, 0);
        apply_bilateral_filter(&mut img, 3.0, 5000.0, 3);
        let var_after = channel_variance(&img, 0);
        assert!(
            var_after < var_before,
            "variance should decrease: before={var_before}, after={var_after}"
        );
    }

    #[test]
    fn test_bilateral_preserves_edges() {
        // Left half = 0, right half = 65000. After bilateral the edge pixels
        // should still differ substantially.
        let w = 16u32;
        let h = 8u32;
        let n = (w as usize) * (h as usize);
        let mut data = Vec::with_capacity(n * 3);
        for _y in 0..h {
            for x in 0..w {
                let v = if x < w / 2 { 0u16 } else { 60000u16 };
                data.extend_from_slice(&[v, v, v]);
            }
        }
        let mut img = RgbImage::new(w, h, data).expect("valid RGB buffer");
        apply_bilateral_filter(&mut img, 2.0, 1000.0, 2);

        // Pixel in the left half should stay dark.
        let left_px = img.data()[((4 * w as usize) + 2) * 3];
        // Pixel in the right half should stay bright.
        let right_px = img.data()[((4 * w as usize) + 13) * 3];
        assert!(left_px < 10000, "left edge should stay dark, got {left_px}");
        assert!(
            right_px > 50000,
            "right edge should stay bright, got {right_px}"
        );
    }

    #[test]
    fn test_gaussian_blur_uniform_unchanged() {
        let mut img = make_uniform(8, 8, 5000);
        apply_gaussian_blur(&mut img, 1.5, 2);
        // A uniform image blurred with any kernel is still uniform.
        assert!(img.data().iter().all(|&v| v == 5000));
    }

    #[test]
    fn test_filter_small_image() {
        // 1×1 and 2×2 images must not crash.
        let mut img1 = make_uniform(1, 1, 100);
        apply_bilateral_filter(&mut img1, 2.0, 1000.0, 2);

        let mut img2 = make_uniform(2, 2, 200);
        apply_gaussian_blur(&mut img2, 1.0, 2);
    }

    #[test]
    fn test_gaussian_blur_reduces_noise() {
        let w = 16u32;
        let h = 16u32;
        let n = (w as usize) * (h as usize);
        let mut data = Vec::with_capacity(n * 3);
        for i in 0..n {
            let v: u16 = if i % 2 == 0 { 1000 } else { 3000 };
            data.extend_from_slice(&[v, v, v]);
        }
        let mut img = RgbImage::new(w, h, data).expect("valid RGB buffer");
        let var_before = channel_variance(&img, 0);
        apply_gaussian_blur(&mut img, 2.0, 3);
        let var_after = channel_variance(&img, 0);
        assert!(
            var_after < var_before,
            "Gaussian blur should reduce variance: before={var_before}, after={var_after}"
        );
    }
}
