//! DNG export for demosaiced linear RGB images.
//!
//! The DNG container is written by [`gamut_dng::DngEncoder`]: a DNG 1.7
//! compliant file holding the image as an uncompressed 16-bit `LinearRaw`
//! (3-plane) raw sub-IFD, with an RGB preview, the camera colour profile,
//! and an EXIF sub-IFD in IFD 0.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use gamut_dng::values::CalibrationIlluminant;
use gamut_dng::{CameraProfile, DngEncoder, RawLevels};

use crate::core::RgbImage;
use crate::core::metadata::ImageMetadata;
use crate::error::{RawError, RawResult};

/// DNG encode configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DngEncodeConfig {
    /// Software name to embed.
    ///
    /// Currently not representable: gamut-dng's encoder writes its own
    /// `Software` tag and has no override yet (tracked upstream).
    pub software: Option<String>,
    /// Whether to embed EXIF metadata
    pub embed_exif: bool,
    /// Whether to embed GPS metadata (if available).
    ///
    /// Currently not representable: gamut-dng's encoder has no GPS sub-IFD
    /// surface yet (tracked upstream).
    pub embed_gps: bool,
}

impl DngEncodeConfig {
    /// Create a new config with defaults for archival export.
    pub fn archival() -> Self {
        Self {
            software: Some(format!("rawshift {}", env!("CARGO_PKG_VERSION"))),
            embed_exif: true,
            embed_gps: true,
        }
    }
}

/// Export an RGB image as a demosaiced linear DNG into any writer.
///
/// The output is DNG 1.7 compliant with:
/// - PhotometricInterpretation = LinearRaw (3 planes)
/// - 16-bit per channel, uncompressed
/// - Embedded color matrices and white balance
/// - EXIF capture settings (if available and `embed_exif` is set)
pub fn export_dng_to_writer<W: Write>(
    mut writer: W,
    image: &RgbImage,
    metadata: &ImageMetadata,
    config: &DngEncodeConfig,
) -> RawResult<()> {
    let bytes = encode_dng(image, metadata, config)?;
    writer.write_all(&bytes)?;
    Ok(())
}

/// Export an RGB image as a demosaiced linear DNG file.
///
/// Thin wrapper over [`export_dng_to_writer`] that creates the file at `path`.
pub fn export_dng(
    path: &Path,
    image: &RgbImage,
    metadata: &ImageMetadata,
    config: &DngEncodeConfig,
) -> RawResult<()> {
    let file = File::create(path)?;
    export_dng_to_writer(BufWriter::new(file), image, metadata, config)
}

/// Encode `image` + `metadata` to DNG bytes via [`DngEncoder`].
fn encode_dng(
    image: &RgbImage,
    metadata: &ImageMetadata,
    config: &DngEncodeConfig,
) -> RawResult<Vec<u8>> {
    let raw = build_raw_image(image, metadata)?;
    let profile = build_camera_profile(metadata)?;

    let mut encoder = DngEncoder::new();
    if config.embed_exif {
        let exif = build_exif_metadata(metadata);
        encoder = encoder.with_metadata(gamut_dng::DngMetadata {
            exif,
            ..Default::default()
        });
    }

    let mut out = Vec::new();
    encoder
        .encode(&raw, &profile, &mut out)
        .map_err(|e| RawError::gamut("DNG: encode", e))?;
    Ok(out)
}

/// The image as a gamut-dng `LinearRaw` (3-plane, 16-bit) raw image with the
/// metadata's black/white levels.
fn build_raw_image(image: &RgbImage, metadata: &ImageMetadata) -> RawResult<gamut_dng::RawImage> {
    let dims = gamut_dng::Dimensions::new(image.width(), image.height())
        .map_err(|e| RawError::gamut("DNG: image dimensions", e))?;
    let mut raw = gamut_dng::RawImage::new_linear_raw(dims, 16, 3, image.data().to_vec())
        .map_err(|e| RawError::gamut("DNG: raw image", e))?;

    // Black levels: one per plane when the metadata carries at least three
    // (mirroring the legacy writer, which only wrote a 3-value BlackLevel);
    // otherwise the DNG default of zero. White level defaults to 16-bit max.
    let black: Vec<f64> = if metadata.image.black_levels.len() >= 3 {
        metadata.image.black_levels[..3]
            .iter()
            .map(|&v| f64::from(v))
            .collect()
    } else {
        vec![0.0; 3]
    };
    let white = f64::from(metadata.image.white_level.unwrap_or(65535));
    let levels = RawLevels::new(3, (1, 1), black, vec![white; 3])
        .map_err(|e| RawError::gamut("DNG: levels", e))?;
    raw = raw
        .with_levels(levels)
        .map_err(|e| RawError::gamut("DNG: levels", e))?;
    Ok(raw)
}

