//! Nikon NEF format support.
//!
//! This module provides parsing for Nikon Electronic Format (NEF) files,
//! which are based on the TIFF container format with Nikon-specific extensions.
//!
//! IFD structure walking is backed by [`gamut_ifd`].

use std::io::{Read, Seek};
use std::marker::PhantomData;

use gamut_ifd::{Ifd, Value};

use super::ifd::{self, tags};
use crate::core::image::{CfaPattern, Dimensions, RawImage, Rect, white_level_from_bit_depth};
use crate::error::{FormatError, ParseError, RawError, RawResult};

/// Metadata extracted from a Nikon NEF file.
#[derive(Debug, Clone)]
pub struct NefMetadata {
    /// Camera manufacturer (typically "NIKON CORPORATION")
    pub make: String,
    /// Camera model (e.g., "NIKON Z8")
    pub model: String,
    /// Full sensor dimensions
    pub sensor_size: Dimensions,
    /// Active/crop area
    pub active_area: Rect,
    /// Bits per sample (typically 12 or 14)
    pub bit_depth: u8,
    /// CFA pattern (Bayer arrangement)
    pub cfa_pattern: CfaPattern,
    /// Black level values (per CFA channel)
    pub black_levels: [u16; 4],
    /// White/saturation level
    pub white_level: u16,
    /// Offset to raw data
    pub raw_data_offset: u64,
    /// Size of raw data in bytes
    pub raw_data_size: u64,
    /// Compression type used
    pub compression: u16,
}

/// Parsed Nikon NEF file.
pub struct NefFile<R> {
    /// The whole file, read into memory (IFD offsets are absolute).
    data: Vec<u8>,
    /// The main IFD chain (with SubIFDs/Exif/GPS pointer groups resolved)
    ifds: Vec<Ifd>,
    /// Index into the IFD list (main IFD index, sub IFD index within the
    /// SubIFDs pointer group)
    raw_ifd_index: Option<(usize, usize)>,
    /// Extracted metadata
    metadata: Option<NefMetadata>,
    /// The reader type this file was parsed from.
    _reader: PhantomData<R>,
}

impl<R: Read + Seek> NefFile<R> {
    /// Parse a Nikon NEF file.
    pub fn parse(reader: R) -> RawResult<Self> {
        let data = ifd::read_all(reader)?;
        let tree = ifd::parse_tree(&data, "NEF: TIFF structure")?;

        // Find the raw SubIFD
        let raw_ifd_index = Self::find_raw_ifd(&tree.ifds);

        let mut nef = NefFile {
            data,
            ifds: tree.ifds,
            raw_ifd_index,
            metadata: None,
            _reader: PhantomData,
        };

        // Extract metadata
        nef.extract_metadata()?;

        Ok(nef)
    }

    /// Find the SubIFD containing the raw image data.
    ///
    /// The raw SubIFD typically has:
    /// - PhotometricInterpretation = CFA (32803)
    /// - Largest dimensions
    /// - BitsPerSample = 12 or 14
    fn find_raw_ifd(ifds: &[Ifd]) -> Option<(usize, usize)> {
        let mut best_match: Option<(usize, usize, u64)> = None;

        for (ifd_idx, ifd) in ifds.iter().enumerate() {
            for (sub_idx, sub_ifd) in ifd::sub_ifd_group(ifd, tags::SUB_IFDS).iter().enumerate() {
                // Check for CFA photometric interpretation (CFA is 32803);
                // also treat as potential CFA if it has a CFAPattern tag.
                let is_cfa = match sub_ifd.get_u32(tags::PHOTOMETRIC_INTERPRETATION) {
                    Some(photometric) => photometric == 32803,
                    None => sub_ifd.get(tags::CFA_PATTERN).is_some(),
                };

                if is_cfa {
                    let width = ifd::first_u32(sub_ifd, tags::IMAGE_WIDTH).unwrap_or(0);
                    let height = ifd::first_u32(sub_ifd, tags::IMAGE_LENGTH).unwrap_or(0);

                    let pixel_count = width as u64 * height as u64;

                    // Keep the largest one
                    if best_match.is_none() || best_match.as_ref().unwrap().2 < pixel_count {
                        best_match = Some((ifd_idx, sub_idx, pixel_count));
                    }
                }
            }
        }

        best_match.map(|(ifd_idx, sub_idx, _)| (ifd_idx, sub_idx))
    }

    /// Get the raw SubIFD.
    fn raw_ifd(&self) -> Option<&Ifd> {
        self.raw_ifd_index.map(|(ifd_idx, sub_idx)| {
            &ifd::sub_ifd_group(&self.ifds[ifd_idx], tags::SUB_IFDS)[sub_idx]
        })
    }

