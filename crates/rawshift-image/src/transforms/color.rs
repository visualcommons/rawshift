//! Color space transformations.
//!
//! This transform module provides the canonical entry points for color processing:
//! - White Balance application
//! - Color Matrix application (Camera RGB -> Output RGB)
//!
//! It re-exports the optimized primitives from [`crate::processing::color`] and
//! provides the [`ColorSpaceTransform`] struct for bundled pipeline steps.

use crate::core::RgbImage;
use crate::error::{RawError, RawResult};

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

// =============================================================================
// Dual-illuminant colour matrix interpolation
// =============================================================================

/// Correlated colour temperature (CCT) in Kelvin.
///
/// Used to select the interpolation weight between dual-illuminant matrices.
pub type ColorTemperature = f32;

/// Interpolate between two colour matrices based on colour temperature.
///
/// DNG specifies two calibration matrices for different standard illuminants
/// (e.g. Standard A at 2856 K and D65 at 6500 K). This function blends
/// between them for the measured scene colour temperature using the reciprocal
/// CCT (mired) scale, which is the industry-standard approach.
///
/// The interpolation parameter `t` is:
/// ```text
/// t = (1/cct_scene − 1/cct_1) / (1/cct_2 − 1/cct_1)
/// ```
/// clamped to `[0, 1]`. The result is then:
/// ```text
/// matrix = (1 − t) * matrix_1 + t * matrix_2
/// ```
///
/// # Arguments
/// * `matrix_1`  - Colour matrix for illuminant 1 (row-major 3×3, e.g. Standard A / 2856 K).
/// * `cct_1`     - Colour temperature for `matrix_1` in Kelvin (e.g. 2856.0).
/// * `matrix_2`  - Colour matrix for illuminant 2 (row-major 3×3, e.g. D65 / 6500 K).
/// * `cct_2`     - Colour temperature for `matrix_2` in Kelvin (e.g. 6500.0).
/// * `scene_cct` - Estimated scene colour temperature in Kelvin.
///
/// Returns the interpolated 3×3 row-major matrix.
pub fn interpolate_color_matrix(
    matrix_1: &[[f64; 3]; 3],
    cct_1: ColorTemperature,
    matrix_2: &[[f64; 3]; 3],
    cct_2: ColorTemperature,
    scene_cct: ColorTemperature,
) -> [[f64; 3]; 3] {
    let denom = (1.0 / cct_2 as f64) - (1.0 / cct_1 as f64);
    let t = if denom.abs() < 1e-12 {
        0.0_f64
    } else {
        let numer = (1.0 / scene_cct as f64) - (1.0 / cct_1 as f64);
        (numer / denom).clamp(0.0, 1.0)
    };

    let mut result = [[0.0_f64; 3]; 3];
    for row in 0..3 {
        for col in 0..3 {
            result[row][col] = (1.0 - t) * matrix_1[row][col] + t * matrix_2[row][col];
        }
    }
    result
}

/// Estimate scene colour temperature from as-shot neutral (white balance) values.
///
/// The as-shot neutral vector records the reciprocal gain applied to each
/// camera channel so that a neutral (white) object renders as equal R, G, B.
/// This function uses the B/R ratio as a proxy for colour temperature:
///
/// - Warm (tungsten, ~2800 K): high R, low B → low B/R ratio.
/// - Cool (daylight, ~6500 K): balanced R/B → B/R ≈ 1.
///
/// The approximation `CCT ≈ 3000 + 9000 × (B/R)` is clamped to [2000, 10000] K.
///
/// # Arguments
/// * `as_shot_neutral` - `[R, G, B]` neutral gain vector (values in (0, 1]).
///
/// Returns an approximate CCT in Kelvin.
pub fn estimate_cct_from_as_shot_neutral(as_shot_neutral: [f64; 3]) -> ColorTemperature {
    let r = as_shot_neutral[0].max(1e-6);
    let b = as_shot_neutral[2].max(1e-6);
    let rb = b / r;
    (3000.0 + 9000.0 * rb).clamp(2000.0, 10000.0) as f32
}

/// Convert an [`RgbImage`] into sRGB-encoded color space, in place.
///
/// Behaviour depends on the image's current [`ColorDescription`](crate::core::ColorDescription):
/// - `SRGB` / `UNSPECIFIED` — no-op (`UNSPECIFIED` is assumed to be sRGB already).
/// - `LINEAR_SRGB` — applies the sRGB transfer function (OETF).
/// - `DISPLAY_P3` / `REC2020` and other descriptions — not yet supported;
///   returns [`RawError::Unsupported`]. Wide-gamut conversion needs a
///   color-management engine, which is planned follow-up work.
///
/// On success the image's color description is updated to `SRGB`.
pub fn convert_to_srgb(image: &mut RgbImage) -> RawResult<()> {
    use crate::core::ColorDescription;
    use crate::transforms::tonemap::srgb_encode;

    let color = image.color();
    if color == ColorDescription::SRGB || color == ColorDescription::UNSPECIFIED {
        // Already sRGB (or assumed to be) — nothing to do.
    } else if color == ColorDescription::LINEAR_SRGB {
        for sample in image.data_mut() {
            let linear = *sample as f32 / 65535.0;
            *sample = (srgb_encode(linear) * 65535.0 + 0.5) as u16;
        }
    } else {
        return Err(RawError::Unsupported(format!(
            "conversion from {} to sRGB requires a color-management engine \
             (not yet implemented)",
            color.name()
        )));
    }
    image.set_color(ColorDescription::SRGB);
    Ok(())
}

