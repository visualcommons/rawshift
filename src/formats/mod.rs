//! RAW format decoders.
//!
//! This module provides format-specific decoders for various RAW image formats.
//! Use `RawFile::open()` as the common entry point for automatic format detection.

pub mod arw;
pub mod dng;
pub mod dng_export;
pub mod export;

pub use dng_export::{export_dng, DngExportConfig};

use std::io::{Read, Seek, SeekFrom};

use crate::error::{RawError, RawResult};
use crate::processing::color::{apply_color_matrix, apply_gamma, apply_white_balance, clamp_u16};
use crate::processing::ProcessingOptions;
use crate::tiff::{TiffParser, TiffTag};
use std::path::Path;
use tracing::instrument;

/// Supported RAW file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawFormat {
    /// Sony ARW format
    Arw,
    /// Adobe DNG format (planned)
    Dng,
}

/// Common entry point for parsing RAW files.
///
/// Wraps the specific format implementation for the detected file type.
pub enum RawFile<R> {
    /// Sony ARW format
    Arw(Box<arw::ArwFile<R>>),
    /// Adobe DNG format
    Dng(Box<dng::DngFile<R>>),
}

impl<R: Read + Seek> RawFile<R> {
    /// Open and parse a RAW file, automatically detecting the format.
    ///
    /// This is the primary entry point for using valid file formats.
    pub fn open(mut reader: R) -> RawResult<Self> {
        let format = Self::detect_format(&mut reader)?;

        match format {
            RawFormat::Arw => {
                let file = arw::ArwFile::parse(reader)?;
                Ok(RawFile::Arw(Box::new(file)))
            }
            RawFormat::Dng => {
                let file = dng::DngFile::parse(reader)?;
                Ok(RawFile::Dng(Box::new(file)))
            }
        }
    }

    /// Get unified metadata from this RAW file.
    ///
    /// This provides format-agnostic access to all available metadata.
    pub fn metadata(&self) -> crate::core::ImageMetadata {
        use crate::core::MetadataExtractor;
        match self {
            RawFile::Arw(arw) => arw.extract_metadata(),
            RawFile::Dng(dng) => dng.extract_metadata(),
        }
    }

