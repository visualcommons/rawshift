//! Color processing primitives.
//!
//! This module provides functions for color correction in the raw processing pipeline:
//! - White balance correction
//! - Color matrix application (camera RGB to sRGB)
//! - Gamma correction
//!
//! All functions operate on 16-bit RGB data in the range [0, 65535].

use crate::core::image::{RawImage, RgbImage};

/// Apply white balance to a raw Bayer CFA image.
///
/// Multiplies each pixel by the coefficient corresponding to its CFA channel
/// (determined by the pixel's position within the 2x2 Bayer pattern).
/// Data is clamped to the u16 range [0, 65535].
///
/// # Arguments
/// * `image` - The raw image to modify in place
/// * `coeffs` - (red_scale, green_scale, blue_scale) multipliers
pub fn apply_white_balance_raw(image: &mut RawImage, coeffs: (f32, f32, f32)) {
    let (r_scale, g_scale, b_scale) = coeffs;
    let pattern = image.cfa_pattern().to_array();

    // Build a 2x2 gain lookup from the CFA pattern.
    // Pattern values: 0=Red, 1=Green, 2=Blue
    let scale_for = |color: u8| -> f32 {
        match color {
            0 => r_scale,
            2 => b_scale,
            _ => g_scale,
        }
    };
    let gains: [f32; 4] = [
        scale_for(pattern[0]),
        scale_for(pattern[1]),
        scale_for(pattern[2]),
        scale_for(pattern[3]),
    ];

    let width = image.width() as usize;
    for (idx, pixel) in image.data.iter_mut().enumerate() {
        let x = idx % width;
        let y = idx / width;
        let gain = gains[(y % 2) * 2 + (x % 2)];
        let val = *pixel as f32 * gain;
        *pixel = clamp_u16(val);
    }
}

/// Apply white balance to an RGB image.
///
/// Multiplies each channel by the corresponding coefficient.
/// Data is clamped to the u16 range [0, 65535].
///
/// # Arguments
/// * `image` - The image to modify in place
/// * `coeffs` - (red_scale, green_scale, blue_scale) multipliers
///
/// # Example
/// ```ignore
/// // Typical daylight white balance for Sony sensors
/// apply_white_balance(&mut image, (2.35, 1.0, 1.65));
/// ```
pub fn apply_white_balance(image: &mut RgbImage, coeffs: (f32, f32, f32)) {
    let (r_scale, g_scale, b_scale) = coeffs;

    // Process pixel triplets
    for chunk in image.data.chunks_exact_mut(3) {
        // Red
        let r = chunk[0] as f32 * r_scale;
        chunk[0] = clamp_u16(r);

        // Green
        let g = chunk[1] as f32 * g_scale;
        chunk[1] = clamp_u16(g);

        // Blue
        let b = chunk[2] as f32 * b_scale;
        chunk[2] = clamp_u16(b);
    }
}

/// Apply a 3x3 color matrix to an RGB image.
///
/// The matrix transforms from camera RGB to output color space (typically sRGB).
/// Matrix is row-major: `[R_row, G_row, B_row]` where each row has 3 elements.
///
/// ```text
/// [ R_out ]   [ m0 m1 m2 ] [ R_in ]
/// [ G_out ] = [ m3 m4 m5 ] [ G_in ]
/// [ B_out ]   [ m6 m7 m8 ] [ B_in ]
/// ```
///
/// # Arguments
/// * `image` - The image to modify in place
/// * `matrix` - 3x3 row-major color transformation matrix
pub fn apply_color_matrix(image: &mut RgbImage, matrix: &[f32; 9]) {
    for chunk in image.data.chunks_exact_mut(3) {
        let r = chunk[0] as f32;
        let g = chunk[1] as f32;
        let b = chunk[2] as f32;

        let r_out = r * matrix[0] + g * matrix[1] + b * matrix[2];
        let g_out = r * matrix[3] + g * matrix[4] + b * matrix[5];
        let b_out = r * matrix[6] + g * matrix[7] + b * matrix[8];

        chunk[0] = clamp_u16(r_out);
        chunk[1] = clamp_u16(g_out);
        chunk[2] = clamp_u16(b_out);
    }
}

