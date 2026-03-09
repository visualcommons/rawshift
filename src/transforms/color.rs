//! Color space transformations.
//!
//! This transform module provides the canonical entry points for color processing:
//! - White Balance application
//! - Color Matrix application (Camera RGB -> Output RGB)
//!
//! It re-exports the optimized primitives from [`crate::processing::color`] and
//! provides the [`ColorSpaceTransform`] struct for bundled pipeline steps.

use crate::core::image::RgbImage;
use crate::error::RawResult;

// Re-export processing primitives as canonical transform entry points.
pub use crate::processing::color::{
    apply_color_matrix, apply_white_balance, apply_white_balance_raw,
};

/// Matrix to convert from CIE XYZ to sRGB (D65).
pub const XYZ_TO_SRGB_D65: [f64; 9] = [
    3.2404542, -1.5371385, -0.4985314, -0.9692660, 1.8760108, 0.0415560, 0.0556434, -0.2040259,
    1.0572252,
];

/// Compute the determinant of a 3x3 row-major matrix.
fn det_3x3(m: &[f64; 9]) -> f64 {
    m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6])
}

/// Invert a 3x3 row-major matrix. Returns `None` if the matrix is singular.
fn invert_3x3(m: &[f64; 9]) -> Option<[f64; 9]> {
    let det = det_3x3(m);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    Some([
        (m[4] * m[8] - m[5] * m[7]) * inv_det,
        (m[2] * m[7] - m[1] * m[8]) * inv_det,
        (m[1] * m[5] - m[2] * m[4]) * inv_det,
        (m[5] * m[6] - m[3] * m[8]) * inv_det,
        (m[0] * m[8] - m[2] * m[6]) * inv_det,
        (m[2] * m[3] - m[0] * m[5]) * inv_det,
        (m[3] * m[7] - m[4] * m[6]) * inv_det,
        (m[1] * m[6] - m[0] * m[7]) * inv_det,
        (m[0] * m[4] - m[1] * m[3]) * inv_det,
    ])
}

/// Multiply two 3x3 row-major matrices: result = a * b.
fn multiply_3x3(a: &[f64; 9], b: &[f64; 9]) -> [f64; 9] {
    [
        a[0] * b[0] + a[1] * b[3] + a[2] * b[6],
        a[0] * b[1] + a[1] * b[4] + a[2] * b[7],
        a[0] * b[2] + a[1] * b[5] + a[2] * b[8],
        a[3] * b[0] + a[4] * b[3] + a[5] * b[6],
        a[3] * b[1] + a[4] * b[4] + a[5] * b[7],
        a[3] * b[2] + a[4] * b[5] + a[5] * b[8],
        a[6] * b[0] + a[7] * b[3] + a[8] * b[6],
        a[6] * b[1] + a[7] * b[4] + a[8] * b[7],
        a[6] * b[2] + a[7] * b[5] + a[8] * b[8],
    ]
}

/// Compute a Camera→sRGB color matrix from a DNG-style XYZ→Camera color matrix.
///
/// The DNG `ColorMatrix` maps CIE XYZ to camera-native RGB. To convert camera
/// data to sRGB we need: `sRGB = XYZ_TO_SRGB * inverse(ColorMatrix)`.
///
/// Returns `None` if the color matrix is singular.
pub fn compute_camera_to_srgb(xyz_to_camera: &[f64; 9]) -> Option<[f32; 9]> {
    let camera_to_xyz = invert_3x3(xyz_to_camera)?;
    let camera_to_srgb = multiply_3x3(&XYZ_TO_SRGB_D65, &camera_to_xyz);
    Some(camera_to_srgb.map(|v| v as f32))
}

/// Pipeline step for color space corrections.
///
/// Bundles white balance and color matrix into a single transform step.
/// Tone mapping / gamma is handled separately by [`crate::transforms::tonemap`].
pub struct ColorSpaceTransform {
    /// White balance multipliers (R, G, B)
    pub wb_coeffs: (f32, f32, f32),
    /// Color matrix (Camera RGB -> Output RGB)
    /// This should be the pre-calculated product of:
    /// XYZ->Output * Camera->XYZ
    pub color_matrix: [f32; 9],
}

