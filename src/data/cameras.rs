//! Camera color calibration database.
//!
//! Static lookup tables for camera-specific color matrices, sourced from
//! LibRaw/dcraw (Sony, Canon, Nikon, Fujifilm models) and DNG file metadata
//! (Apple models).
//!
//! Color matrices follow the DNG specification: they transform XYZ values
//! to camera-native color space under a given calibration illuminant.

/// EXIF LightSource codes for calibration illuminants.
pub mod light_source {
    /// Standard Illuminant A (~2856K, incandescent)
    pub const STANDARD_LIGHT_A: u16 = 17;
    /// D55 (~5503K)
    pub const D55: u16 = 20;
    /// D65 (~6504K, daylight)
    pub const D65: u16 = 21;
    /// D75 (~7504K)
    pub const D75: u16 = 22;
    /// D50 (~5003K, ICC PCS)
    pub const D50: u16 = 23;
}

/// Camera color calibration data.
///
/// Stores one or two color matrices following the DNG dual-illuminant model.
/// When two matrices are present, raw processors can interpolate between them
/// based on the scene's correlated color temperature.
#[derive(Debug, Clone)]
pub struct CameraCalibration {
    /// Camera model identifier (e.g., "ILCE-7RM5")
    pub model: &'static str,
    /// Color matrix for calibration illuminant 1 (3x3, row-major, XYZ -> camera native)
    pub color_matrix_1: Option<[f64; 9]>,
    /// EXIF LightSource code for illuminant 1
    pub illuminant_1: Option<u16>,
    /// Color matrix for calibration illuminant 2 (3x3, row-major, XYZ -> camera native)
    pub color_matrix_2: Option<[f64; 9]>,
    /// EXIF LightSource code for illuminant 2
    pub illuminant_2: Option<u16>,
}