/// The camera colour profile from the export metadata (with the legacy
/// defaults: identity `ColorMatrix1`, D65, neutral white balance).
fn build_camera_profile(metadata: &ImageMetadata) -> RawResult<CameraProfile> {
    let unique_model = metadata
        .camera
        .unique_camera_model
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if !metadata.camera.model.is_empty() {
                metadata.camera.model.clone()
            } else {
                "Unknown Camera".to_string()
            }
        });

    let color_matrix1 = metadata
        .dng_color
        .color_matrix_1
        .unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    let illuminant1 = metadata
        .dng_color
        .calibration_illuminant_1
        .and_then(CalibrationIlluminant::from_code)
        .unwrap_or(CalibrationIlluminant::D65);
    // AsShotNeutral is mandatory in a gamut-dng profile (components must be
    // positive); fall back to neutral when the metadata has none.
    let neutral = metadata
        .dng_color
        .as_shot_neutral
        .filter(|n| n.iter().all(|&v| v.is_finite() && v > 0.0))
        .unwrap_or([1.0, 1.0, 1.0]);

    let mut profile = CameraProfile::new(unique_model, color_matrix1, illuminant1, neutral)
        .map_err(|e| RawError::gamut("DNG: camera profile", e))?;

    if let Some(cm2) = metadata.dng_color.color_matrix_2 {
        let illuminant2 = metadata
            .dng_color
            .calibration_illuminant_2
            .and_then(CalibrationIlluminant::from_code)
            .unwrap_or(CalibrationIlluminant::StandardLightA);
        profile = profile.with_second_illuminant(cm2, illuminant2);
    }
    if let Some(ab) = metadata.dng_color.analog_balance {
        profile = profile.with_analog_balance(ab);
    }
    if let Some(be) = metadata.dng_calibration.baseline_exposure {
        profile = profile.with_baseline_exposure(be);
    }
    Ok(profile)
}

/// The EXIF capture settings gamut-dng can embed.
fn build_exif_metadata(metadata: &ImageMetadata) -> gamut_dng::ExifMetadata {
    let rational = |r: &crate::core::metadata::URational| (r.numerator, r.denominator);
    gamut_dng::ExifMetadata {
        exposure_time: metadata.exif.exposure_time.as_ref().map(rational),
        f_number: metadata.exif.f_number.as_ref().map(rational),
        iso_speed: metadata.exif.iso.and_then(|v| u16::try_from(v).ok()),
        date_time_original: metadata.datetime.datetime_original.clone(),
        focal_length: metadata.exif.focal_length.as_ref().map(rational),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RgbImage;
    use crate::core::metadata::URational;
    use gamut_dng::{DngDecoder, RawPhotometry};

    fn test_metadata() -> ImageMetadata {
        let mut metadata = ImageMetadata::default();
        metadata.camera.make = "TestMake".to_string();
        metadata.camera.model = "TestModel".to_string();
        metadata.dng_color.color_matrix_1 = Some([0.9, 0.1, 0.0, 0.0, 1.0, 0.0, 0.0, 0.1, 0.8]);
        metadata.dng_color.calibration_illuminant_1 = Some(21); // D65
        metadata.dng_color.as_shot_neutral = Some([0.5, 1.0, 0.6]);
        metadata.image.white_level = Some(65535);
        metadata.exif.iso = Some(200);
        metadata.exif.exposure_time = Some(URational::new(1, 125));
        metadata.datetime.datetime_original = Some("2026:01:01 00:00:00".to_string());
        metadata
    }

    #[test]
    fn test_export_round_trips_pixels_and_profile() {
        let width = 20u32;
        let height = 10u32;
        let data: Vec<u16> = (0..(width * height * 3)).map(|i| (i * 3) as u16).collect();
        let image = RgbImage::new(width, height, data.clone()).expect("valid RGB buffer");
        let metadata = test_metadata();
        let config = DngEncodeConfig::archival();

        let mut buf = Vec::new();
        export_dng_to_writer(&mut buf, &image, &metadata, &config).expect("export");

        let decoded = DngDecoder::new().decode(&buf).expect("decode");
        assert_eq!(decoded.raw.dimensions().width, width);
        assert_eq!(decoded.raw.dimensions().height, height);
        assert_eq!(
            decoded.raw.photometry(),
            &RawPhotometry::LinearRaw { planes: 3 }
        );
        assert_eq!(decoded.raw.samples(), data.as_slice());
        assert_eq!(decoded.profile.unique_camera_model(), "TestModel");
        assert_eq!(decoded.profile.as_shot_neutral(), &[0.5, 1.0, 0.6]);
        assert_eq!(
            decoded.metadata.exif.exposure_time,
            Some((1, 125)),
            "EXIF exposure time should round-trip"
        );
        assert_eq!(decoded.metadata.exif.iso_speed, Some(200));
    }

    #[test]
    fn test_export_defaults_without_color_metadata() {
        let image = RgbImage::new(4, 4, vec![100u16; 4 * 4 * 3]).expect("valid RGB buffer");
        let metadata = ImageMetadata::default();
        let config = DngEncodeConfig::default();

        let mut buf = Vec::new();
        export_dng_to_writer(&mut buf, &image, &metadata, &config).expect("export");

        let decoded = DngDecoder::new().decode(&buf).expect("decode");
        assert_eq!(decoded.profile.unique_camera_model(), "Unknown Camera");
        assert_eq!(decoded.profile.as_shot_neutral(), &[1.0, 1.0, 1.0]);
        // No EXIF requested: nothing embedded.
        assert_eq!(decoded.metadata.exif.iso_speed, None);
    }
}