impl ColorSpaceTransform {
    /// Create a new color transform with specific settings.
    pub fn new(wb_coeffs: (f32, f32, f32), color_matrix: [f32; 9]) -> Self {
        Self {
            wb_coeffs,
            color_matrix,
        }
    }

    /// Apply the color transformation pipeline to an image in-place.
    ///
    /// 1. Apply White Balance (Linear -> Linear)
    /// 2. Apply Color Matrix (Camera Linear -> Output Linear)
    pub fn apply(&self, image: &mut RgbImage) -> RawResult<()> {
        apply_white_balance(image, self.wb_coeffs);
        apply_color_matrix(image, &self.color_matrix);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-6;

    fn assert_matrix_near(a: &[f64; 9], b: &[f64; 9], eps: f64) {
        for (i, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
            assert!(
                (x - y).abs() < eps,
                "element [{}] differs: {} vs {} (diff {})",
                i,
                x,
                y,
                (x - y).abs()
            );
        }
    }

    #[test]
    fn test_identity_inverse() {
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let inv = invert_3x3(&identity).unwrap();
        assert_matrix_near(&inv, &identity, EPSILON);
    }

    #[test]
    fn test_inverse_roundtrip() {
        // Use a known non-singular matrix (Sony ILCE-7RM5 ColorMatrix2)
        let cm = [
            0.8200, -0.2976, -0.0719, -0.4296, 1.2053, 0.2532, -0.0429, 0.1282, 0.5774,
        ];
        let inv = invert_3x3(&cm).unwrap();
        let product = multiply_3x3(&cm, &inv);
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        assert_matrix_near(&product, &identity, 1e-10);
    }

    #[test]
    fn test_singular_matrix_returns_none() {
        // All-zero row → singular
        let singular = [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 4.0, 5.0, 6.0];
        assert!(invert_3x3(&singular).is_none());
    }

    #[test]
    fn test_multiply_identity() {
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let m = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let result = multiply_3x3(&identity, &m);
        assert_matrix_near(&result, &m, EPSILON);
    }

    #[test]
    fn test_compute_camera_to_srgb_produces_valid_matrix() {
        // Sony ILCE-7RM5 ColorMatrix2 (D65)
        let cm = [
            0.8200, -0.2976, -0.0719, -0.4296, 1.2053, 0.2532, -0.0429, 0.1282, 0.5774,
        ];
        let result = compute_camera_to_srgb(&cm).unwrap();

        // The camera-to-sRGB matrix should have reasonable values:
        // - Diagonal should be positive (each channel maps mostly to itself)
        assert!(result[0] > 0.0, "R→R should be positive: {}", result[0]);
        assert!(result[4] > 0.0, "G→G should be positive: {}", result[4]);
        assert!(result[8] > 0.0, "B→B should be positive: {}", result[8]);

        // - No element should be wildly large (indicates numerical instability)
        for (i, &v) in result.iter().enumerate() {
            assert!(
                v.abs() < 20.0,
                "element [{}] is unreasonably large: {}",
                i,
                v
            );
        }
    }

    #[test]
    fn test_compute_camera_to_srgb_singular_returns_none() {
        let singular = [1.0, 2.0, 3.0, 2.0, 4.0, 6.0, 1.0, 2.0, 3.0];
        assert!(compute_camera_to_srgb(&singular).is_none());
    }

    #[test]
    fn test_all_camera_db_matrices_are_invertible() {
        for cam in crate::data::cameras::all_cameras() {
            if let Some(cm) = &cam.color_matrix_1 {
                assert!(
                    compute_camera_to_srgb(cm).is_some(),
                    "ColorMatrix1 for {} is singular",
                    cam.model
                );
            }
            if let Some(cm) = &cam.color_matrix_2 {
                assert!(
                    compute_camera_to_srgb(cm).is_some(),
                    "ColorMatrix2 for {} is singular",
                    cam.model
                );
            }
        }
    }
}
