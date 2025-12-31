//! RAW format decoders.
//!
//! This module provides format-specific decoders for various RAW image formats.
//! Use `RawFile::open()` as the common entry point for automatic format detection.

pub mod arw;
pub mod dng;

use std::io::{Read, Seek, SeekFrom};

use crate::error::{RawError, RawResult};
use crate::processing::color::{apply_color_matrix, apply_gamma, apply_white_balance};
use crate::processing::ProcessingOptions;
use crate::tiff::{TiffParser, TiffTag};
use std::path::Path;
use zune_core::colorspace::ColorSpace;
use zune_image::image::Image;

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
    Arw(Box<arw::ArwFile<R>>),
    Dng,
    // Add other formats here as they are implemented
    // Jpeg(jpeg::JpegFile<R>),
    // etc.
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
                // For now, return the Dng variant without parsing content
                // TODO: Implement DNG parsing
                Ok(RawFile::Dng)
            }
        }
    }

    /// Export the raw file to an image format based on the file extension.
    ///
    /// This runs the full processing pipeline:
    /// 1. Decode raw data
    /// 2. Apply black level subtraction and normalization
    /// 3. Demosaic
    /// 4. Apply White Balance (if specified)
    /// 5. Apply Color Matrix (if specified)
    /// 6. Apply Gamma Correction (if specified)
    /// 7. Save to disk
    pub fn export<P: AsRef<Path>>(
        &mut self,
        path: P,
        options: &ProcessingOptions,
    ) -> RawResult<()> {
        let mut raw_image = match self {
            RawFile::Arw(arw) => arw.decode_raw()?,
            RawFile::Dng => {
                return Err(RawError::UnsupportedFormat(
                    "DNG export not implemented".to_string(),
                ))
            }
        };

        // 1. Black Level Subtraction
        // TODO: Handle per-channel black levels correctly
        let black_level = raw_image.black_levels[0];
        if black_level > 0 {
            for pixel in &mut raw_image.data {
                *pixel = pixel.saturating_sub(black_level);
            }
        }

        // 2. Scale to 16-bit (Normalize)
        let shift = 16u8.saturating_sub(raw_image.bit_depth);
        if shift > 0 {
            for pixel in &mut raw_image.data {
                *pixel <<= shift;
            }
        }

        // 3. Demosaic
        let demosaic_impl = options.demosaic.implementation();
        let mut rgb_image = demosaic_impl.demosaic(&raw_image);

        // 4. White Balance
        if let Some(coeffs) = options.white_balance {
            apply_white_balance(&mut rgb_image, coeffs);
        }

        // 5. Color Matrix
        if let Some(matrix) = options.color_matrix {
            apply_color_matrix(&mut rgb_image, &matrix);
        }

        // 6. Gamma
        if let Some(gamma) = options.gamma {
            apply_gamma(&mut rgb_image, gamma);
        }

        // 7. Save
        let width = rgb_image.width;
        let height = rgb_image.height;

        // Create zune-image Image
        // width, height, colorspace, depth, data
        let image = Image::from_u16(
            &rgb_image.data,
            width as usize,
            height as usize,
            ColorSpace::RGB,
        );

        image.save(path)?;

        Ok(())
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
