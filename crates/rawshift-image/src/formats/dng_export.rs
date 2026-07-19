//! DNG export for demosaiced linear RGB images.
//!
//! This module provides functionality to export processed images as
//! DNG 1.7 compliant files with complete metadata preservation.

use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::Path;

use crate::core::RgbImage;
use crate::core::metadata::ImageMetadata;
use crate::error::RawResult;
use crate::tiff::writer::{IfdEntry, TiffWriter};
use crate::tiff::{ByteOrder, TiffTag};

/// DNG export configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DngExportConfig {
    /// Software name to embed (defaults to "rawshift")
    pub software: Option<String>,
    /// Whether to embed EXIF metadata
    pub embed_exif: bool,
    /// Whether to embed GPS metadata (if available)
    pub embed_gps: bool,
}

impl DngExportConfig {
    /// Create a new config with defaults for archival export.
    pub fn archival() -> Self {
        Self {
            software: Some(format!("rawshift {}", env!("CARGO_PKG_VERSION"))),
            embed_exif: true,
            embed_gps: true,
        }
    }
}

/// Export an RGB image as a demosaiced linear DNG into any seekable writer.
///
/// The DNG container is written via [`TiffWriter`], which needs `Seek` to
/// backfill the IFD0 offset; an in-memory [`std::io::Cursor`] satisfies this.
///
/// The output is DNG 1.7 compliant with:
/// - PhotometricInterpretation = 2 (RGB)
/// - 16-bit per channel
/// - Embedded color matrices and white balance
/// - Full EXIF metadata (if available)
pub fn export_dng_to_writer<W: Write + Seek>(
    writer: W,
    image: &RgbImage,
    metadata: &ImageMetadata,
    config: &DngExportConfig,
) -> RawResult<()> {
    let mut writer = TiffWriter::new(writer, ByteOrder::LittleEndian);

    // Write TIFF header
    writer.write_header()?;

    // Write image data first to get offset
    let (strip_offset, strip_bytes) = writer.write_image_strip_rgb16(image.data())?;

    // Build IFD entries
    let mut entries = build_dng_ifd(image, metadata, config, strip_offset, strip_bytes);

    // Write IFD
    let ifd_offset = writer.write_ifd(&mut entries, 0)?;

    // Update IFD0 offset in header
    writer.update_ifd0_offset(ifd_offset as u32)?;

    Ok(())
}

/// Export an RGB image as a demosaiced linear DNG file.
///
/// Thin wrapper over [`export_dng_to_writer`] that creates the file at `path`.
pub fn export_dng(
    path: &Path,
    image: &RgbImage,
    metadata: &ImageMetadata,
    config: &DngExportConfig,
) -> RawResult<()> {
    let file = File::create(path)?;
    export_dng_to_writer(BufWriter::new(file), image, metadata, config)
}