/// Pre-computed gamma correction lookup table.
///
/// Caches the gamma curve for efficient repeated application.
/// Use this when applying the same gamma to multiple images.
///
/// # Example
/// ```ignore
/// let lut = GammaLut::new(2.2);
/// lut.apply(&mut image1);
/// lut.apply(&mut image2); // Reuses the same LUT
/// ```
pub struct GammaLut {
    table: Box<[u16; 65536]>,
    gamma: f32,
}

impl GammaLut {
    /// Create a new gamma lookup table.
    ///
    /// # Arguments
    /// * `gamma` - Gamma value (typically 2.2 for sRGB)
    #[must_use]
    pub fn new(gamma: f32) -> Self {
        let mut table = Box::new([0u16; 65536]);
        let inv_gamma = 1.0 / gamma;

        for (i, v) in table.iter_mut().enumerate() {
            let normalized = i as f32 / 65535.0;
            let corrected = normalized.powf(inv_gamma);
            *v = clamp_u16(corrected * 65535.0);
        }

        Self { table, gamma }
    }

    /// Get the gamma value this LUT was created with.
    #[must_use]
    pub fn gamma(&self) -> f32 {
        self.gamma
    }

    /// Apply gamma correction using the cached lookup table.
    pub fn apply(&self, image: &mut RgbImage) {
        for pixel in &mut image.data {
            *pixel = self.table[*pixel as usize];
        }
    }
}

/// Apply gamma correction to an RGB image.
///
/// Applies the formula: `V_out = V_in ^ (1 / gamma)`
/// Input and output are in the range [0, 65535].
///
/// For repeated application of the same gamma, consider using [`GammaLut`]
/// which caches the lookup table.
///
/// # Arguments
/// * `image` - The image to modify in place
/// * `gamma` - Gamma value (typically 2.2 for sRGB)
pub fn apply_gamma(image: &mut RgbImage, gamma: f32) {
    // Fast path: gamma 1.0 is identity
    if (gamma - 1.0).abs() < 0.001 {
        return;
    }

    let lut = GammaLut::new(gamma);
    lut.apply(image);
}

