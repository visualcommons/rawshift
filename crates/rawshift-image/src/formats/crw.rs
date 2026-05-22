//! Canon CRW (CIFF) format decoder.
//!
//! CRW is Canon's older RAW format (before CR2), using the CIFF (Camera Image
//! File Format) container. This module provides detection, metadata extraction,
//! and a stub pixel decoder for CRW files.
//!
//! ## CIFF Structure
//!
//! ```text
//! Header (26 bytes):
//!   [0..2]   byte order marker: "II" (LE) or "MM" (BE)
//!   [2..4]   type: 0x0001
//!   [4..8]   header size (typically 26)
//!   [6..14]  "HEAPCCDR" signature
//!   [14..18] offset to the CIFF heap
//!   [18..22] size of the CIFF heap
//! ```

use std::io::{Read, Seek};

use crate::core::image::{CfaPattern, RawImage, Rect, Size, white_level_from_bit_depth};
use crate::error::{FormatError, RawError, RawResult};

// ── CIFF signature ────────────────────────────────────────────────────────────

/// Expected signature at bytes 6..14 of a CRW file.
const CIFF_SIGNATURE: &[u8; 8] = b"HEAPCCDR";

/// CIFF type field value.
const CIFF_TYPE: u16 = 0x0001;

// ── Metadata ──────────────────────────────────────────────────────────────────

/// Metadata extracted from a Canon CRW / CIFF file.
#[derive(Debug, Clone)]
pub struct CrwMetadata {
    /// Camera make (always "Canon" for CRW files).
    pub make: String,
    /// Camera model string (e.g. "Canon PowerShot G2").
    pub model: String,
    /// Full sensor dimensions.
    pub sensor_size: Size,
    /// Active / crop area.
    pub active_area: Rect,
    /// Bits per sample (typically 12 for CRW).
    pub bit_depth: u8,
    /// CFA Bayer pattern (Canon cameras are typically RGGB).
    pub cfa_pattern: CfaPattern,
    /// Black-level values per CFA channel.
    pub black_levels: [u16; 4],
    /// White / saturation level.
    pub white_level: u16,
    /// Byte offset of the raw pixel data within the file.
    pub raw_data_offset: u64,
    /// Byte length of the raw pixel data.
    pub raw_data_size: u64,
}

// ── CrwFile ───────────────────────────────────────────────────────────────────

/// Parsed Canon CRW file.
pub struct CrwFile<R> {
    /// Underlying reader retained for future full CIFF heap decoding.
    #[allow(dead_code)]
    reader: R,
    /// Extracted metadata (populated by [`CrwFile::parse`]).
    metadata: Option<CrwMetadata>,
}

