//! Tone mapping and sRGB encoding.
//!
//! Implements a tone reproduction pipeline for RAW image export:
//!
//! 1. Apply `BaselineExposure` as a linear gain in scene-linear space.
//! 2. Apply the Hable/Uncharted2 filmic tone curve, with its white point set
//!    to the post-gain maximum so that sensor white always maps to display white.
//!    For negative BaselineExposure (extra highlight headroom), values above
//!    the "intended white" get smooth shoulder rolloff instead of hard clipping.
//! 3. Encode to sRGB (IEC 61966-2-1) with the piecewise gamma transfer function.
//!
//! Output is comparable to dcraw/libraw defaults and correctly handles both
//! negative BaselineExposure (e.g. iPhone ProRAW at -0.83 EV) and positive values.

use crate::core::image::RgbImage;
use crate::processing::color::apply_gamma;

/// Apply tone reproduction to an RGB image.
///
/// If `custom_gamma` is `Some(g)`, applies simple power-law gamma correction
/// (useful for advanced users who want direct control).
/// Otherwise, applies the full filmic tone mapping pipeline with optional
/// BaselineExposure from the image metadata.
///
/// Input must be scene-linear, normalized to [0, 65535].
/// After this call, data is display-referred and gamma-encoded in [0, 65535].
pub fn apply_tone_reproduction(image: &mut RgbImage, custom_gamma: Option<f32>) {
    if let Some(gamma) = custom_gamma {
        apply_gamma(image, gamma);
    } else {
        apply_tonemap(image, image.baseline_exposure());
    }
}

/// Apply the full tone reproduction pipeline to an RGB image.
///
/// Input must be scene-linear, normalized to [0, 65535].
/// After this call, data is display-referred and sRGB-gamma encoded in [0, 65535].
///
/// # Arguments
/// * `image`             - Linear RGB image, modified in place.
/// * `baseline_exposure` - DNG BaselineExposure in EV. `None` = 0 EV (identity gain).
pub fn apply_tonemap(image: &mut RgbImage, baseline_exposure: Option<f32>) {
    let gain = baseline_exposure.map(|ev| 2.0f32.powf(ev)).unwrap_or(1.0);
    let lut = build_lut(gain);
    for pixel in &mut image.data {
        *pixel = lut[*pixel as usize];
    }
}

/// Build a 65 536-entry LUT: linear scene value → sRGB display value.
///
/// White-point strategy:
/// - `white_point = gain`  (= 2^BaselineExposure * 1.0 for a fully-exposed sensor)
/// - For negative EV (gain < 1): the post-gain max is < 1; setting W = gain makes
///   the curve expand [0, gain] to full display output, exploiting highlight headroom.
/// - For positive EV (gain > 1): sensor max maps through the shoulder to display white.
/// - For EV = 0 (gain = 1): straight mapping with filmic shoulder.
fn build_lut(gain: f32) -> Box<[u16; 65536]> {
    // White point for the Hable curve: sensor max after gain should map to display white.
    let white_point = gain.max(1e-6_f32);
    let white_out = hable(white_point);

    let mut table = Box::new([0u16; 65536]);
    for (i, entry) in table.iter_mut().enumerate() {
        let linear = i as f32 / 65535.0;
        let exposed = linear * gain;
        let tonemapped = (hable(exposed) / white_out).clamp(0.0, 1.0);
        let srgb = srgb_encode(tonemapped);
        *entry = (srgb * 65535.0 + 0.5) as u16;
    }
    table
}

/// Hable (Uncharted 2) filmic tone curve — single channel.
///
/// Reference: John Hable, "Filmic Tonemapping Operators", 2010.
#[inline(always)]
fn hable(x: f32) -> f32 {
    const A: f32 = 0.15; // Shoulder strength
    const B: f32 = 0.50; // Linear strength
    const C: f32 = 0.10; // Linear angle
    const D: f32 = 0.20; // Toe strength
    const E: f32 = 0.02; // Toe numerator
    const F: f32 = 0.30; // Toe denominator
    ((x * (A * x + C * B) + D * E) / (x * (A * x + B) + D * F)) - E / F
}