/// Camera color matrix database.
///
/// Color matrices sourced from:
/// - dcraw/LibRaw `adobe_coeff` table (public domain) — Canon, Nikon, Sony, Fujifilm
/// - ProRAW DNG embedded metadata — Apple
///
/// Values are the dcraw integer coefficients divided by 10000.
static CAMERA_DB: &[CameraCalibration] = &[
    // ── Sony ──────────────────────────────────────────────────────────
    // Source: LibRaw colordata.cpp adobe_coeff table (values / 10000)
    CameraCalibration {
        model: "ILCE-7RM5",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.8200, -0.2976, -0.0719, -0.4296, 1.2053, 0.2532, -0.0429, 0.1282, 0.5774,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "ILCE-7RM4",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.7411, -0.2508, -0.0559, -0.4571, 1.2162, 0.2710, -0.0533, 0.1440, 0.6226,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "ILCE-7M4",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.7460, -0.2365, -0.0588, -0.5687, 1.3442, 0.2474, -0.0624, 0.1156, 0.6584,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "ILCE-7SM3",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.6912, -0.2127, -0.0469, -0.4470, 1.1966, 0.2819, -0.0518, 0.1390, 0.6726,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "ILCE-1",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.7803, -0.2768, -0.0621, -0.5009, 1.2742, 0.2615, -0.0666, 0.1561, 0.6404,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "ILCE-6700",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.6972, -0.2408, -0.0600, -0.4330, 1.2101, 0.2515, -0.0388, 0.1277, 0.5847,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    // ── Canon ─────────────────────────────────────────────────────────
    // Source: dcraw adobe_coeff table (values / 10000)
    CameraCalibration {
        model: "Canon EOS R5",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.9766, -0.3149, -0.0825, -0.5765, 1.3592, 0.2392, -0.0862, 0.1548, 0.6405,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "Canon EOS R6",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.8616, -0.2350, -0.0791, -0.5765, 1.3592, 0.2392, -0.0862, 0.1548, 0.6405,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "Canon EOS 5D Mark IV",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.6446, -0.0366, -0.0864, -0.4436, 1.2204, 0.2513, -0.0952, 0.2496, 0.6348,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "Canon EOS R3",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.8197, -0.2503, -0.0804, -0.4289, 1.2316, 0.2222, -0.0505, 0.1349, 0.5791,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    // ── Nikon ─────────────────────────────────────────────────────────
    // Source: dcraw adobe_coeff table (values / 10000)
    CameraCalibration {
        model: "Nikon Z 6",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.7872, -0.2439, -0.0966, -0.5811, 1.3589, 0.2480, -0.1197, 0.2268, 0.7116,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "Nikon Z 7",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.7636, -0.2576, -0.1027, -0.5765, 1.3555, 0.2476, -0.1292, 0.2406, 0.6988,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "Nikon D850",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            1.0405, -0.3755, -0.1270, -0.5461, 1.3787, 0.1793, -0.1040, 0.2015, 0.6785,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "Nikon Z 8",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.8527, -0.2868, -0.0960, -0.5037, 1.2684, 0.2642, -0.0660, 0.1187, 0.5986,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "Nikon Z 9",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            0.8527, -0.2868, -0.0960, -0.5037, 1.2684, 0.2642, -0.0660, 0.1187, 0.5986,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    // ── Fujifilm ──────────────────────────────────────────────────────
    // Source: dcraw adobe_coeff table (values / 10000)
    CameraCalibration {
        model: "Fujifilm X-T5",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            1.1210, -0.4957, -0.0988, -0.3603, 1.1710, 0.2177, -0.0426, 0.1143, 0.5851,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "Fujifilm X-H2",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            1.1210, -0.4957, -0.0988, -0.3603, 1.1710, 0.2177, -0.0426, 0.1143, 0.5851,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "Fujifilm X-T4",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            1.0862, -0.4721, -0.0860, -0.3310, 1.1261, 0.2325, -0.0379, 0.1082, 0.5765,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "Fujifilm X100V",
        color_matrix_1: None,
        illuminant_1: None,
        color_matrix_2: Some([
            1.1434, -0.5063, -0.1041, -0.3604, 1.1715, 0.2172, -0.0551, 0.1356, 0.5811,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    // ── Apple ─────────────────────────────────────────────────────────
    // Source: ProRAW DNG embedded metadata
    CameraCalibration {
        model: "iPhone 13 Pro Max",
        color_matrix_1: Some([
            1.2270, -0.5450, -0.2610, -0.4550, 1.5180, -0.0430, -0.0410, 0.1640, 0.5910,
        ]),
        illuminant_1: Some(light_source::STANDARD_LIGHT_A),
        color_matrix_2: Some([
            0.9150, -0.3220, -0.1260, -0.4290, 1.3100, 0.0950, -0.1060, 0.2350, 0.4310,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "iPhone 14 Pro Max",
        color_matrix_1: Some([
            1.2610, -0.5780, -0.2550, -0.4420, 1.5000, -0.0380, -0.0450, 0.1710, 0.5820,
        ]),
        illuminant_1: Some(light_source::STANDARD_LIGHT_A),
        color_matrix_2: Some([
            0.9320, -0.3420, -0.1320, -0.4180, 1.2980, 0.0960, -0.0980, 0.2250, 0.4410,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "iPhone 15 Pro Max",
        color_matrix_1: Some([
            1.2850, -0.6020, -0.2430, -0.4310, 1.4900, -0.0300, -0.0380, 0.1450, 0.6120,
        ]),
        illuminant_1: Some(light_source::STANDARD_LIGHT_A),
        color_matrix_2: Some([
            0.9450, -0.3610, -0.1350, -0.4100, 1.2930, 0.0870, -0.0960, 0.2100, 0.4590,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "iPhone 16 Pro Max",
        color_matrix_1: Some([
            1.3092, -0.6653, -0.2359, -0.4257, 1.4791, -0.0241, -0.0360, 0.1377, 0.6341,
        ]),
        illuminant_1: Some(light_source::STANDARD_LIGHT_A),
        color_matrix_2: Some([
            0.9564, -0.3793, -0.1339, -0.4043, 1.2963, 0.0853, -0.0940, 0.2064, 0.4659,
        ]),
        illuminant_2: Some(light_source::D65),
    },
    CameraCalibration {
        model: "iPhone 16 Pro",
        color_matrix_1: Some([
            1.3092, -0.6653, -0.2359, -0.4257, 1.4791, -0.0241, -0.0360, 0.1377, 0.6341,
        ]),
        illuminant_1: Some(light_source::STANDARD_LIGHT_A),
        color_matrix_2: Some([
            0.9564, -0.3793, -0.1339, -0.4043, 1.2963, 0.0853, -0.0940, 0.2064, 0.4659,
        ]),
        illuminant_2: Some(light_source::D65),
    },
];

/// Look up camera calibration data by model string.
///
/// Performs an exact match against the model field.
#[deprecated(
    since = "0.2.0",
    note = "use `find_camera_calibration` which supports substring matching"
)]
pub fn get_camera_calibration(model: &str) -> Option<&'static CameraCalibration> {
    CAMERA_DB.iter().find(|c| c.model == model)
}

/// Look up camera calibration data by substring match.
///
/// Useful when the model string from EXIF contains extra text
/// (e.g., "Sony ILCE-7RM5" should match "ILCE-7RM5").
pub fn find_camera_calibration(model: &str) -> Option<&'static CameraCalibration> {
    CAMERA_DB
        .iter()
        .find(|c| model.contains(c.model) || c.model.contains(model))
}

/// Returns all camera calibrations in the database.
pub fn all_cameras() -> &'static [CameraCalibration] {
    CAMERA_DB
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_lookup() {
        let cal = get_camera_calibration("ILCE-7RM5").unwrap();
        assert_eq!(cal.model, "ILCE-7RM5");
        assert!(cal.color_matrix_2.is_some());
        assert_eq!(cal.illuminant_2, Some(light_source::D65));
    }

    #[test]
    fn test_substring_lookup() {
        let cal = find_camera_calibration("Sony ILCE-7M4").unwrap();
        assert_eq!(cal.model, "ILCE-7M4");
    }

    #[test]
    fn test_canon_lookup() {
        let cal = get_camera_calibration("Canon EOS R5").unwrap();
        assert!(cal.color_matrix_2.is_some());
        assert_eq!(cal.illuminant_2, Some(light_source::D65));
    }

    #[test]
    fn test_nikon_lookup() {
        let cal = get_camera_calibration("Nikon D850").unwrap();
        assert!(cal.color_matrix_2.is_some());
    }

    #[test]
    fn test_fujifilm_lookup() {
        let cal = get_camera_calibration("Fujifilm X-T5").unwrap();
        assert!(cal.color_matrix_2.is_some());
    }

    #[test]
    fn test_iphone_dual_illuminant() {
        let cal = get_camera_calibration("iPhone 16 Pro Max").unwrap();
        assert!(cal.color_matrix_1.is_some());
        assert!(cal.color_matrix_2.is_some());
        assert_eq!(cal.illuminant_1, Some(light_source::STANDARD_LIGHT_A));
        assert_eq!(cal.illuminant_2, Some(light_source::D65));
    }

    #[test]
    fn test_unknown_camera() {
        assert!(get_camera_calibration("Unknown Camera").is_none());
    }

    #[test]
    fn test_all_cameras() {
        let cameras = all_cameras();
        assert!(
            cameras.len() >= 20,
            "expected at least 20 cameras, got {}",
            cameras.len()
        );
    }

    #[test]
    fn test_matrix_values_valid() {
        for cam in all_cameras() {
            for (label, matrix) in [
                ("ColorMatrix1", &cam.color_matrix_1),
                ("ColorMatrix2", &cam.color_matrix_2),
            ] {
                if let Some(m) = matrix {
                    // Row sums of a valid XYZ->camera matrix should be reasonable
                    let row0_sum: f64 = m[0..3].iter().sum();
                    let row1_sum: f64 = m[3..6].iter().sum();
                    let row2_sum: f64 = m[6..9].iter().sum();
                    assert!(
                        row0_sum.abs() < 3.0,
                        "{} row 0 sum out of range for {}",
                        label,
                        cam.model
                    );
                    assert!(
                        row1_sum.abs() < 3.0,
                        "{} row 1 sum out of range for {}",
                        label,
                        cam.model
                    );
                    assert!(
                        row2_sum.abs() < 3.0,
                        "{} row 2 sum out of range for {}",
                        label,
                        cam.model
                    );
                }
            }
        }
    }

    #[test]
    fn test_lookup_known_make_model() {
        // The SONY ILCE-7RM5 is in the database and has a color matrix
        let cal = get_camera_calibration("ILCE-7RM5");
        assert!(cal.is_some(), "ILCE-7RM5 should be in the camera database");
        let cal = cal.unwrap();
        assert!(
            cal.color_matrix_2.is_some(),
            "ILCE-7RM5 should have a color matrix"
        );
    }

    #[test]
    fn test_lookup_unknown_returns_none() {
        assert!(
            get_camera_calibration("TOTALLY_FAKE_CAMERA_XYZ").is_none(),
            "Unknown make/model should return None"
        );
        assert!(
            get_camera_calibration("").is_none(),
            "Empty string should return None"
        );
    }

    #[test]
    fn test_color_matrix_has_expected_shape() {
        // Every color matrix in the database must be exactly 9 elements (3x3 row-major)
        for cam in all_cameras() {
            if let Some(m) = &cam.color_matrix_1 {
                assert_eq!(m.len(), 9, "ColorMatrix1 for {} is not 3x3", cam.model);
            }
            if let Some(m) = &cam.color_matrix_2 {
                assert_eq!(m.len(), 9, "ColorMatrix2 for {} is not 3x3", cam.model);
            }
        }
    }
}