    /// Export the raw file to an image format based on the encoded options.
    ///
    /// This runs the full processing pipeline:
    /// 1. Decode raw data
    /// 2. Apply black level subtraction and normalization
    /// 3. Demosaic
    /// 4. Apply White Balance (if specified)
    /// 5. Apply Color Matrix (if specified)
    /// 6. Apply Gamma Correction (if specified)
    /// 7. Save to disk using format-specific encoder
    #[instrument(
        skip(self),
        fields(
            path = %path.as_ref().display(),
            process = ?processing_options,
            encode = ?encode_options
        )
    )]
    pub fn export<P: AsRef<Path>>(
        &mut self,
        path: P,
        processing_options: &ProcessingOptions,
        encode_options: &export::EncodeOptions,
    ) -> RawResult<()> {
        tracing::trace!("Exporting raw file");

        // 1. Obtain the initial RGB image
        // Strategies:
        // A) LinearRaw (already RGB, e.g., iPhone ProRAW) -> Decode -> Scale to 16-bit
        // B) Standard RAW (Bayer CFA) -> Decode -> Subtract Black -> Scale -> Demosaic
        let mut rgb_image = if self.is_linear_raw_dng() {
            tracing::trace!("Using LinearRaw path (already demosaiced)");
            // A) LinearRaw Path
            let RawFile::Dng(dng) = self else {
                unreachable!()
            };

            let metadata = dng.metadata();
            let bit_depth = metadata.map(|m| m.bit_depth).unwrap_or(16);
            let linearization_table = metadata.and_then(|m| m.linearization_table.as_ref());

            // Determine if the data is already scaled to 16-bit based on LinearizationTable
            let is_scaled_by_table = if let Some(table) = linearization_table {
                if !table.is_empty() {
                    // Check the maximum value in the table.
                    // If it exceeds 12-bit range (4095), we assume it targets 16-bit.
                    // (Standard DNG linearization usually targets 16-bit 65535).
                    let max_val = table.iter().max().copied().unwrap_or(0);
                    tracing::trace!("LinearizationTable present. Max value: {}", max_val);
                    max_val > 4095
                } else {
                    false
                }
            } else {
                false
            };

            let mut image = dng.decode_linear_raw()?;

            // Normalize to 16-bit
            let shift = if is_scaled_by_table {
                0
            } else {
                16u8.saturating_sub(bit_depth)
            };

            if shift > 0 {
                tracing::debug!(
                    "Scaling {}-bit linear data to 16-bit (shift: {})",
                    bit_depth,
                    shift
                );
                for pixel in &mut image.data {
                    let val = (*pixel as u32) << shift;
                    *pixel = val.min(65535) as u16;
                }
            }
            image
        } else {
            tracing::trace!("Using standard CFA path (demosaicing needed)");
            // B) Standard RAW Path
            let mut raw_image = match self {
                RawFile::Arw(arw) => arw.decode_raw()?,
                RawFile::Dng(dng) => dng.decode_raw()?,
            };

            // Black Level Subtraction
            // TODO: Handle per-channel black levels correctly
            let black_level = raw_image.black_levels[0];
            if black_level > 0 {
                for pixel in &mut raw_image.data {
                    *pixel = pixel.saturating_sub(black_level);
                }
            }

            // Normalize to 16-bit
            let shift = 16u8.saturating_sub(raw_image.bit_depth);
            if shift > 0 {
                for pixel in &mut raw_image.data {
                    *pixel <<= shift;
                }
            }

            // Demosaic
            let demosaic_impl = processing_options.demosaic.to_demosaic();
            let mut rgb = demosaic_impl.demosaic(&raw_image);

            // Transfer metadata
            rgb.baseline_exposure = raw_image.baseline_exposure;
            rgb.default_crop = raw_image.default_crop;

            rgb
        };

        // 2. Shared Post-Processing Pipeline
        tracing::trace!("Applying post-processing");

        // Apply Baseline Exposure
        if let Some(exposure) = rgb_image.baseline_exposure {
            // Exposure adjustment in EV. Gain = 2^exposure.
            // Negative exposure (e.g. -0.8) means we need to apply gain < 1.0 (darken)?
            // Or usually BaselineExposure is a correction factor to applied:
            // "adjustment ... to match the baseline exposure".
            // Typically means multiplying the linear values by 2^exposure.
            let gain = 2.0f32.powf(exposure);
            tracing::trace!(
                "Applying baseline exposure gain: {:.4} (EV: {:.2})",
                gain,
                exposure
            );
            for pixel in &mut rgb_image.data {
                *pixel = clamp_u16(*pixel as f32 * gain);
            }
        }

        // Apply Crop
        if let Some(crop) = rgb_image.default_crop {
            let x = crop.origin.x as usize;
            let y = crop.origin.y as usize;
            let w = crop.size.width as usize;
            let h = crop.size.height as usize;

            if x + w <= rgb_image.width as usize && y + h <= rgb_image.height as usize {
                tracing::trace!("Cropping to default crop: {}x{} at {},{}", w, h, x, y);
                let mut new_data = Vec::with_capacity(w * h * 3);
                for row in 0..h {
                    let src_base = ((y + row) * rgb_image.width as usize + x) * 3;
                    new_data.extend_from_slice(&rgb_image.data[src_base..src_base + w * 3]);
                }
                rgb_image.width = w as u32;
                rgb_image.height = h as u32;
                rgb_image.data = new_data;
            } else {
                tracing::warn!(
                    "Default crop out of bounds: {:?} vs {}x{}",
                    crop,
                    rgb_image.width,
                    rgb_image.height
                );
            }
        }

        // White Balance
        // If not specified, try to derive from metadata (AsShotNeutral)
        let wb_coeffs = processing_options.white_balance.or_else(|| {
            let meta = self.metadata();
            if let Some(neutral) = meta.dng_color.as_shot_neutral {
                // AsShotNeutral is the neutral color in linear space (e.g. 0.47, 1.0, 0.65)
                // Multipliers are 1/x normalized to Green=1.0 usually, or just 1/x.
                // We'll just use 1/x.
                if neutral[0] > 0.0 && neutral[1] > 0.0 && neutral[2] > 0.0 {
                    tracing::trace!("Using AsShotNeutral from metadata: {:?}", neutral);
                    return Some((
                        1.0 / neutral[0] as f32,
                        1.0 / neutral[1] as f32,
                        1.0 / neutral[2] as f32,
                    ));
                }
            }
            None
        });

        if let Some(coeffs) = wb_coeffs {
            tracing::trace!("Applying white balance: {:?}", coeffs);
            apply_white_balance(&mut rgb_image, coeffs);
        }

        // Color Matrix
        if let Some(matrix) = processing_options.color_matrix {
            tracing::trace!("Applying color matrix");
            apply_color_matrix(&mut rgb_image, &matrix);
        }

        // Gamma Correction
        // Default to sRGB (2.2) if not specified, especially for display formats like PNG
        let gamma = processing_options.gamma.or(Some(2.2)); // TODO: See if this is correct

        if let Some(g) = gamma {
            tracing::trace!("Applying gamma: {}", g);
            apply_gamma(&mut rgb_image, g);
        }

        // 3. Save to Disk
        tracing::info!("Encoding image to disk: {:?}", path.as_ref());

        match encode_options {
            export::EncodeOptions::Png(opts) => {
                use zune_core::colorspace::ColorSpace;
                use zune_core::options::EncoderOptions;
                use zune_png::PngEncoder;

                // Configure options
                let options = EncoderOptions::default()
                    .set_width(rgb_image.width as usize)
                    .set_height(rgb_image.height as usize)
                    .set_colorspace(ColorSpace::RGB)
                    .set_depth(opts.bit_depth);

                // Prepare data (Big Endian for 16-bit PNG)
                let data_bytes = if opts.bit_depth == zune_core::bit_depth::BitDepth::Sixteen {
                    let mut bytes = Vec::with_capacity(rgb_image.data.len() * 2);
                    for &pixel in &rgb_image.data {
                        bytes.extend_from_slice(&pixel.to_be_bytes());
                    }
                    bytes
                } else {
                    // 8-bit downscaling
                    let mut bytes = Vec::with_capacity(rgb_image.data.len());
                    for &pixel in &rgb_image.data {
                        bytes.push((pixel >> 8) as u8);
                    }
                    bytes
                };

                // Encode
                let mut encoder = PngEncoder::new(&data_bytes, options);
                let encoded_data = encoder.encode();

                // Write to file
                let mut file = std::fs::File::create(path.as_ref())?;
                use std::io::Write;
                file.write_all(&encoded_data)?;
            }
            export::EncodeOptions::Dng(config) => {
                export_dng(path.as_ref(), &rgb_image, &self.metadata(), config)?;
            }
            export::EncodeOptions::Jpeg(_) => unimplemented!("JPEG encoding not yet implemented"),
            export::EncodeOptions::Avif(_) => unimplemented!("AVIF encoding not yet implemented"),
            export::EncodeOptions::Heic(_) => unimplemented!("HEIC encoding not yet implemented"),
            export::EncodeOptions::Jxl(_) => unimplemented!("JXL encoding not yet implemented"),
            export::EncodeOptions::WebP(_) => unimplemented!("WebP encoding not yet implemented"),
        }

        Ok(())
    }

    /// Helper to check if the current file is a LinearRaw DNG
    pub fn is_linear_raw_dng(&self) -> bool {
        match self {
            RawFile::Dng(dng) => dng.metadata().map(|m| m.is_linear_raw).unwrap_or(false),
            _ => false,
        }
    }

    /// Detect the format of the provided reader.
    fn detect_format(reader: &mut R) -> RawResult<RawFormat> {
        // Read magic bytes
        let start = reader.stream_position()?;
        let mut header = [0u8; 16];
        reader.read_exact(&mut header)?;
        reader.seek(SeekFrom::Start(start))?;

        // Check for TIFF magic (II or MM at offset 0)
        let is_tiff = (header[0] == b'I' && header[1] == b'I' && header[2] == 42 && header[3] == 0)
            || (header[0] == b'M' && header[1] == b'M' && header[2] == 0 && header[3] == 42);

        if !is_tiff {
            return Err(RawError::UnsupportedFormat(
                "Not a TIFF-based RAW file".to_string(),
            ));
        }

        // Parse as TIFF to inspect Make tag for format detection
        let mut parser = TiffParser::new(reader)?;
        let ifd0 = parser.parse_ifd0()?;

        // Check for DNG version first - if present, it's a DNG regardless of Make
        if ifd0.get(TiffTag::DNGVersion).is_some() {
            return Ok(RawFormat::Dng);
        }

        // Check Make tag to determine specific format
        if let Some(make_entry) = ifd0.get(TiffTag::Make) {
            if let Ok(value) = parser.read_value(make_entry) {
                if let Some(make) = value.as_str() {
                    let make_lower = make.to_lowercase();
                    if make_lower.contains("sony") {
                        return Ok(RawFormat::Arw);
                    }
                    // Add more manufacturers here as we add support
                    // if make_lower.contains("canon") { return Ok(RawFormat::Cr2); }
                    // if make_lower.contains("nikon") { return Ok(RawFormat::Nef); }
                }
            }
        }

        // Default to DNG for unrecognized TIFF-based formats (or return unsupported)
        Err(RawError::UnsupportedFormat(
            "Unrecognized camera manufacturer".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_detect_format_invalid_magic() {
        // This data is valid length but has wrong magic bytes
        // We pad to 16+ bytes to satisfy the header read
        let mut data = vec![0u8; 32];
        data[..14].copy_from_slice(b"not a raw file");
        let mut cursor = Cursor::new(data);
        let result = RawFile::detect_format(&mut cursor);
        assert!(
            matches!(result, Err(RawError::UnsupportedFormat(_))),
            "Should fail with UnsupportedFormat for invalid magic: {:?}",
            result
        );
    }

    #[test]
    fn test_detect_format_tiff_no_make() {
        // Valid TIFF header but no Make tag - should return UnsupportedFormat
        // Pad to ensure enough data for parser
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes()); // IFD at offset 8
        data[8..10].copy_from_slice(&0u16.to_le_bytes()); // 0 entries
        data[10..14].copy_from_slice(&0u32.to_le_bytes()); // no next IFD

        let mut cursor = Cursor::new(data);
        let result = RawFile::detect_format(&mut cursor);
        assert!(
            matches!(result, Err(RawError::UnsupportedFormat(_))),
            "Should fail with UnsupportedFormat for unrecognized camera: {:?}",
            result
        );
    }

    #[test]
    fn test_detect_format_dng() {
        // Mock TIFF with DNGVersion tag
        let mut data = vec![0u8; 64];
        // TIFF Header (LE)
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // IFD at offset 8
        let entry_count = 1u16;
        data[8..10].copy_from_slice(&entry_count.to_le_bytes());

        // Entry 1: DNGVersion (0xC612)
        // Tag (2), Type (1=Byte), Count (4), Value/Offset (1,2,3,4)
        data[10..12].copy_from_slice(&0xC612u16.to_le_bytes());
        data[12..14].copy_from_slice(&1u16.to_le_bytes()); // Type Byte
        data[14..18].copy_from_slice(&4u32.to_le_bytes()); // Count 4
        data[18..22].copy_from_slice(&[1, 1, 0, 0]); // Version 1.1.0.0

        // Next IFD (0)
        data[22..26].copy_from_slice(&0u32.to_le_bytes());

        let mut cursor = Cursor::new(data);
        let result = RawFile::detect_format(&mut cursor);
        assert!(matches!(result, Ok(RawFormat::Dng)));
    }

    #[test]
    fn test_detect_format_sony_dng() {
        // Mock TIFF with BOTH DNGVersion and Make="Sony"
        // Should be detected as DNG, not ARW
        let mut data = vec![0u8; 128];
        // TIFF Header (LE)
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // IFD at offset 8
        let entry_count = 2u16;
        data[8..10].copy_from_slice(&entry_count.to_le_bytes());

        // Entry 1: Make (0x010F), Type ASCII (2), Count 5 ("Sony\0"), Offset to data
        let make_offset = 64u32;
        data[10..12].copy_from_slice(&0x010Fu16.to_le_bytes());
        data[12..14].copy_from_slice(&2u16.to_le_bytes());
        data[14..18].copy_from_slice(&5u32.to_le_bytes());
        data[18..22].copy_from_slice(&make_offset.to_le_bytes());

        // Entry 2: DNGVersion (0xC612)
        // Tag (2), Type (1=Byte), Count (4), Value/Offset (1,2,3,4)
        data[22..24].copy_from_slice(&0xC612u16.to_le_bytes());
        data[24..26].copy_from_slice(&1u16.to_le_bytes());
        data[26..30].copy_from_slice(&4u32.to_le_bytes());
        data[30..34].copy_from_slice(&[1, 1, 0, 0]);

        // Next IFD (0)
        data[34..38].copy_from_slice(&0u32.to_le_bytes());

        // String data at offset 64
        data[64..69].copy_from_slice(b"Sony\0");

        let mut cursor = Cursor::new(data);
        let result = RawFile::detect_format(&mut cursor);
        assert!(matches!(result, Ok(RawFormat::Dng)));
    }
}