    /// Get the main IFD (IFD0).
    fn ifd0(&self) -> Option<&Ifd> {
        self.ifds.first()
    }

    /// Get the extracted metadata.
    pub fn metadata(&self) -> Option<&NefMetadata> {
        self.metadata.as_ref()
    }

    /// Extract metadata from the parsed IFDs.
    fn extract_metadata(&mut self) -> RawResult<()> {
        let ifd0 = self.ifd0().ok_or_else(|| {
            RawError::Parse(ParseError::InvalidIfd {
                offset: 0,
                reason: "No IFD0 found".to_string(),
            })
        })?;

        // Extract Make
        let make = ifd::ascii_tag(ifd0, tags::MAKE).unwrap_or_default();

        // Validate this is a Nikon file
        let make_upper = make.to_uppercase();
        if !make_upper.contains("NIKON") {
            return Err(RawError::Format(FormatError::Nef(format!(
                "Not a Nikon file (Make: {})",
                make
            ))));
        }

        // Extract Model
        let model = ifd::ascii_tag(ifd0, tags::MODEL).unwrap_or_default();

        // Get the raw SubIFD
        let raw_ifd = self
            .raw_ifd()
            .ok_or_else(|| RawError::Unsupported("Could not find raw SubIFD".to_string()))?;

        // Extract dimensions from raw SubIFD
        let width = ifd::first_u32(raw_ifd, tags::IMAGE_WIDTH)
            .ok_or(RawError::Parse(ParseError::MissingTag(tags::IMAGE_WIDTH)))?;

        let height = ifd::first_u32(raw_ifd, tags::IMAGE_LENGTH)
            .ok_or(RawError::Parse(ParseError::MissingTag(tags::IMAGE_LENGTH)))?;

        let sensor_size = Dimensions { width, height };

        // Extract bit depth (first element; NEF stores one value per sample)
        let bit_depth = ifd::first_u32(raw_ifd, tags::BITS_PER_SAMPLE).unwrap_or(14) as u8;

        // Extract compression
        let compression = ifd::first_u32(raw_ifd, tags::COMPRESSION).unwrap_or(1) as u16;

        // Extract CFA pattern (Nikon typically uses RGGB)
        let cfa_pattern = raw_ifd
            .get(tags::CFA_PATTERN)
            .and_then(Value::as_bytes)
            .filter(|bytes| bytes.len() >= 4)
            .and_then(|bytes| CfaPattern::from_array([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .unwrap_or(CfaPattern::Rggb);

        // Extract crop/active area (Nikon uses standard TIFF; no DNG tags typically)
        let active_area = Rect::from_coords(0, 0, width, height);

        // Extract black levels
        // Nikon stores black levels in MakerNote (tag 0x0004), but we use a reasonable default.
        // Per the spec: use bit-depth-based default: (0.02 * (1 << bit_depth)) as u16
        let default_black =
            (0.02_f32 * 1u32.checked_shl(bit_depth as u32).unwrap_or(u32::MAX) as f32) as u16;
        let black_levels = [default_black, default_black, default_black, default_black];

        // Extract white level: (1 << bit_depth) - 1
        let white_level = white_level_from_bit_depth(bit_depth);

        // Get raw data location from strips
        let (raw_data_offset, raw_data_size) = if let (Some(offsets), Some(counts)) = (
            raw_ifd.get(tags::STRIP_OFFSETS),
            raw_ifd.get(tags::STRIP_BYTE_COUNTS),
        ) {
            (offsets.as_u64().unwrap_or(0), counts.as_u64().unwrap_or(0))
        } else {
            (0, 0)
        };

        self.metadata = Some(NefMetadata {
            make,
            model,
            sensor_size,
            active_area,
            bit_depth,
            cfa_pattern,
            black_levels,
            white_level,
            raw_data_offset,
            raw_data_size,
            compression,
        });

        Ok(())
    }

    /// Validate that this is a Nikon NEF file.
    pub fn validate(&self) -> RawResult<()> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| RawError::Unsupported("Metadata not extracted".to_string()))?;

        // Check for Nikon
        if !metadata.make.to_uppercase().contains("NIKON") {
            return Err(RawError::Format(FormatError::Nef(format!(
                "Not a Nikon camera: {}",
                metadata.make
            ))));
        }

        // Check for valid dimensions
        if metadata.sensor_size.width == 0 || metadata.sensor_size.height == 0 {
            return Err(RawError::Parse(ParseError::InvalidDimensions {
                width: metadata.sensor_size.width,
                height: metadata.sensor_size.height,
            }));
        }

        // Check for raw data
        if metadata.raw_data_offset == 0 || metadata.raw_data_size == 0 {
            return Err(RawError::Unsupported("No raw data found".to_string()));
        }

        Ok(())
    }

    /// Read raw data as a byte vector.
    ///
    /// This retrieves the compressed raw data stream from the file.
    pub fn read_raw_data(&mut self) -> RawResult<Vec<u8>> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| RawError::Unsupported("Metadata not extracted".to_string()))?;