impl<R> std::fmt::Debug for CrwFile<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrwFile")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek> CrwFile<R> {
    /// Parse a Canon CRW file.
    ///
    /// Reads enough of the file to validate the CIFF header and extract
    /// available metadata. Full CIFF heap parsing is not yet implemented;
    /// sensible defaults are used where real tag data is absent.
    pub fn parse(mut reader: R) -> RawResult<Self> {
        // Read the CIFF header (first 26 bytes are sufficient).
        let mut header = [0u8; 26];
        reader.read_exact(&mut header).map_err(|e| {
            RawError::Format(FormatError::Crw(format!("failed to read CIFF header: {e}")))
        })?;

        // Validate the magic bytes.
        if !is_crw(&header) {
            return Err(RawError::Format(FormatError::Crw(
                "not a CRW file (CIFF signature mismatch)".to_string(),
            )));
        }

        // Determine endianness.
        let little_endian = header[0] == 0x49; // 'I'

        let read_u16 = |b: &[u8], off: usize| -> u16 {
            if little_endian {
                u16::from_le_bytes([b[off], b[off + 1]])
            } else {
                u16::from_be_bytes([b[off], b[off + 1]])
            }
        };
        let read_u32 = |b: &[u8], off: usize| -> u32 {
            if little_endian {
                u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
            } else {
                u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
            }
        };

        // Validate CIFF type field (bytes 2..4).
        let ciff_type = read_u16(&header, 2);
        if ciff_type != CIFF_TYPE {
            return Err(RawError::Format(FormatError::Crw(format!(
                "unexpected CIFF type field: 0x{ciff_type:04X}"
            ))));
        }

        // Heap offset / size from header bytes 14..18 and 18..22.
        let _heap_offset = read_u32(&header, 14);
        let _heap_size = read_u32(&header, 18);

        // Full CIFF heap parsing is not yet implemented.
        // Provide default metadata: Canon, typical 20 MP 5D-era sensor, 12-bit, RGGB.
        let sensor_size = Size::new(5616, 3744);
        let active_area = Rect::from_coords(0, 0, 5616, 3744);
        let bit_depth: u8 = 12;
        let white_level: u16 = white_level_from_bit_depth(bit_depth);

        let metadata = CrwMetadata {
            make: "Canon".to_string(),
            model: String::new(), // Would be parsed from CIFF tag 0x080a
            sensor_size,
            active_area,
            bit_depth,
            cfa_pattern: CfaPattern::Rggb,
            black_levels: [0u16; 4],
            white_level,
            raw_data_offset: 0,
            raw_data_size: 0,
        };

        Ok(CrwFile {
            reader,
            metadata: Some(metadata),
        })
    }

    /// Return the extracted metadata, if available.
    pub fn metadata(&self) -> Option<&CrwMetadata> {
        self.metadata.as_ref()
    }

    /// Decode the raw pixel data.
    /// Extract the embedded JPEG thumbnail.
    ///
    /// CRW thumbnail extraction from CIFF heap is not yet implemented.
    pub fn thumbnail(&mut self) -> RawResult<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Decode the raw image data.
    ///
    /// Full CRW/CIFF pixel decoding requires a complete CIFF heap parser and a
    /// Canon-specific RAW decompressor, which are not yet implemented. This
    /// method returns [`RawError::UnsupportedOperation`] until that work is
    /// done.
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        Err(RawError::Unsupported(
            "CRW pixel decode is not yet implemented; \
             full CIFF heap parsing and Canon RAW decompression are required"
                .to_string(),
        ))
    }
}

// ── Detection ─────────────────────────────────────────────────────────────────

/// Detect whether raw bytes represent a Canon CRW (CIFF) file.
///
/// A CRW file begins with a two-byte endianness marker (`II` or `MM`),
/// followed by the CIFF type word (`0x0001`), and the string `HEAPCCDR`
/// at bytes 6–13.
pub fn is_crw(data: &[u8]) -> bool {
    if data.len() < 14 {
        return false;
    }

    // Byte-order marker.
    let is_le = data[0] == 0x49 && data[1] == 0x49; // "II"
    let is_be = data[0] == 0x4D && data[1] == 0x4D; // "MM"
    if !is_le && !is_be {
        return false;
    }

    // CIFF type field: 0x0001 in the file's own byte order.
    let ciff_type_ok = if is_le {
        data[2] == 0x01 && data[3] == 0x00
    } else {
        data[2] == 0x00 && data[3] == 0x01
    };
    if !ciff_type_ok {
        return false;
    }

    // "HEAPCCDR" signature at bytes 6..14.
    &data[6..14] == CIFF_SIGNATURE
}

// ── MetadataExtractor impl ────────────────────────────────────────────────────

impl<R: Read + Seek> crate::core::MetadataExtractor for CrwFile<R> {
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
                bit_depth: m.map(|x| x.bit_depth).unwrap_or(12),
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // ── is_crw ────────────────────────────────────────────────────────────

    fn make_crw_magic(little_endian: bool) -> Vec<u8> {
        let mut data = vec![0u8; 26];
        if little_endian {
            data[0] = 0x49; // 'I'
            data[1] = 0x49;
            data[2] = 0x01; // type LE
            data[3] = 0x00;
        } else {
            data[0] = 0x4D; // 'M'
            data[1] = 0x4D;
            data[2] = 0x00; // type BE
            data[3] = 0x01;
        }
        // Header size at [4..8] — 26 bytes, matching endianness
        if little_endian {
            data[4..8].copy_from_slice(&26u32.to_le_bytes());
        } else {
            data[4..8].copy_from_slice(&26u32.to_be_bytes());
        }
        // HEAPCCDR signature at [6..14]
        data[6..14].copy_from_slice(CIFF_SIGNATURE);
        data
    }

    #[test]
    fn test_is_crw_little_endian() {
        let data = make_crw_magic(true);
        assert!(is_crw(&data), "LE CRW magic should be detected");
    }

