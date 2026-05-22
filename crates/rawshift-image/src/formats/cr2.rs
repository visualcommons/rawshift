//! Canon CR2 format decoder.
//!
//! This module provides parsing for Canon CR2 (Canon Raw version 2) files,
//! which are based on the TIFF container format with LJPEG-compressed raw data.
//!
//! ## Format Structure
//!
//! CR2 is a TIFF-based format with 4 IFDs:
//! - IFD 0: Small JPEG thumbnail + basic metadata (Make, Model)
//! - IFD 1: Larger JPEG preview image
//! - IFD 2: Additional metadata
//! - IFD 3: Main raw data (LJPEG-compressed CFA data)
//!
//! The raw data IFD (index 3) contains:
//! - `Compression = 6` (JPEG/LJPEG)
//! - `StripOffsets` / `StripByteCounts` pointing to the LJPEG data
//! - Canon-specific `CR2Slice` tag (0xC640) for slice reconstruction

use std::io::{Read, Seek};

use crate::core::image::{CfaPattern, RawImage, Rect, Size, white_level_from_bit_depth};
use crate::error::{FormatError, ParseError, RawError, RawResult};
use crate::tiff::{Ifd, TiffParser, TiffTag, TiffValue};

/// Magic marker bytes: byte offset 8-10 in a CR2 file.
/// Bytes 8-9 = "CR", byte 10 = 0x02 (CR2 version).
const CR2_MAGIC_OFFSET: usize = 8;
const CR2_MAGIC: [u8; 3] = [b'C', b'R', 0x02];

/// Compression type value for JPEG (used in IFD 3).
const COMPRESSION_JPEG: u16 = 6;

/// Tag ID for Canon CR2Slice (0xC640). Reserved for future slice reconstruction.
#[allow(dead_code)]
const TAG_CR2_SLICE: u16 = 0xC640;

/// Metadata extracted from a Canon CR2 file.
#[derive(Debug, Clone)]
pub struct Cr2Metadata {
    /// Camera manufacturer (typically "Canon")
    pub make: String,
    /// Camera model (e.g., "Canon EOS 5D Mark III")
    pub model: String,
    /// Full sensor dimensions
    pub sensor_size: Size,
    /// Active/crop area (full sensor size if no ActiveArea tag)
    pub active_area: Rect,
    /// Bits per sample (typically 14)
    pub bit_depth: u8,
    /// CFA pattern (Bayer arrangement)
    pub cfa_pattern: CfaPattern,
    /// Black level values (per CFA channel)
    pub black_levels: [u16; 4],
    /// White/saturation level
    pub white_level: u16,
    /// Offset to raw LJPEG data in the file
    pub raw_data_offset: u64,
    /// Size of raw LJPEG data in bytes
    pub raw_data_size: u64,
}

/// Parsed Canon CR2 file.
pub struct Cr2File<R> {
    parser: TiffParser<R>,
    /// The main IFD chain (IFD 0 through IFD 3)
    ifds: Vec<Ifd>,
    /// Extracted metadata
    metadata: Option<Cr2Metadata>,
}