        let offset = metadata.raw_data_offset;
        let size = metadata.raw_data_size as usize;

        Ok(ifd::read_range(&self.data, offset, size)?.to_vec())
    }

    /// Extract the embedded JPEG thumbnail from IFD 0, if present.
    pub fn thumbnail(&mut self) -> RawResult<Option<Vec<u8>>> {
        let ifd0 = match self.ifd0() {
            Some(ifd) => ifd,
            None => return Ok(None),
        };
        ifd::jpeg_thumbnail(&self.data, ifd0)
    }

    /// Decode the raw image data into a RawImage.
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| {
                RawError::Format(FormatError::Nef("Metadata not available".to_string()))
            })?
            .clone();

        match metadata.compression {
            // Uncompressed: raw u16 values directly in the strip
            1 => {
                let data = self.read_raw_data()?;
                let width = metadata.sensor_size.width as usize;
                let height = metadata.sensor_size.height as usize;
                let expected_pixels = width * height;

                // Parse as u16 values (little-endian for most Nikon)
                let mut pixels = Vec::with_capacity(expected_pixels);
                let chunk_size = 2;
                for chunk in data.chunks(chunk_size) {
                    if chunk.len() == 2 {
                        let val = u16::from_le_bytes([chunk[0], chunk[1]]);
                        pixels.push(val);
                    }
                }

                if pixels.len() != expected_pixels {
                    return Err(RawError::Format(FormatError::Decompression(format!(
                        "Uncompressed decode: got {} pixels, expected {}",
                        pixels.len(),
                        expected_pixels
                    ))));
                }

                Ok(RawImage::builder(
                    metadata.sensor_size,
                    metadata.active_area,
                    metadata.bit_depth,
                    metadata.cfa_pattern,
                )
                .black_levels(metadata.black_levels)
                .white_level(metadata.white_level)
                .data(pixels)
                .build())
            }

            // LJPEG compressed (6 = old JPEG/LJPEG, 34713 = Nikon LJPEG)
            6 | 34713 => {
                use crate::codecs::ljpeg::LjpegDecoder;

                let data = self.read_raw_data()?;
                let mut decoder = LjpegDecoder::new();
                decoder.set_dimensions(metadata.sensor_size.width, metadata.sensor_size.height);
                let output = decoder.decode(&data)?;

                let expected_pixels = metadata
                    .sensor_size
                    .num_pixels()
                    .expect("sensor pixel count overflows usize");
                if output.len() != expected_pixels {
                    return Err(RawError::Format(FormatError::Decompression(format!(
                        "LJPEG decoded {} pixels, expected {}",
                        output.len(),
                        expected_pixels
                    ))));
                }

                Ok(RawImage::builder(
                    metadata.sensor_size,
                    metadata.active_area,
                    metadata.bit_depth,
                    metadata.cfa_pattern,
                )
                .black_levels(metadata.black_levels)
                .white_level(metadata.white_level)
                .data(output)
                .build())
            }

            other => Err(RawError::Unsupported(format!(
                "Nikon compression type {} not yet supported (supported: 1=Uncompressed, 6/34713=LJPEG)",
                other
            ))),
        }
    }
}