    #[test]
    fn test_is_crw_big_endian() {
        let data = make_crw_magic(false);
        assert!(is_crw(&data), "BE CRW magic should be detected");
    }

    #[test]
    fn test_is_crw_wrong_signature() {
        let mut data = make_crw_magic(true);
        // Corrupt the HEAPCCDR signature
        data[6] = b'X';
        assert!(!is_crw(&data), "corrupted signature should not be detected");
    }

    #[test]
    fn test_is_crw_wrong_byte_order_marker() {
        let mut data = make_crw_magic(true);
        data[0] = 0x00;
        data[1] = 0x00;
        assert!(
            !is_crw(&data),
            "bad byte-order marker should not be detected"
        );
    }

    #[test]
    fn test_is_crw_too_short() {
        let data = vec![0x49u8, 0x49, 0x01, 0x00, 0x00, 0x00, b'H', b'E'];
        assert!(!is_crw(&data), "8-byte buffer is too short to be CRW");
    }

    #[test]
    fn test_is_crw_wrong_type_field() {
        let mut data = make_crw_magic(true);
        // Set type field to something other than 0x0001
        data[2] = 0x2A; // TIFF magic
        data[3] = 0x00;
        assert!(!is_crw(&data), "wrong type field should not be detected");
    }

    #[test]
    fn test_is_crw_all_zeros() {
        let data = vec![0u8; 26];
        assert!(!is_crw(&data));
    }

    // ── CrwMetadata construction ──────────────────────────────────────────

    #[test]
    fn test_crw_metadata_fields() {
        let meta = CrwMetadata {
            make: "Canon".to_string(),
            model: "Canon PowerShot G2".to_string(),
            sensor_size: Size::new(2272, 1704),
            active_area: Rect::from_coords(0, 0, 2272, 1704),
            bit_depth: 12,
            cfa_pattern: CfaPattern::Rggb,
            black_levels: [0; 4],
            white_level: 4095,
            raw_data_offset: 0,
            raw_data_size: 0,
        };

        assert_eq!(meta.make, "Canon");
        assert_eq!(meta.model, "Canon PowerShot G2");
        assert_eq!(meta.sensor_size.width, 2272);
        assert_eq!(meta.bit_depth, 12);
        assert_eq!(meta.cfa_pattern, CfaPattern::Rggb);
        assert_eq!(meta.white_level, 4095);
    }

    // ── CrwFile::parse on non-CRW data ────────────────────────────────────

    #[test]
    fn test_parse_non_crw_returns_error() {
        let data = vec![0u8; 64];
        let cursor = Cursor::new(data);
        let result = CrwFile::parse(cursor);
        assert!(
            matches!(result, Err(RawError::Format(FormatError::Crw(_)))),
            "non-CRW data should produce CrwError"
        );
    }

    #[test]
    fn test_parse_jpeg_magic_returns_error() {
        // JPEG magic (FF D8 FF) should be rejected
        let mut data = vec![0u8; 64];
        data[0] = 0xFF;
        data[1] = 0xD8;
        data[2] = 0xFF;
        let cursor = Cursor::new(data);
        let result = CrwFile::parse(cursor);
        assert!(matches!(result, Err(RawError::Format(FormatError::Crw(_)))));
    }

    // ── CrwFile::parse on valid CRW magic ─────────────────────────────────

    #[test]
    fn test_parse_valid_magic_succeeds() {
        let data = make_crw_magic(true);
        let cursor = Cursor::new(data);
        let file = CrwFile::parse(cursor).expect("should parse valid CRW magic");
        let meta = file.metadata().expect("metadata should be present");
        assert_eq!(meta.make, "Canon");
        assert_eq!(meta.bit_depth, 12);
        assert_eq!(meta.cfa_pattern, CfaPattern::Rggb);
    }

    // ── decode_raw stub ───────────────────────────────────────────────────

    #[test]
    fn test_decode_raw_returns_unsupported_operation() {
        let data = make_crw_magic(true);
        let cursor = Cursor::new(data);
        let mut file = CrwFile::parse(cursor).expect("parse should succeed");
        let result = file.decode_raw();
        assert!(
            matches!(result, Err(RawError::Unsupported(_))),
            "decode_raw should return UnsupportedOperation"
        );
    }
}