impl<R> std::fmt::Debug for Cr2File<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cr2File")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek> Cr2File<R> {
    /// Parse a Canon CR2 file.
    ///
    /// This opens the file, validates it as a CR2, walks the IFD chain,
    /// and extracts all available metadata.
    pub fn parse(reader: R) -> RawResult<Self> {
        let mut parser = TiffParser::new(reader)?;

        // Walk the IFD chain (IFD 0, 1, 2, 3)
        let ifds = parser.walk_ifd_chain()?;

        if ifds.is_empty() {
            return Err(RawError::Format(FormatError::Cr2(
                "No IFDs found in file".to_string(),
            )));
        }

        let mut cr2 = Cr2File {
            parser,
            ifds,
            metadata: None,
        };

        cr2.extract_metadata()?;

        Ok(cr2)
    }

    /// Get the main IFD (IFD 0).
    fn ifd0(&self) -> Option<&Ifd> {
        self.ifds.first()
    }

    /// Get the raw data IFD (IFD 3).
    ///
    /// In CR2, IFD 3 contains the LJPEG-compressed raw sensor data.
    /// It is identified by `Compression = 6` (JPEG) and large sensor dimensions.
    fn raw_ifd(&self) -> Option<&Ifd> {
        // IFD 3 is the raw IFD in all known CR2 files
        if self.ifds.len() >= 4 {
            let ifd = &self.ifds[3];
            // Validate that this IFD has JPEG compression and non-trivial dimensions
            let compression = ifd
                .get(TiffTag::Compression)
                .map(|e| e.value_offset as u16)
                .unwrap_or(0);
            let width = ifd
                .get(TiffTag::ImageWidth)
                .map(|e| e.value_offset as u32)
                .unwrap_or(0);
            let height = ifd
                .get(TiffTag::ImageLength)
                .map(|e| e.value_offset as u32)
                .unwrap_or(0);

            if compression == COMPRESSION_JPEG && width > 0 && height > 0 {
                return Some(ifd);
            }
        }

        // Fallback: search all IFDs for one with JPEG compression and large dimensions
        let mut best: Option<(usize, u64)> = None;
        for (idx, ifd) in self.ifds.iter().enumerate() {
            let compression = ifd
                .get(TiffTag::Compression)
                .map(|e| e.value_offset as u16)
                .unwrap_or(0);
            if compression != COMPRESSION_JPEG {
                continue;
            }
            let width = ifd
                .get(TiffTag::ImageWidth)
                .map(|e| e.value_offset as u32)
                .unwrap_or(0);
            let height = ifd
                .get(TiffTag::ImageLength)
                .map(|e| e.value_offset as u32)
                .unwrap_or(0);
            let pixels = width as u64 * height as u64;
            if pixels > 0 && (best.is_none() || best.unwrap().1 < pixels) {
                best = Some((idx, pixels));
            }
        }

        best.map(|(idx, _)| &self.ifds[idx])
    }

    /// Get the extracted metadata.
    pub fn metadata(&self) -> Option<&Cr2Metadata> {
        self.metadata.as_ref()
    }

    /// Extract metadata from the parsed IFDs.
    fn extract_metadata(&mut self) -> RawResult<()> {
        let ifd0 = self.ifd0().cloned().ok_or_else(|| {
            RawError::Parse(ParseError::InvalidIfd {
                offset: 0,
                reason: "No IFD0 found".to_string(),
            })
        })?;

        // Extract Make
        let make = if let Some(entry) = ifd0.get(TiffTag::Make) {
            let value = self.parser.read_value(entry)?;
            value.as_str().unwrap_or("").trim().to_string()
        } else {
            String::new()
        };

        // Validate that this is a Canon file
        if !make.to_uppercase().contains("CANON") {
            return Err(RawError::Format(FormatError::Cr2(format!(
                "Not a Canon file (Make: {})",
                make
            ))));
        }

        // Extract Model
        let model = if let Some(entry) = ifd0.get(TiffTag::Model) {
            let value = self.parser.read_value(entry)?;
            value.as_str().unwrap_or("").trim().to_string()
        } else {
            String::new()
        };

        // Get the raw IFD (IFD 3)
        let raw_ifd = self.raw_ifd().cloned().ok_or_else(|| {
            RawError::Format(FormatError::Cr2(
                "Could not find raw data IFD (IFD 3)".to_string(),
            ))
        })?;

        // Extract dimensions from raw IFD
        let width = raw_ifd
            .get(TiffTag::ImageWidth)
            .map(|e| e.value_offset as u32)
            .ok_or(RawError::Parse(ParseError::TagNotFound(
                TiffTag::ImageWidth,
            )))?;

        let height = raw_ifd
            .get(TiffTag::ImageLength)
            .map(|e| e.value_offset as u32)
            .ok_or(RawError::Parse(ParseError::TagNotFound(
                TiffTag::ImageLength,
            )))?;

        let sensor_size = Size::new(width, height);

        // Extract bit depth
        let bit_depth = if let Some(entry) = raw_ifd.get(TiffTag::BitsPerSample) {
            let value = self.parser.read_value(entry)?;
            value.as_u32().unwrap_or(14) as u8
        } else {
            14 // Canon CR2 default is 14-bit
        };

        // Extract CFA pattern
        let cfa_pattern = if let Some(entry) = raw_ifd.get(TiffTag::CFAPattern) {
            let value = self.parser.read_value(entry)?;
            if let TiffValue::Bytes(bytes) = value {
                if bytes.len() >= 4 {
                    let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
                    CfaPattern::from_array(arr).unwrap_or(CfaPattern::Rggb)
                } else {
                    CfaPattern::Rggb
                }
            } else {
                CfaPattern::Rggb
            }
        } else {
            // Canon cameras typically use RGGB
            CfaPattern::Rggb
        };

        // Use full sensor size as active area (CR2 doesn't typically have an ActiveArea tag)
        let active_area = Rect::from_coords(0, 0, width, height);

        // Extract black level (try DNG-style BlackLevel tag 0xC61A, synthesize if absent)
        let black_levels = if let Some(entry) = raw_ifd.get(TiffTag::BlackLevel) {
            let value = self.parser.read_value(entry)?;
            if let Some(vec) = value.as_u32_vec() {
                if vec.len() >= 4 {
                    [vec[0] as u16, vec[1] as u16, vec[2] as u16, vec[3] as u16]
                } else if vec.len() == 1 {
                    let v = vec[0] as u16;
                    [v, v, v, v]
                } else {
                    [0, 0, 0, 0]
                }
            } else {
                [0, 0, 0, 0]
            }
        } else {
            // No black level found; use 0 as a conservative default
            [0, 0, 0, 0]
        };

        // Synthesize white level from bit depth
        let white_level = white_level_from_bit_depth(bit_depth);

        // Get raw data location from StripOffsets / StripByteCounts
        let (raw_data_offset, raw_data_size) = if let (Some(offset_entry), Some(count_entry)) = (
            raw_ifd.get(TiffTag::StripOffsets),
            raw_ifd.get(TiffTag::StripByteCounts),
        ) {
            let offsets = self.parser.read_value(offset_entry)?;
            let counts = self.parser.read_value(count_entry)?;

            // CR2 uses a single strip for the raw data
            let offset = offsets.as_u64().unwrap_or(0);
            let size = counts.as_u64().unwrap_or(0);
            (offset, size)
        } else {
            (0, 0)
        };

        if raw_data_offset == 0 || raw_data_size == 0 {
            return Err(RawError::Format(FormatError::Cr2(
                "No raw data strip found in IFD 3 (missing StripOffsets/StripByteCounts)"
                    .to_string(),
            )));
        }

        self.metadata = Some(Cr2Metadata {
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
        });

        Ok(())
    }

    /// Extract the embedded JPEG thumbnail from IFD 0, if present.
    pub fn thumbnail(&mut self) -> RawResult<Option<Vec<u8>>> {
        let ifd0 = match self.ifd0() {
            Some(ifd) => ifd,
            None => return Ok(None),
        };
        let offset_entry = match ifd0.get(crate::tiff::TiffTag::JPEGInterchangeFormat) {
            Some(e) => e.clone(),
            None => return Ok(None),
        };
        let length_entry = match ifd0.get(crate::tiff::TiffTag::JPEGInterchangeFormatLength) {
            Some(e) => e.clone(),
            None => return Ok(None),
        };
        let offset = match self.parser.read_value(&offset_entry)? {
            crate::tiff::TiffValue::Longs(v) if !v.is_empty() => v[0] as u64,
            crate::tiff::TiffValue::Shorts(v) if !v.is_empty() => v[0] as u64,
            _ => return Ok(None),
        };
        let length = match self.parser.read_value(&length_entry)? {
            crate::tiff::TiffValue::Longs(v) if !v.is_empty() => v[0] as usize,
            crate::tiff::TiffValue::Shorts(v) if !v.is_empty() => v[0] as usize,
            _ => return Ok(None),
        };
        if length == 0 {
            return Ok(None);
        }
        self.parser.seek_to(offset)?;
        let data = self.parser.read_bytes(length)?;
        Ok(Some(data))
    }

    /// Decode the raw image data into a [`RawImage`].
    ///
    /// Reads the LJPEG-compressed data from `raw_data_offset` and decodes it.
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        let metadata = self.metadata.as_ref().cloned().ok_or_else(|| {
            RawError::Format(FormatError::Cr2("Metadata not extracted".to_string()))
        })?;

        // Read the LJPEG-compressed data
        self.parser.seek_to(metadata.raw_data_offset)?;
        let data = self.parser.read_bytes(metadata.raw_data_size as usize)?;

        // Decode with LJPEG decoder
        use crate::codecs::ljpeg::LjpegDecoder;
        let mut decoder = LjpegDecoder::new();
        decoder.set_dimensions(metadata.sensor_size.width, metadata.sensor_size.height);

        let pixels = decoder.decode(&data)?;

        let expected = metadata.sensor_size.pixel_count() as usize;
        if pixels.len() != expected {
            return Err(RawError::Format(FormatError::Cr2(format!(
                "Decoded {} pixels, expected {} ({}x{})",
                pixels.len(),
                expected,
                metadata.sensor_size.width,
                metadata.sensor_size.height,
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
}

/// Detect whether raw bytes represent a CR2 file.
///
/// CR2 files start with a TIFF header (`II` or `MM` + `0x002A`) and have
/// the bytes `CR` + `0x02` at offset 8.
pub fn is_cr2(data: &[u8]) -> bool {
    if data.len() < 11 {
        return false;
    }

    // TIFF header byte-order marker
    let is_le = data[0] == b'I' && data[1] == b'I' && data[2] == 0x2A && data[3] == 0x00;
    let is_be = data[0] == b'M' && data[1] == b'M' && data[2] == 0x00 && data[3] == 0x2A;

    if !is_le && !is_be {
        return false;
    }

    // CR2 magic at offset 8: "CR" + 0x02
    data[CR2_MAGIC_OFFSET] == CR2_MAGIC[0]
        && data[CR2_MAGIC_OFFSET + 1] == CR2_MAGIC[1]
        && data[CR2_MAGIC_OFFSET + 2] == CR2_MAGIC[2]
}

impl<R: Read + Seek> crate::core::MetadataExtractor for Cr2File<R> {
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

    // -------------------------------------------------------------------------
    // CR2 detection tests
    // -------------------------------------------------------------------------

    fn make_cr2_header(little_endian: bool) -> Vec<u8> {
        let mut data = vec![0u8; 32];
        if little_endian {
            data[0] = b'I';
            data[1] = b'I';
            data[2] = 0x2A;
            data[3] = 0x00;
        } else {
            data[0] = b'M';
            data[1] = b'M';
            data[2] = 0x00;
            data[3] = 0x2A;
        }
        // IFD offset at 4..8 (point past end so parser won't succeed)
        data[4] = 0x08;
        // CR2 magic at offset 8
        data[8] = b'C';
        data[9] = b'R';
        data[10] = 0x02;
        data
    }

    #[test]
    fn test_is_cr2_little_endian() {
        let data = make_cr2_header(true);
        assert!(is_cr2(&data), "LE CR2 header should be detected");
    }

    #[test]
    fn test_is_cr2_big_endian() {
        let data = make_cr2_header(false);
        assert!(is_cr2(&data), "BE CR2 header should be detected");
    }

    #[test]
    fn test_is_cr2_wrong_magic() {
        let mut data = make_cr2_header(true);
        // Change CR2 magic to something else
        data[8] = b'X';
        assert!(!is_cr2(&data), "Non-CR2 should not be detected as CR2");
    }

    #[test]
    fn test_is_cr2_not_tiff() {
        let data = vec![0u8; 32];
        assert!(
            !is_cr2(&data),
            "All-zero bytes should not be detected as CR2"
        );
    }

    #[test]
    fn test_is_cr2_too_short() {
        let data = vec![b'I', b'I', 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, b'C', b'R'];
        assert!(!is_cr2(&data), "10-byte buffer is too short to be CR2");
    }

    #[test]
    fn test_is_cr2_dng_not_cr2() {
        // Valid TIFF header but no CR2 magic
        let mut data = vec![0u8; 32];
        data[0] = b'I';
        data[1] = b'I';
        data[2] = 0x2A;
        data[3] = 0x00;
        data[4] = 0x08;
        // bytes 8-10 are 0, not "CR\x02"
        assert!(!is_cr2(&data), "Generic TIFF (not CR2) should not match");
    }

    // -------------------------------------------------------------------------
    // Metadata struct tests (no real file required)
    // -------------------------------------------------------------------------

    #[test]
    fn test_cr2_metadata_fields() {
        let meta = Cr2Metadata {
            make: "Canon".to_string(),
            model: "Canon EOS 5D Mark III".to_string(),
            sensor_size: Size::new(5760, 3840),
            active_area: Rect::from_coords(0, 0, 5760, 3840),
            bit_depth: 14,
            cfa_pattern: CfaPattern::Rggb,
            black_levels: [2048, 2048, 2048, 2048],
            white_level: 16383,
            raw_data_offset: 1024,
            raw_data_size: 12345678,
        };

        assert_eq!(meta.make, "Canon");
        assert_eq!(meta.model, "Canon EOS 5D Mark III");
        assert_eq!(meta.sensor_size.width, 5760);
        assert_eq!(meta.sensor_size.height, 3840);
        assert_eq!(meta.bit_depth, 14);
        assert_eq!(meta.cfa_pattern, CfaPattern::Rggb);
        assert_eq!(meta.black_levels, [2048, 2048, 2048, 2048]);
        assert_eq!(meta.white_level, 16383);
        assert_eq!(meta.raw_data_offset, 1024);
        assert_eq!(meta.raw_data_size, 12345678);
    }

    #[test]
    fn test_cr2_metadata_white_level_calculation() {
        // 14-bit: max = (1 << 14) - 1 = 16383
        assert_eq!(white_level_from_bit_depth(14), 16383);

        // 12-bit: max = (1 << 12) - 1 = 4095
        assert_eq!(white_level_from_bit_depth(12), 4095);

        // 16-bit: should clamp to u16::MAX
        assert_eq!(white_level_from_bit_depth(16), u16::MAX);
    }

    // -------------------------------------------------------------------------
    // CFA pattern parsing tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_cfa_pattern_rggb() {
        let arr: [u8; 4] = [0, 1, 1, 2]; // R, G, G, B
        assert_eq!(CfaPattern::from_array(arr), Some(CfaPattern::Rggb));
    }

    #[test]
    fn test_cfa_pattern_grbg() {
        let arr: [u8; 4] = [1, 0, 2, 1]; // G, R, B, G
        assert_eq!(CfaPattern::from_array(arr), Some(CfaPattern::Grbg));
    }

    #[test]
    fn test_cfa_pattern_bggr() {
        let arr: [u8; 4] = [2, 1, 1, 0]; // B, G, G, R
        assert_eq!(CfaPattern::from_array(arr), Some(CfaPattern::Bggr));
    }

    #[test]
    fn test_cfa_pattern_unknown_defaults() {
        // An unknown pattern should return None
        let arr: [u8; 4] = [3, 3, 3, 3];
        assert_eq!(CfaPattern::from_array(arr), None);
        // Default fallback used in CR2 parser is Rggb
        let fallback = CfaPattern::from_array(arr).unwrap_or(CfaPattern::Rggb);
        assert_eq!(fallback, CfaPattern::Rggb);
    }

    // -------------------------------------------------------------------------
    // Error on non-Canon TIFF
    // -------------------------------------------------------------------------

    fn make_tiff_with_make(make: &str) -> Vec<u8> {
        // Build a minimal LE TIFF with Make + Model tags pointing to strings in the data section.
        let make_bytes = {
            let mut v = make.as_bytes().to_vec();
            v.push(0); // null terminator
            v
        };
        let make_len = make_bytes.len() as u32;

        // We'll have 2 entries (Make, Model), then no-next-ifd.
        // IFD at offset 8.
        // Entry format: tag(2) type(2) count(4) value_offset(4) = 12 bytes each
        // After the 2-byte entry count (1), entries (24), next-ifd (4) = 30 bytes → data at 8+30 = 38
        let ifd_offset: u32 = 8;
        let data_section_offset: u32 = ifd_offset + 2 + (2 * 12) + 4; // 2 entries

        let make_offset = data_section_offset;
        let model_offset = make_offset + make_len;

        let model = "TestModel\0";
        let model_bytes = model.as_bytes();
        let model_len = model_bytes.len() as u32;

        let mut data = Vec::new();
        // TIFF header (LE)
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&ifd_offset.to_le_bytes());

        // IFD: 2 entries
        data.extend_from_slice(&2u16.to_le_bytes());

        // Entry 1: Make (0x010F), ASCII
        data.extend_from_slice(&0x010Fu16.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes()); // ASCII
        data.extend_from_slice(&make_len.to_le_bytes());
        data.extend_from_slice(&make_offset.to_le_bytes());

        // Entry 2: Model (0x0110), ASCII
        data.extend_from_slice(&0x0110u16.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes()); // ASCII
        data.extend_from_slice(&model_len.to_le_bytes());
        data.extend_from_slice(&model_offset.to_le_bytes());

        // Next IFD = 0
        data.extend_from_slice(&0u32.to_le_bytes());

        // Data section: make string
        data.extend_from_slice(&make_bytes);
        // model string
        data.extend_from_slice(model_bytes);

        data
    }

    #[test]
    fn test_parse_non_canon_returns_cr2_error() {
        let tiff_data = make_tiff_with_make("SONY");
        let cursor = Cursor::new(tiff_data);
        let result = Cr2File::parse(cursor);
        assert!(
            matches!(result, Err(RawError::Format(FormatError::Cr2(_)))),
            "Non-Canon Make should produce Cr2Error"
        );
    }

    #[test]
    fn test_parse_canon_make_no_ifd3_returns_cr2_error() {
        // Canon Make, but only 1 IFD (no IFD 3 with raw data)
        let tiff_data = make_tiff_with_make("Canon");
        let cursor = Cursor::new(tiff_data);
        let result = Cr2File::parse(cursor);
        assert!(
            matches!(result, Err(RawError::Format(FormatError::Cr2(_)))),
            "Canon Make with no raw IFD should produce Cr2Error"
        );
    }
}