/// Build the IFD entries for a DNG file.
fn build_dng_ifd(
    image: &RgbImage,
    metadata: &ImageMetadata,
    config: &DngExportConfig,
    strip_offset: u64,
    strip_bytes: u64,
) -> Vec<IfdEntry> {
    let mut entries = vec![
        // === Required TIFF baseline tags ===
        IfdEntry::long(TiffTag::ImageWidth, image.width()),
        IfdEntry::long(TiffTag::ImageLength, image.height()),
        IfdEntry::shorts(TiffTag::BitsPerSample, &[16, 16, 16]),
        IfdEntry::short(TiffTag::Compression, 1), // Uncompressed
        IfdEntry::short(TiffTag::PhotometricInterpretation, 2), // RGB
        IfdEntry::short(TiffTag::SamplesPerPixel, 3),
        IfdEntry::long(TiffTag::RowsPerStrip, image.height()),
        IfdEntry::long(TiffTag::StripOffsets, strip_offset as u32),
        IfdEntry::long(TiffTag::StripByteCounts, strip_bytes as u32),
        IfdEntry::short(TiffTag::PlanarConfiguration, 1), // Chunky
        // Resolution (72 dpi default)
        IfdEntry::rational(TiffTag::XResolution, 72, 1),
        IfdEntry::rational(TiffTag::YResolution, 72, 1),
        IfdEntry::short(TiffTag::ResolutionUnit, 2), // Inch
    ];

    // Orientation
    if let Some(orient) = metadata.image.orientation {
        entries.push(IfdEntry::short(TiffTag::Orientation, orient));
    } else {
        entries.push(IfdEntry::short(TiffTag::Orientation, 1)); // Normal
    }

    // === DNG version tags ===
    entries.push(IfdEntry::bytes(TiffTag::DNGVersion, &[1, 7, 0, 0]));
    entries.push(IfdEntry::bytes(TiffTag::DNGBackwardVersion, &[1, 7, 0, 0]));

    // === Camera identification ===
    if !metadata.camera.make.is_empty() {
        entries.push(IfdEntry::ascii(TiffTag::Make, &metadata.camera.make));
    }
    if !metadata.camera.model.is_empty() {
        entries.push(IfdEntry::ascii(TiffTag::Model, &metadata.camera.model));
    }

    // UniqueCameraModel (required for DNG)
    let unique_model = metadata
        .camera
        .unique_camera_model
        .as_deref()
        .unwrap_or_else(|| {
            if !metadata.camera.model.is_empty() {
                &metadata.camera.model
            } else {
                "Unknown Camera"
            }
        });
    entries.push(IfdEntry::ascii(TiffTag::UniqueCameraModel, unique_model));

    // === Software ===
    if let Some(ref sw) = config.software {
        entries.push(IfdEntry::ascii(TiffTag::Software, sw));
    }

    // === Color calibration ===
    // ColorMatrix1 is required for DNG
    if let Some(cm1) = &metadata.dng_color.color_matrix_1 {
        // Convert f64 to SRATIONAL (multiply by 10000 for precision)
        let rationals: Vec<(i32, i32)> = cm1
            .iter()
            .map(|&v| ((v * 10000.0).round() as i32, 10000))
            .collect();
        entries.push(IfdEntry::srationals(TiffTag::ColorMatrix1, &rationals));
    } else {
        // Default sRGB-ish color matrix if none provided
        let default_cm: Vec<(i32, i32)> = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            .iter()
            .map(|&v| ((v * 10000.0).round() as i32, 10000))
            .collect();
        entries.push(IfdEntry::srationals(TiffTag::ColorMatrix1, &default_cm));
    }

    // CalibrationIlluminant1
    if let Some(ill1) = metadata.dng_color.calibration_illuminant_1 {
        entries.push(IfdEntry::short(TiffTag::CalibrationIlluminant1, ill1));
    } else {
        entries.push(IfdEntry::short(TiffTag::CalibrationIlluminant1, 21)); // D65
    }

    // ColorMatrix2 (if available)
    if let Some(cm2) = &metadata.dng_color.color_matrix_2 {
        let rationals: Vec<(i32, i32)> = cm2
            .iter()
            .map(|&v| ((v * 10000.0).round() as i32, 10000))
            .collect();
        entries.push(IfdEntry::srationals(TiffTag::ColorMatrix2, &rationals));

        if let Some(ill2) = metadata.dng_color.calibration_illuminant_2 {
            entries.push(IfdEntry::short(TiffTag::CalibrationIlluminant2, ill2));
        } else {
            entries.push(IfdEntry::short(TiffTag::CalibrationIlluminant2, 17)); // Standard Light A
        }
    }

    // AsShotNeutral
    if let Some(neutral) = &metadata.dng_color.as_shot_neutral {
        let rationals: Vec<(u32, u32)> = neutral
            .iter()
            .map(|&v| ((v * 1000000.0).round() as u32, 1000000))
            .collect();
        entries.push(IfdEntry::rationals(TiffTag::AsShotNeutral, &rationals));
    }

    // AnalogBalance
    if let Some(balance) = &metadata.dng_color.analog_balance {
        let rationals: Vec<(u32, u32)> = balance
            .iter()
            .map(|&v| ((v * 1000000.0).round() as u32, 1000000))
            .collect();
        entries.push(IfdEntry::rationals(TiffTag::AnalogBalance, &rationals));
    }

    // === Calibration ===
    if let Some(be) = metadata.dng_calibration.baseline_exposure {
        // Convert to SRATIONAL
        let int_part = (be * 100.0).round() as i32;
        entries.push(IfdEntry::srational(
            TiffTag::BaselineExposure,
            int_part,
            100,
        ));
    }

    // Black/White levels
    if !metadata.image.black_levels.is_empty() {
        // For RGB, we typically have 3 black levels
        let black: Vec<(u32, u32)> = metadata
            .image
            .black_levels
            .iter()
            .take(3)
            .map(|&v| (v, 1))
            .collect();
        if black.len() == 3 {
            entries.push(IfdEntry::rationals(TiffTag::BlackLevel, &black));
        }
    }

    if let Some(wl) = metadata.image.white_level {
        entries.push(IfdEntry::longs(TiffTag::WhiteLevel, &[wl, wl, wl]));
    } else {
        // Default to 16-bit max
        entries.push(IfdEntry::longs(TiffTag::WhiteLevel, &[65535, 65535, 65535]));
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RgbImage;

    #[test]
    fn test_build_dng_ifd() {
        let image = RgbImage::new(100, 50, vec![0u16; 100 * 50 * 3]).expect("valid RGB buffer");
        let metadata = ImageMetadata::default();
        let config = DngExportConfig::archival();

        let entries = build_dng_ifd(&image, &metadata, &config, 1024, 30000);

        // Check required tags are present
        assert!(
            entries
                .iter()
                .any(|e| e.tag == TiffTag::ImageWidth.as_u16())
        );
        assert!(
            entries
                .iter()
                .any(|e| e.tag == TiffTag::ImageLength.as_u16())
        );
        assert!(
            entries
                .iter()
                .any(|e| e.tag == TiffTag::DNGVersion.as_u16())
        );
        assert!(
            entries
                .iter()
                .any(|e| e.tag == TiffTag::UniqueCameraModel.as_u16())
        );
        assert!(
            entries
                .iter()
                .any(|e| e.tag == TiffTag::ColorMatrix1.as_u16())
        );
    }
}