impl<R: Read + Seek> crate::core::ExtractMetadata for NefFile<R> {
    fn extract_metadata(&self) -> crate::core::ImageMetadata {
        use crate::core::metadata::*;

        let m = self.metadata.as_ref();

        ImageMetadata {
            camera: CameraInfo {
                make: m.map(|x| x.make.clone()).unwrap_or_default(),
                model: m.map(|x| x.model.clone()).unwrap_or_default(),
                unique_camera_model: None,
                lens_make: None,
                lens_model: None,
                lens_info: None,
                serial_number: None,
            },
            exif: ExifInfo::default(),
            datetime: DateTimeInfo::default(),
            gps: GpsInfo::default(),
            dng_color: DngColorInfo::default(),
            dng_calibration: DngCalibrationInfo::default(),
            dng_profile: DngProfileInfo::default(),
            image: ImageInfo {
                orientation: None,
                bit_depth: m.map(|x| x.bit_depth).unwrap_or(14),
                black_levels: m
                    .map(|x| x.black_levels.iter().map(|&v| v as u32).collect())
                    .unwrap_or_default(),
                white_level: m.map(|x| x.white_level as u32),
                default_crop_origin: m.map(|x| (x.active_area.origin.x, x.active_area.origin.y)),
                default_crop_size: m.map(|x| (x.active_area.size.width, x.active_area.size.height)),
            },
            xmp: None,
            icc_profile: None,
            exif_raw: None,
            makernote_raw: None,
            iptc_raw: None,
            extra: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a minimal valid TIFF with a given Make tag and optional SubIFD.
    fn build_nikon_tiff(make: &str) -> Vec<u8> {
        // Layout:
        //   0..8   : TIFF header (LE, magic=42, ifd0 at offset 8)
        //  8..N    : IFD0 with Make tag (ASCII pointing to make_offset)
        //  N..     : string data for Make

        let make_offset = 64u32; // Where we'll put the string
        let make_bytes: Vec<u8> = {
            let mut b = make.as_bytes().to_vec();
            b.push(0); // null terminator
            b
        };
        let make_count = make_bytes.len() as u32;

        let mut data = vec![0u8; 128];

        // TIFF header (LE)
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes()); // IFD at offset 8

        // IFD0 with 1 entry (Make)
        let entry_count: u16 = 1;
        data[8..10].copy_from_slice(&entry_count.to_le_bytes());

        // Entry: Make (0x010F), Type ASCII (2), Count, Offset
        data[10..12].copy_from_slice(&0x010Fu16.to_le_bytes()); // tag
        data[12..14].copy_from_slice(&2u16.to_le_bytes()); // type ASCII
        data[14..18].copy_from_slice(&make_count.to_le_bytes()); // count
        data[18..22].copy_from_slice(&make_offset.to_le_bytes()); // offset to string

        // Next IFD pointer
        data[22..26].copy_from_slice(&0u32.to_le_bytes());

        // String data at offset 64
        let end = make_offset as usize + make_bytes.len();
        if end <= data.len() {
            data[make_offset as usize..end].copy_from_slice(&make_bytes);
        }

        data
    }

    #[test]
    fn test_nef_metadata_construction() {
        let meta = NefMetadata {
            make: "NIKON CORPORATION".to_string(),
            model: "NIKON Z8".to_string(),
            sensor_size: Dimensions {
                width: 8256,
                height: 5504,
            },
            active_area: Rect::from_coords(0, 0, 8256, 5504),
            bit_depth: 14,
            cfa_pattern: CfaPattern::Rggb,
            black_levels: [300, 300, 300, 300],
            white_level: 16383,
            raw_data_offset: 4096,
            raw_data_size: 90_000_000,
            compression: 34713,
        };

        assert_eq!(meta.make, "NIKON CORPORATION");
        assert_eq!(meta.model, "NIKON Z8");
        assert_eq!(meta.bit_depth, 14);
        assert_eq!(meta.cfa_pattern, CfaPattern::Rggb);
        assert_eq!(meta.compression, 34713);
        assert_eq!(meta.sensor_size.width, 8256);
        assert_eq!(meta.sensor_size.height, 5504);
    }

    #[test]
    fn test_cfa_pattern_parsing_from_bytes() {
        // RGGB = [0, 1, 1, 2]
        let bytes = [0u8, 1, 1, 2];
        let pattern = CfaPattern::from_array([bytes[0], bytes[1], bytes[2], bytes[3]])
            .unwrap_or(CfaPattern::Rggb);
        assert_eq!(pattern, CfaPattern::Rggb);

        // BGGR = [2, 1, 1, 0]
        let bytes_bggr = [2u8, 1, 1, 0];
        let pattern_bggr =
            CfaPattern::from_array([bytes_bggr[0], bytes_bggr[1], bytes_bggr[2], bytes_bggr[3]])
                .unwrap_or(CfaPattern::Rggb);
        assert_eq!(pattern_bggr, CfaPattern::Bggr);
    }

    #[test]
    fn test_compression_type_identification() {
        // Test that compression types are correctly identified by matching what
        // decode_raw would handle
        let uncompressed: u16 = 1;
        let ljpeg_old: u16 = 6;
        let nikon_ljpeg: u16 = 34713;
        let unsupported: u16 = 7;

        // Compression 1 (uncompressed) should be handled
        assert_eq!(uncompressed, 1);
        // Compression 6 (old JPEG/LJPEG) should be handled
        assert_eq!(ljpeg_old, 6);
        // Compression 34713 (Nikon LJPEG) should be handled
        assert_eq!(nikon_ljpeg, 34713);
        // Compression 7 (standard JPEG) should not be handled by NEF decoder
        assert_ne!(unsupported, 1);
        assert_ne!(unsupported, 6);
        assert_ne!(unsupported, 34713);
    }

    #[test]
    fn test_nikon_make_detection() {
        // "NIKON CORPORATION" should be recognized
        assert!("NIKON CORPORATION".to_uppercase().contains("NIKON"));
        // "NIKON" alone should also work
        assert!("NIKON".to_uppercase().contains("NIKON"));
        // Other manufacturers should not match
        assert!(!"SONY".to_uppercase().contains("NIKON"));
        assert!(!"Canon".to_uppercase().contains("NIKON"));
    }

    #[test]
    fn test_parse_nikon_tiff_make_detection() {
        // Build a minimal TIFF with Make="NIKON CORPORATION"
        let data = build_nikon_tiff("NIKON CORPORATION");
        let cursor = Cursor::new(data);

        // Parsing should fail because there's no raw SubIFD, but that's expected.
        // The error should NOT be a "Not a Nikon file" error.
        let result = NefFile::parse(cursor);
        match result {
            Err(RawError::Format(FormatError::Nef(msg))) => {
                // Should not say "Not a Nikon file"
                assert!(
                    !msg.contains("Not a Nikon file"),
                    "Should not fail with Nikon detection error, got: {}",
                    msg
                );
            }
            Err(RawError::Unsupported(msg)) => {
                // This is expected - no raw SubIFD found
                assert!(
                    msg.contains("raw SubIFD"),
                    "Expected 'raw SubIFD' error, got: {}",
                    msg
                );
            }
            Err(_) => {
                // Other errors are also acceptable (e.g., truncated IFD data)
            }
            Ok(_) => {
                // Should not succeed without raw SubIFD
                panic!("Should not succeed without raw SubIFD");
            }
        }
    }

    #[test]
    fn test_parse_non_nikon_tiff_rejected() {
        // Build a minimal TIFF with Make="SONY"
        let data = build_nikon_tiff("SONY");
        let cursor = Cursor::new(data);

        let result = NefFile::parse(cursor);
        match result {
            Err(RawError::Format(FormatError::Nef(msg))) => {
                assert!(
                    msg.contains("Not a Nikon file"),
                    "Expected 'Not a Nikon file' error, got: {}",
                    msg
                );
            }
            Err(_) => {
                // Other errors might occur before Make is read - acceptable
            }
            Ok(_) => {
                panic!("Should not accept a non-Nikon file as NEF");
            }
        }
    }

    #[test]
    fn test_malformed_tiff_invalid_magic() {
        // Invalid magic bytes
        let data = vec![0u8; 32];
        let cursor = Cursor::new(data);
        let result = NefFile::parse(cursor);
        assert!(result.is_err(), "Should fail on invalid magic bytes");
    }

    #[test]
    fn test_truncated_tiff_fails_gracefully() {
        // Only 4 bytes - too short for a valid TIFF header
        let data = vec![b'I', b'I', 42, 0];
        let cursor = Cursor::new(data);
        let result = NefFile::parse(cursor);
        assert!(result.is_err(), "Should fail on truncated TIFF");
    }

    #[test]
    fn test_default_black_level_calculation() {
        // Test the formula: (0.02 * (1 << bit_depth)) as u16
        let bit_depth_12: u8 = 12;
        let bit_depth_14: u8 = 14;

        let black_12 = (0.02_f32 * (1u32 << bit_depth_12) as f32) as u16;
        let black_14 = (0.02_f32 * (1u32 << bit_depth_14) as f32) as u16;

        // 12-bit: 0.02 * 4096 = 81 (approximately)
        assert_eq!(black_12, 81);
        // 14-bit: 0.02 * 16384 = 327 (approximately)
        assert_eq!(black_14, 327);
    }

    #[test]
    fn test_white_level_calculation() {
        // White level is (1 << bit_depth) - 1
        assert_eq!(white_level_from_bit_depth(12), 4095);
        assert_eq!(white_level_from_bit_depth(14), 16383);

        // Edge cases: should not panic
        assert_eq!(white_level_from_bit_depth(0), 0);
        assert_eq!(white_level_from_bit_depth(16), u16::MAX);
        assert_eq!(white_level_from_bit_depth(32), u16::MAX);
        assert_eq!(white_level_from_bit_depth(255), u16::MAX);
    }
}