/// Clamp a floating-point value to the u16 range [0, 65535].
///
/// # Arguments
/// * `val` - The value to clamp
///
/// # Returns
/// The value clamped to [0, 65535] and cast to u16.
#[inline(always)]
pub fn clamp_u16(val: f32) -> u16 {
    val.clamp(0.0, 65535.0) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::image::{CfaPattern, Rect, Size};

    fn create_test_raw_image(width: u32, height: u32, pattern: CfaPattern) -> RawImage {
        let size = Size::new(width, height);
        let active = Rect::from_coords(0, 0, width, height);
        RawImage::new(size, active, 14, pattern)
    }

    #[test]
    fn test_white_balance_raw_identity() {
        let mut image = create_test_raw_image(4, 4, CfaPattern::Rggb);
        // Fill with uniform value
        for pixel in &mut image.data {
            *pixel = 1000;
        }
        apply_white_balance_raw(&mut image, (1.0, 1.0, 1.0));

        for &pixel in &image.data {
            assert_eq!(pixel, 1000);
        }
    }

    #[test]
    fn test_white_balance_raw_rggb() {
        let mut image = create_test_raw_image(4, 4, CfaPattern::Rggb);
        for pixel in &mut image.data {
            *pixel = 1000;
        }
        apply_white_balance_raw(&mut image, (2.0, 1.0, 0.5));

        // RGGB: row 0: R G R G, row 1: G B G B, ...
        // (0,0) = R -> 2000
        assert_eq!(image.data[0], 2000);
        // (1,0) = G -> 1000
        assert_eq!(image.data[1], 1000);
        // (2,0) = R -> 2000
        assert_eq!(image.data[2], 2000);
        // (0,1) = G -> 1000
        assert_eq!(image.data[4], 1000);
        // (1,1) = B -> 500
        assert_eq!(image.data[5], 500);
        // (2,1) = G -> 1000
        assert_eq!(image.data[6], 1000);
        // (3,1) = B -> 500
        assert_eq!(image.data[7], 500);
    }

    #[test]
    fn test_white_balance_raw_bggr() {
        let mut image = create_test_raw_image(2, 2, CfaPattern::Bggr);
        for pixel in &mut image.data {
            *pixel = 1000;
        }
        apply_white_balance_raw(&mut image, (3.0, 1.0, 2.0));

        // BGGR: (0,0)=B (1,0)=G (0,1)=G (1,1)=R
        assert_eq!(image.data[0], 2000); // B * 2.0
        assert_eq!(image.data[1], 1000); // G * 1.0
        assert_eq!(image.data[2], 1000); // G * 1.0
        assert_eq!(image.data[3], 3000); // R * 3.0
    }

    #[test]
    fn test_white_balance_raw_grbg() {
        let mut image = create_test_raw_image(2, 2, CfaPattern::Grbg);
        for pixel in &mut image.data {
            *pixel = 1000;
        }
        apply_white_balance_raw(&mut image, (2.0, 1.0, 3.0));

        // GRBG: (0,0)=G (1,0)=R (0,1)=B (1,1)=G
        assert_eq!(image.data[0], 1000); // G
        assert_eq!(image.data[1], 2000); // R
        assert_eq!(image.data[2], 3000); // B
        assert_eq!(image.data[3], 1000); // G
    }

    #[test]
    fn test_white_balance_raw_gbrg() {
        let mut image = create_test_raw_image(2, 2, CfaPattern::Gbrg);
        for pixel in &mut image.data {
            *pixel = 1000;
        }
        apply_white_balance_raw(&mut image, (2.0, 1.0, 3.0));

        // GBRG: (0,0)=G (1,0)=B (0,1)=R (1,1)=G
        assert_eq!(image.data[0], 1000); // G
        assert_eq!(image.data[1], 3000); // B
        assert_eq!(image.data[2], 2000); // R
        assert_eq!(image.data[3], 1000); // G
    }

    #[test]
    fn test_white_balance_raw_clamps() {
        let mut image = create_test_raw_image(2, 2, CfaPattern::Rggb);
        image.data[0] = 60000; // R position
        image.data[1] = 30000; // G position
        image.data[2] = 60000; // R position
        image.data[3] = 30000; // G position
        apply_white_balance_raw(&mut image, (2.0, 2.0, 2.0));

        assert_eq!(image.data[0], 65535); // 60000 * 2 clamped
        assert_eq!(image.data[1], 60000); // 30000 * 2
    }

    fn create_test_image(width: u32, height: u32, r: u16, g: u16, b: u16) -> RgbImage {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..(width * height) {
            data.push(r);
            data.push(g);
            data.push(b);
        }
        RgbImage::new(width, height, data)
    }

    #[test]
    fn test_clamp_u16() {
        assert_eq!(clamp_u16(0.0), 0);
        assert_eq!(clamp_u16(100.5), 100);
        assert_eq!(clamp_u16(65535.0), 65535);
        assert_eq!(clamp_u16(-100.0), 0);
        assert_eq!(clamp_u16(100000.0), 65535);
    }

    #[test]
    fn test_white_balance_identity() {
        let mut image = create_test_image(2, 2, 1000, 2000, 3000);
        apply_white_balance(&mut image, (1.0, 1.0, 1.0));

        // Identity transform should leave values unchanged
        for i in 0..4 {
            assert_eq!(image.data[i * 3], 1000);
            assert_eq!(image.data[i * 3 + 1], 2000);
            assert_eq!(image.data[i * 3 + 2], 3000);
        }
    }

    #[test]
    fn test_white_balance_scaling() {
        let mut image = create_test_image(2, 2, 1000, 2000, 3000);
        apply_white_balance(&mut image, (2.0, 1.0, 0.5));

        for i in 0..4 {
            assert_eq!(image.data[i * 3], 2000); // R * 2.0
            assert_eq!(image.data[i * 3 + 1], 2000); // G * 1.0
            assert_eq!(image.data[i * 3 + 2], 1500); // B * 0.5
        }
    }

    #[test]
    fn test_white_balance_clamps() {
        let mut image = create_test_image(1, 1, 60000, 30000, 1000);
        apply_white_balance(&mut image, (2.0, 2.0, 0.0));

        assert_eq!(image.data[0], 65535); // Clipped to max
        assert_eq!(image.data[1], 60000); // 30000 * 2
        assert_eq!(image.data[2], 0); // Clipped to 0
    }

    #[test]
    fn test_color_matrix_identity() {
        let identity_matrix: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut image = create_test_image(2, 2, 1000, 2000, 3000);
        apply_color_matrix(&mut image, &identity_matrix);

        // Identity matrix should leave values unchanged
        for i in 0..4 {
            assert_eq!(image.data[i * 3], 1000);
            assert_eq!(image.data[i * 3 + 1], 2000);
            assert_eq!(image.data[i * 3 + 2], 3000);
        }
    }

    #[test]
    fn test_color_matrix_swap_channels() {
        // Matrix that swaps R and B
        let swap_matrix: [f32; 9] = [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0];
        let mut image = create_test_image(1, 1, 1000, 2000, 3000);
        apply_color_matrix(&mut image, &swap_matrix);

        assert_eq!(image.data[0], 3000); // R_out = B_in
        assert_eq!(image.data[1], 2000); // G_out = G_in
        assert_eq!(image.data[2], 1000); // B_out = R_in
    }

    #[test]
    fn test_gamma_identity() {
        let mut image = create_test_image(2, 2, 1000, 2000, 3000);
        let original = image.data.clone();
        apply_gamma(&mut image, 1.0);

        // Gamma 1.0 should be identity (fast path)
        assert_eq!(image.data, original);
    }

    #[test]
    fn test_gamma_22() {
        let mut image = create_test_image(1, 1, 0, 32768, 65535);
        apply_gamma(&mut image, 2.2);

        // Black should stay black
        assert_eq!(image.data[0], 0);
        // White should stay white
        assert_eq!(image.data[2], 65535);
        // Mid-tone should be brighter (gamma correction raises values)
        assert!(
            image.data[1] > 32768,
            "Mid-tone {} should be > 32768",
            image.data[1]
        );
    }

    #[test]
    fn test_gamma_lut_new() {
        let lut = GammaLut::new(2.2);
        assert_eq!(lut.gamma(), 2.2);
    }

    #[test]
    fn test_gamma_lut_apply() {
        let lut = GammaLut::new(2.2);
        let mut image = create_test_image(1, 1, 0, 32768, 65535);
        lut.apply(&mut image);

        assert_eq!(image.data[0], 0);
        assert_eq!(image.data[2], 65535);
        assert!(
            image.data[1] > 32768,
            "Mid-tone should be brighter after gamma"
        );
    }

    #[test]
    fn test_gamma_lut_reuse() {
        // Test that reusing a LUT produces consistent results
        let lut = GammaLut::new(2.2);

        let mut image1 = create_test_image(1, 1, 1000, 2000, 3000);
        let mut image2 = create_test_image(1, 1, 1000, 2000, 3000);

        lut.apply(&mut image1);
        lut.apply(&mut image2);

        assert_eq!(image1.data, image2.data);
    }
}