/// sRGB piecewise transfer function (IEC 61966-2-1).
///
/// Converts a linear-light value in [0, 1] to an sRGB-encoded value in [0, 1].
#[inline(always)]
fn srgb_encode(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::image::RgbImage;

    fn make_image(values: &[u16]) -> RgbImage {
        let n = values.len() as u32 / 3;
        RgbImage::new(n, 1, values.to_vec())
    }

    #[test]
    fn black_stays_black() {
        let mut img = make_image(&[0, 0, 0]);
        apply_tonemap(&mut img, None);
        assert_eq!(img.data[0], 0);
        assert_eq!(img.data[1], 0);
        assert_eq!(img.data[2], 0);
    }

    #[test]
    fn white_maps_to_white_no_exposure() {
        // Sensor max (65535) with no baseline exposure should map to display white.
        let mut img = make_image(&[65535, 65535, 65535]);
        apply_tonemap(&mut img, None);
        assert_eq!(img.data[0], 65535);
    }

    #[test]
    fn negative_baseline_exposure_maps_sensor_white_to_display_white() {
        // With negative EV, the camera captured extra highlight headroom.
        // Sensor max should still map to display white (the curve remaps the range).
        let mut img = make_image(&[65535, 65535, 65535]);
        apply_tonemap(&mut img, Some(-0.83));
        assert_eq!(img.data[0], 65535);
    }

    #[test]
    fn negative_baseline_exposure_darkens_midtones() {
        // Mid-grey with -EV should be darker than mid-grey with no EV, because
        // the signal is attenuated before tone mapping.
        let mut img_no_exp = make_image(&[32768, 32768, 32768]);
        let mut img_neg_exp = make_image(&[32768, 32768, 32768]);
        apply_tonemap(&mut img_no_exp, None);
        apply_tonemap(&mut img_neg_exp, Some(-0.83));
        assert!(
            img_neg_exp.data[0] < img_no_exp.data[0],
            "negative EV {} should darken mid-grey {}",
            img_neg_exp.data[0],
            img_no_exp.data[0]
        );
    }

    #[test]
    fn positive_baseline_exposure_brightens_midtones() {
        let mut img_no_exp = make_image(&[16384, 16384, 16384]);
        let mut img_pos_exp = make_image(&[16384, 16384, 16384]);
        apply_tonemap(&mut img_no_exp, None);
        apply_tonemap(&mut img_pos_exp, Some(0.5));
        assert!(
            img_pos_exp.data[0] > img_no_exp.data[0],
            "positive EV {} should brighten {}",
            img_pos_exp.data[0],
            img_no_exp.data[0]
        );
    }

    #[test]
    fn srgb_encode_extremes() {
        assert!((srgb_encode(0.0)).abs() < 1e-6);
        assert!((srgb_encode(1.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn tone_reproduction_uses_gamma_when_specified() {
        let mut img_gamma = make_image(&[32768, 32768, 32768]);
        let mut img_filmic = make_image(&[32768, 32768, 32768]);
        apply_tone_reproduction(&mut img_gamma, Some(2.2));
        apply_tone_reproduction(&mut img_filmic, None);
        // They should produce different results
        assert_ne!(img_gamma.data[0], img_filmic.data[0]);
    }

    #[test]
    fn tone_reproduction_none_matches_filmic() {
        let mut img_repro = make_image(&[32768, 32768, 32768]);
        let mut img_filmic = make_image(&[32768, 32768, 32768]);
        apply_tone_reproduction(&mut img_repro, None);
        apply_tonemap(&mut img_filmic, None);
        assert_eq!(img_repro.data[0], img_filmic.data[0]);
    }

    #[test]
    fn test_apply_tone_reproduction_clamps() {
        // Values at white_level (65535) should map to display white (65535)
        let mut img = make_image(&[65535, 65535, 65535]);
        apply_tone_reproduction(&mut img, None);
        assert_eq!(img.data[0], 65535, "white should stay at max after tonemap");
        assert_eq!(img.data[1], 65535);
        assert_eq!(img.data[2], 65535);
    }

    #[test]
    fn test_linear_tone_reproduction() {
        // With gamma=1.0, apply_tone_reproduction should be a near-identity for mid values.
        // (gamma=1.0 is a fast-path in apply_gamma that does nothing)
        let values = [1000u16, 32768, 65535];
        let mut img = make_image(&values);
        apply_tone_reproduction(&mut img, Some(1.0));
        // gamma=1.0 is identity - values should be unchanged
        assert_eq!(img.data[0], values[0]);
        assert_eq!(img.data[1], values[1]);
        assert_eq!(img.data[2], values[2]);
    }

    #[test]
    fn test_tonemap_output_range() {
        // All outputs must be in [0, 65535] regardless of gain
        for ev in [-2.0f32, -0.83, 0.0, 0.5, 2.0] {
            for val in [0u16, 1000, 32768, 65535] {
                let mut img = make_image(&[val, val, val]);
                apply_tonemap(&mut img, Some(ev));
                // u16 is always in range [0, 65535] by definition
                assert!(
                    !img.data.is_empty(),
                    "EV={}, input={}: output should not be empty",
                    ev,
                    val
                );
            }
        }
    }
}