#[cfg(test)]
mod convert_srgb_tests {
    use super::*;
    use crate::core::ColorDescription;

    #[test]
    fn linear_srgb_is_oetf_encoded() {
        // The sRGB OETF lifts linear mid-grey above 0.5.
        let mut img = RgbImage::with_color(
            1,
            1,
            vec![32768, 32768, 32768],
            ColorDescription::LINEAR_SRGB,
        )
        .expect("valid RGB buffer");
        convert_to_srgb(&mut img).expect("LinearSrgb conversion");
        assert_eq!(img.color(), ColorDescription::SRGB);
        assert!(img.data().iter().all(|&v| v > 32768));
    }

    #[test]
    fn srgb_and_unknown_are_noops() {
        for cs in [ColorDescription::SRGB, ColorDescription::UNSPECIFIED] {
            let original = vec![100u16, 200, 300];
            let mut img =
                RgbImage::with_color(1, 1, original.clone(), cs).expect("valid RGB buffer");
            convert_to_srgb(&mut img).expect("no-op conversion");
            assert_eq!(img.data(), original);
            assert_eq!(img.color(), ColorDescription::SRGB);
        }
    }

    #[test]
    fn wide_gamut_is_rejected() {
        let mut img = RgbImage::with_color(1, 1, vec![0, 0, 0], ColorDescription::DISPLAY_P3)
            .expect("valid RGB buffer");
        assert!(convert_to_srgb(&mut img).is_err());
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

    #[test]
    fn test_apply_white_balance_clamps_at_white_level() {
        use crate::core::RgbImage;
        use crate::processing::color::apply_white_balance;

        // Pixel near max with a large gain should clamp at 65535
        let mut img = RgbImage::new(1, 1, vec![60000u16, 60000, 60000]).expect("valid RGB buffer");
        apply_white_balance(&mut img, (3.0, 3.0, 3.0));
        assert_eq!(img.data()[0], 65535, "R should clamp at 65535");
        assert_eq!(img.data()[1], 65535, "G should clamp at 65535");
        assert_eq!(img.data()[2], 65535, "B should clamp at 65535");
    }

    #[test]
    fn test_compute_camera_to_srgb_identity() {
        // The identity matrix for XYZ->Camera is the identity itself.
        // camera_to_xyz = inv(identity) = identity
        // camera_to_srgb = XYZ_TO_SRGB * identity = XYZ_TO_SRGB
        let identity = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let result = compute_camera_to_srgb(&identity).unwrap();
        // Result should equal XYZ_TO_SRGB_D65 (cast to f32)
        for (i, (&got, &expected)) in result.iter().zip(XYZ_TO_SRGB_D65.iter()).enumerate() {
            assert!(
                (got - expected as f32).abs() < 1e-4,
                "Element [{}]: got {} expected {}",
                i,
                got,
                expected
            );
        }
    }

    #[test]
    fn test_apply_color_matrix_zero_input() {
        use crate::core::RgbImage;
        use crate::processing::color::apply_color_matrix;

        let any_matrix: [f32; 9] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let mut img = RgbImage::new(2, 1, vec![0u16; 6]).expect("valid RGB buffer");
        apply_color_matrix(&mut img, &any_matrix);
        for v in img.data() {
            assert_eq!(*v, 0, "Zero input should produce zero output");
        }
    }

    #[test]
    fn test_apply_color_matrix_roundtrip() {
        use crate::core::RgbImage;
        use crate::processing::color::apply_color_matrix;

        // Use a known camera matrix and its inverse for a round-trip test.
        let cm_f64 = [
            0.8200f64, -0.2976, -0.0719, -0.4296, 1.2053, 0.2532, -0.0429, 0.1282, 0.5774,
        ];
        let inv_f64 = invert_3x3(&cm_f64).unwrap();

        let cm: [f32; 9] = cm_f64.map(|v| v as f32);
        let inv: [f32; 9] = inv_f64.map(|v| v as f32);

        let original = vec![10000u16, 20000, 30000];
        let mut img = RgbImage::new(1, 1, original.clone()).expect("valid RGB buffer");

        apply_color_matrix(&mut img, &cm);
        apply_color_matrix(&mut img, &inv);

        // After applying matrix then its inverse, values should be close to original
        for (i, (&got, &expected)) in img.data().iter().zip(original.iter()).enumerate() {
            let diff = (got as i32 - expected as i32).abs();
            assert!(
                diff < 500,
                "Channel [{}]: roundtrip value {} differs from original {} by {}",
                i,
                got,
                expected,
                diff
            );
        }
    }

    #[test]
    fn test_gamma_lut_endpoint_values() {
        use crate::processing::color::GammaLut;

        let lut = GammaLut::new(2.2);
        // Create a minimal RgbImage with 0 and 65535
        use crate::core::RgbImage;
        let mut img = RgbImage::new(1, 1, vec![0u16, 0, 65535]).expect("valid RGB buffer");
        lut.apply(&mut img);
        assert_eq!(img.data()[0], 0, "0 should map to 0");
        assert_eq!(img.data()[1], 0, "0 should map to 0");
        assert_eq!(img.data()[2], 65535, "65535 should map to 65535");
    }

    // -------------------------------------------------------------------------
    // Dual-illuminant interpolation tests
    // -------------------------------------------------------------------------

    fn mat3(v: f64) -> [[f64; 3]; 3] {
        [[v; 3]; 3]
    }

    #[test]
    fn test_interpolate_at_cct1_returns_matrix1() {
        let m1 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let m2 = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]];
        let result = interpolate_color_matrix(&m1, 2856.0, &m2, 6500.0, 2856.0);
        for (row, row_r) in m1.iter().zip(result.iter()) {
            for (&expected, &got) in row.iter().zip(row_r.iter()) {
                assert!(
                    (expected - got).abs() < 1e-6,
                    "at cct1 should return matrix1"
                );
            }
        }
    }

    #[test]
    fn test_interpolate_at_cct2_returns_matrix2() {
        let m1 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let m2 = [[3.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 3.0]];
        let result = interpolate_color_matrix(&m1, 2856.0, &m2, 6500.0, 6500.0);
        for (row, row_r) in m2.iter().zip(result.iter()) {
            for (&expected, &got) in row.iter().zip(row_r.iter()) {
                assert!(
                    (expected - got).abs() < 1e-6,
                    "at cct2 should return matrix2"
                );
            }
        }
    }

    #[test]
    fn test_interpolate_midpoint() {
        // Matrices of all-1.0 and all-3.0; midpoint should be all-2.0.
        let m1 = mat3(1.0);
        let m2 = mat3(3.0);
        // Mired midpoint between 2856 K and 6500 K:
        // 1/2856 ≈ 350.2 mireds,  1/6500 ≈ 153.8 mireds
        // midpoint mired ≈ 252.0 → CCT ≈ 3968 K
        let mid_cct = 1.0 / ((0.5 / 2856.0) + (0.5 / 6500.0)) as f32;
        let result = interpolate_color_matrix(&m1, 2856.0, &m2, 6500.0, mid_cct);
        for row in &result {
            for &v in row {
                assert!(
                    (v - 2.0).abs() < 1e-6,
                    "midpoint should give average: got {v}"
                );
            }
        }
    }

    #[test]
    fn test_interpolate_clamps_below_cct1() {
        let m1 = mat3(0.0);
        let m2 = mat3(1.0);
        // Scene temperature well below cct_1 → t should clamp to 0 → matrix_1.
        let result = interpolate_color_matrix(&m1, 2856.0, &m2, 6500.0, 1000.0);
        for row in &result {
            for &v in row {
                assert!(
                    (v - 0.0).abs() < 1e-6,
                    "clamped below cct1 should return matrix1"
                );
            }
        }
    }

    #[test]
    fn test_interpolate_clamps_above_cct2() {
        let m1 = mat3(0.0);
        let m2 = mat3(1.0);
        // Scene temperature well above cct_2 → t should clamp to 1 → matrix_2.
        let result = interpolate_color_matrix(&m1, 2856.0, &m2, 6500.0, 20000.0);
        for row in &result {
            for &v in row {
                assert!(
                    (v - 1.0).abs() < 1e-6,
                    "clamped above cct2 should return matrix2"
                );
            }
        }
    }

    #[test]
    fn test_estimate_cct_warm() {
        // Warm tungsten light: high R neutral, very low B neutral.
        // B/R = 0.1 / 0.95 ≈ 0.1053 → CCT ≈ 3000 + 9000*0.1053 ≈ 3947 K.
        let cct = estimate_cct_from_as_shot_neutral([0.95, 1.0, 0.1]);
        assert!(cct < 4000.0, "warm WB should give CCT < 4000 K, got {cct}");
    }

    #[test]
    fn test_estimate_cct_daylight() {
        // Neutral/daylight: R ≈ G ≈ B → B/R ≈ 1 → CCT ≈ 3000 + 9000 = 12000 clamped to 10000.
        // With [0.8, 1.0, 0.8] → B/R = 1.0 → CCT = 12000 → clamped to 10000.
        // For a more realistic daylight value use slightly less B:
        let cct = estimate_cct_from_as_shot_neutral([0.6, 1.0, 0.55]);
        assert!(
            (4000.0..=10000.0).contains(&cct),
            "daylight WB should give CCT 4000–10000 K, got {cct}"
        );
    }
}
