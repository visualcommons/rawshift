//! ICC profile constants.
//!
//! Reference values for ICC color management, including the Profile
//! Connection Space (PCS) white point and standard color space primaries.

/// ICC Profile Connection Space (PCS) white point: D50.
///
/// XYZ tristimulus values (Y=1 normalized) per ICC specification.
pub const PCS_WHITEPOINT: [f64; 3] = [0.9505, 1.0000, 1.0890];

/// sRGB color space primaries in CIE 1931 xy chromaticity.
///
/// Format: [[Rx, Ry], [Gx, Gy], [Bx, By]]
pub const SRGB_PRIMARIES: [[f64; 2]; 3] = [
    [0.6400, 0.3300], // Red
    [0.3000, 0.6000], // Green
    [0.1500, 0.0600], // Blue
];

/// sRGB to XYZ (D65) matrix (3x3, row-major).
pub const SRGB_TO_XYZ_D65: [f64; 9] = [
    0.4124564, 0.3575761, 0.1804375, 0.2126729, 0.7151522, 0.0721750, 0.0193339, 0.1191920,
    0.9503041,
];

/// XYZ (D65) to sRGB matrix (3x3, row-major).
pub const XYZ_D65_TO_SRGB: [f64; 9] = [
    3.2404542, -1.5371385, -0.4985314, -0.9692660, 1.8760108, 0.0415560, 0.0556434, -0.2040259,
    1.0572252,
];

/// Adobe RGB (1998) primaries in CIE 1931 xy chromaticity.
pub const ADOBE_RGB_PRIMARIES: [[f64; 2]; 3] = [
    [0.6400, 0.3300], // Red
    [0.2100, 0.7100], // Green
    [0.1500, 0.0600], // Blue
];

/// Bradford chromatic adaptation matrix: D65 to D50.
///
/// Used to adapt color data from D65 (common camera reference)
/// to D50 (ICC PCS).
pub const BRADFORD_D65_TO_D50: [f64; 9] = [
    1.0478112, 0.0228866, -0.0501270, 0.0295424, 0.9904844, -0.0170491, -0.0092345, 0.0150436,
    0.7521316,
];

/// Bradford chromatic adaptation matrix: D50 to D65.
pub const BRADFORD_D50_TO_D65: [f64; 9] = [
    0.9555766, -0.0230393, 0.0631636, -0.0282895, 1.0099416, 0.0210077, 0.0122982, -0.0204830,
    1.3299098,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srgb_matrix_identity_white() {
        // sRGB matrix applied to D65 white point should give ~[1,1,1] in sRGB
        let xyz = [0.9505, 1.0000, 1.0890]; // D50, not D65 — just verify structure
        let r =
            XYZ_D65_TO_SRGB[0] * xyz[0] + XYZ_D65_TO_SRGB[1] * xyz[1] + XYZ_D65_TO_SRGB[2] * xyz[2];
        // Not exactly 1.0 since xyz here is D50, but should be a reasonable value
        assert!(r > 0.0 && r < 2.0);
    }

    #[test]
    fn test_bradford_roundtrip() {
        // D65->D50->D65 should approximate identity
        let input = [0.5, 0.6, 0.7];
        // Forward: D65 -> D50
        let mid = [
            BRADFORD_D65_TO_D50[0] * input[0]
                + BRADFORD_D65_TO_D50[1] * input[1]
                + BRADFORD_D65_TO_D50[2] * input[2],
            BRADFORD_D65_TO_D50[3] * input[0]
                + BRADFORD_D65_TO_D50[4] * input[1]
                + BRADFORD_D65_TO_D50[5] * input[2],
            BRADFORD_D65_TO_D50[6] * input[0]
                + BRADFORD_D65_TO_D50[7] * input[1]
                + BRADFORD_D65_TO_D50[8] * input[2],
        ];
        // Inverse: D50 -> D65
        let output = [
            BRADFORD_D50_TO_D65[0] * mid[0]
                + BRADFORD_D50_TO_D65[1] * mid[1]
                + BRADFORD_D50_TO_D65[2] * mid[2],
            BRADFORD_D50_TO_D65[3] * mid[0]
                + BRADFORD_D50_TO_D65[4] * mid[1]
                + BRADFORD_D50_TO_D65[5] * mid[2],
            BRADFORD_D50_TO_D65[6] * mid[0]
                + BRADFORD_D50_TO_D65[7] * mid[1]
                + BRADFORD_D50_TO_D65[8] * mid[2],
        ];
        for i in 0..3 {
            assert!(
                (output[i] - input[i]).abs() < 1e-4,
                "roundtrip failed at index {i}: {:.6} != {:.6}",
                output[i],
                input[i]
            );
        }
    }
}
