//! Fujifilm RAF format decoder.
//!
//! This module provides parsing for Fujifilm RAF (Raw Fujifilm) files.
//! RAF is Fujifilm's proprietary RAW format that embeds a JPEG preview
//! and raw sensor data within a custom container.
//!
//! ## Format Structure
//!
//! RAF files start with a 160-byte header followed by:
//! - An embedded JPEG preview (at `jpeg_offset`) containing EXIF metadata
//! - Raw sensor data (at `raw_data_offset`)
//!
//! All multi-byte values in the RAF header are **big-endian**.

use std::io::{Read, Seek, SeekFrom};

use crate::core::image::{CfaPattern, Dimensions, RawImage, Rect, XTransPattern};
use crate::error::{FormatError, RawError, RawResult};
use tracing::instrument;

/// RAF magic bytes at the beginning of every Fujifilm RAF file.
const RAF_MAGIC: &[u8; 16] = b"FUJIFILMCCD-RAW ";

/// Size of the RAF file header in bytes.
pub const RAF_HEADER_SIZE: usize = 160;

/// Offset of the JPEG image offset field in the RAF header.
const JPEG_OFFSET_FIELD: usize = 84;
/// Offset of the JPEG image size field in the RAF header.
const JPEG_SIZE_FIELD: usize = 88;
/// Offset of the raw data offset field in the RAF header.
const RAW_DATA_OFFSET_FIELD: usize = 92;
/// Offset of the raw header size field in the RAF header (unused for data offset).
#[allow(dead_code)]
const RAW_HEADER_SIZE_FIELD: usize = 96;
/// Offset of the raw data size field in the RAF header.
const RAW_DATA_SIZE_FIELD: usize = 100;

/// Offset of the camera model string in the RAF header.
const MODEL_OFFSET: usize = 28;
/// Length of the camera model string field.
const MODEL_LEN: usize = 32;

/// Default sensor dimensions for Fujifilm cameras (26MP X-T5 etc.).
const DEFAULT_WIDTH: u32 = 6240;
const DEFAULT_HEIGHT: u32 = 4168;

/// Metadata extracted from a Fujifilm RAF file.
#[derive(Debug, Clone)]
pub struct RafMetadata {
    /// Camera manufacturer ("FUJIFILM")
    pub make: String,
    /// Camera model (e.g., "X-T5")
    pub model: String,
    /// Full sensor dimensions
    pub sensor_size: Dimensions,
    /// Active/crop area (full sensor size as RAF does not provide a sub-area)
    pub active_area: Rect,
    /// Bits per sample (12 or 14)
    pub bit_depth: u8,
    /// CFA pattern (Bayer arrangement – used for non-X-Trans models)
    pub cfa_pattern: CfaPattern,
    /// X-Trans 6×6 CFA pattern, set for X-Trans sensor models.
    pub xtrans_pattern: Option<XTransPattern>,
    /// Black level values per CFA channel
    pub black_levels: [u16; 4],
    /// White/saturation level
    pub white_level: u16,
    /// Byte offset to the embedded JPEG preview within the file
    pub jpeg_offset: u64,
    /// Byte size of the embedded JPEG preview
    pub jpeg_size: u64,
    /// Byte offset to the raw CFA data within the file
    pub raw_data_offset: u64,
    /// Byte size of the raw CFA data
    pub raw_data_size: u64,
}

/// Parsed Fujifilm RAF file.
pub struct RafFile<R> {
    reader: R,
    /// Extracted metadata (set after `parse()` succeeds)
    metadata: Option<RafMetadata>,
}

impl<R> std::fmt::Debug for RafFile<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RafFile")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek> RafFile<R> {
    /// Parse a Fujifilm RAF file.
    ///
    /// Validates the RAF magic, reads the header to locate the embedded JPEG
    /// and raw data, then attempts to extract sensor dimensions from the JPEG
    /// EXIF data. Falls back to reasonable defaults when EXIF parsing fails.
    #[instrument(skip(reader))]
    pub fn parse(mut reader: R) -> RawResult<Self> {
        // Read the full header (160 bytes)
        let mut header = [0u8; RAF_HEADER_SIZE];
        reader.read_exact(&mut header).map_err(|e| {
            RawError::Format(FormatError::Raf(format!("Failed to read RAF header: {e}")))
        })?;

        // Validate magic
        if &header[..16] != RAF_MAGIC {
            return Err(RawError::Format(FormatError::Raf(
                "Invalid RAF magic: not a Fujifilm RAF file".to_string(),
            )));
        }

        // Extract camera model string (null-terminated, padded to 32 bytes)
        let model_raw = &header[MODEL_OFFSET..MODEL_OFFSET + MODEL_LEN];
        let model = extract_cstring(model_raw);

        // Read offsets/sizes (all big-endian u32)
        let jpeg_offset = read_be_u32(&header, JPEG_OFFSET_FIELD) as u64;
        let jpeg_size = read_be_u32(&header, JPEG_SIZE_FIELD) as u64;
        let raw_data_offset = read_be_u32(&header, RAW_DATA_OFFSET_FIELD) as u64;
        let raw_data_size = read_be_u32(&header, RAW_DATA_SIZE_FIELD) as u64;

        if raw_data_offset == 0 {
            return Err(RawError::Format(FormatError::Raf(
                "RAF header has zero raw data offset".to_string(),
            )));
        }

        // Determine if this is an X-Trans model
        let xtrans = is_xtrans_model(&model);

        // Attempt to get dimensions from the embedded JPEG EXIF
        let (width, height) = if jpeg_size > 0 {
            extract_dimensions_from_jpeg(&mut reader, jpeg_offset, jpeg_size)
                .unwrap_or((DEFAULT_WIDTH, DEFAULT_HEIGHT))
        } else {
            (DEFAULT_WIDTH, DEFAULT_HEIGHT)
        };

        let sensor_size = Dimensions { width, height };
        let active_area = Rect::from_coords(0, 0, width, height);

        // Fujifilm default calibration values
        let bit_depth: u8 = 14;
        let white_level: u16 = 16383; // (1 << 14) - 1
        let black_levels: [u16; 4] = [512; 4]; // Fujifilm default

        let xtrans_pattern = if xtrans {
            Some(XTransPattern::standard())
        } else {
            None
        };

        let metadata = RafMetadata {
            make: "FUJIFILM".to_string(),
            model,
            sensor_size,
            active_area,
            bit_depth,
            cfa_pattern: CfaPattern::Rggb,
            xtrans_pattern,
            black_levels,
            white_level,
            jpeg_offset,
            jpeg_size,
            raw_data_offset,
            raw_data_size,
        };

        Ok(RafFile {
            reader,
            metadata: Some(metadata),
        })
    }

    /// Return a reference to the extracted metadata, if available.
    pub fn metadata(&self) -> Option<&RafMetadata> {
        self.metadata.as_ref()
    }

    /// Extract the embedded JPEG preview from the RAF file.
    pub fn thumbnail(&mut self) -> RawResult<Option<Vec<u8>>> {
        let metadata = match self.metadata.as_ref() {
            Some(m) => m,
            None => return Ok(None),
        };
        let offset = metadata.jpeg_offset;
        let size = metadata.jpeg_size as usize;
        if size == 0 {
            return Ok(None);
        }
        self.reader.seek(std::io::SeekFrom::Start(offset))?;
        let mut data = vec![0u8; size];
        self.reader.read_exact(&mut data)?;
        Ok(Some(data))
    }

    /// Decode the raw image data into a [`RawImage`].
    ///
    /// Seeks to `raw_data_offset`, skips the small raw sub-header (32 bytes),
    /// reads the remaining bytes, and unpacks them as big-endian 16-bit values.
    #[instrument(skip(self))]
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        let metadata = self.metadata.as_ref().cloned().ok_or_else(|| {
            RawError::Format(FormatError::Raf("Metadata not extracted".to_string()))
        })?;

        let raw_data_size = metadata.raw_data_size as usize;
        if raw_data_size < 32 {
            return Err(RawError::Format(FormatError::Raf(format!(
                "RAF raw data size too small: {raw_data_size} bytes"
            ))));
        }

        // Seek to the raw data section
        self.reader
            .seek(SeekFrom::Start(metadata.raw_data_offset))
            .map_err(|e| {
                RawError::Format(FormatError::Raf(format!("Failed to seek to raw data: {e}")))
            })?;

        // Read the raw data (includes the sub-header)
        let mut raw_bytes = vec![0u8; raw_data_size];
        self.reader.read_exact(&mut raw_bytes).map_err(|e| {
            RawError::Format(FormatError::Raf(format!("Failed to read raw data: {e}")))
        })?;

        // Skip the 32-byte raw sub-header present in newer RAF files
        const RAW_SUB_HEADER: usize = 32;
        let pixel_bytes = if raw_bytes.len() > RAW_SUB_HEADER {
            &raw_bytes[RAW_SUB_HEADER..]
        } else {
            &raw_bytes[..]
        };

        // Unpack big-endian 16-bit pixel values
        let pixels = unpack_raw_16bit(pixel_bytes);

        let expected = metadata
            .sensor_size
            .num_pixels()
            .expect("sensor pixel count overflows usize");
        if pixels.len() != expected {
            return Err(RawError::Format(FormatError::Raf(format!(
                "Pixel count mismatch: got {} pixels, expected {} ({}×{})",
                pixels.len(),
                expected,
                metadata.sensor_size.width,
                metadata.sensor_size.height,
            ))));
        }

        {
            let mut builder = RawImage::builder(
                metadata.sensor_size,
                metadata.active_area,
                metadata.bit_depth,
                metadata.cfa_pattern,
            )
            .black_levels(metadata.black_levels)
            .white_level(metadata.white_level)
            .data(pixels);
            if let Some(xtrans) = metadata.xtrans_pattern {
                builder = builder.xtrans_pattern(xtrans);
            }
            Ok(builder.build())
        }
    }
}

impl<R: Read + Seek> crate::core::ExtractMetadata for RafFile<R> {
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
                default_crop_origin: None,
                default_crop_size: None,
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

// ── Helper functions ──────────────────────────────────────────────────────────

/// Detect whether raw bytes represent a Fujifilm RAF file.
///
/// Returns `true` if the first 16 bytes match the RAF magic string.
pub fn is_raf(data: &[u8]) -> bool {
    data.len() >= 16 && &data[..16] == RAF_MAGIC
}

/// Determine whether a Fujifilm model name uses the X-Trans sensor.
///
/// X-Trans sensors are used in X-series mirrorless cameras. Older
/// S-series cameras use standard Bayer RGGB.
pub fn is_xtrans_model(model: &str) -> bool {
    let model_upper = model.to_uppercase();
    // X-Trans cameras: X-T, X-Pro, X-E, X-H, X-S, X100 series
    model_upper.contains("X-T")
        || model_upper.contains("X-PRO")
        || model_upper.contains("X-E")
        || model_upper.contains("X-H")
        || model_upper.contains("X-S")
        || model_upper.contains("X100")
        || model_upper.contains("X-A") // Some X-A models also use X-Trans
}

/// Unpack big-endian 16-bit pixel values from a byte slice.
pub fn unpack_raw_16bit(raw_bytes: &[u8]) -> Vec<u16> {
    raw_bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect()
}

/// Read a big-endian u32 from a byte slice at the given offset.
fn read_be_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Extract a null-terminated C string from a fixed-length byte slice.
fn extract_cstring(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

/// Attempt to extract sensor dimensions from the embedded JPEG EXIF data.
///
/// The embedded JPEG preview at `jpeg_offset` contains standard EXIF tags.
/// We look for the EXIF APP1 marker (0xFF 0xE1), locate the TIFF header
/// within it, and read ImageWidth (tag 256) and ImageLength (tag 257).
///
/// Returns `None` if parsing fails for any reason (e.g., missing EXIF,
/// truncated data, unrecognised byte order).
fn extract_dimensions_from_jpeg<R: Read + Seek>(
    reader: &mut R,
    jpeg_offset: u64,
    jpeg_size: u64,
) -> Option<(u32, u32)> {
    // Read the JPEG data (cap at 64 KiB to avoid excessive I/O for large previews)
    let read_size = jpeg_size.min(65536) as usize;
    reader.seek(SeekFrom::Start(jpeg_offset)).ok()?;
    let mut jpeg_bytes = vec![0u8; read_size];
    reader.read_exact(&mut jpeg_bytes).ok()?;

    // Find the EXIF APP1 marker (FF E1) after the SOI marker (FF D8)
    if jpeg_bytes.len() < 4 {
        return None;
    }
    // SOI = FF D8
    if jpeg_bytes[0] != 0xFF || jpeg_bytes[1] != 0xD8 {
        return None;
    }

    // Scan for APP1 marker (FF E1)
    let mut pos = 2usize;
    while pos + 3 < jpeg_bytes.len() {
        if jpeg_bytes[pos] != 0xFF {
            return None;
        }
        let marker = jpeg_bytes[pos + 1];
        let seg_len = u16::from_be_bytes([jpeg_bytes[pos + 2], jpeg_bytes[pos + 3]]) as usize;

        if marker == 0xE1 {
            // APP1 found – check for "Exif\0\0" header
            let app1_data = &jpeg_bytes[pos + 2..];
            if app1_data.len() < 8 {
                return None;
            }
            if &app1_data[2..8] != b"Exif\0\0" {
                return None;
            }
            // TIFF header starts at offset 8 within APP1 data (after the 2-byte length)
            let tiff_start = pos + 4; // skip marker (2) + length (2)
            let tiff_offset = tiff_start + 6; // skip "Exif\0\0" (6 bytes)
            if tiff_offset >= jpeg_bytes.len() {
                return None;
            }
            let tiff_data = &jpeg_bytes[tiff_offset..];
            return parse_tiff_dimensions(tiff_data);
        }

        // Skip this segment
        pos += 2 + seg_len;
    }

    None
}

/// Parse ImageWidth and ImageLength from a TIFF header embedded in EXIF data.
///
/// Supports both little-endian ("II") and big-endian ("MM") TIFF byte orders.
fn parse_tiff_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 8 {
        return None;
    }

    // Determine byte order
    let little_endian = match &data[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };

    let read_u16 = |offset: usize| -> Option<u16> {
        if offset + 2 > data.len() {
            return None;
        }
        let bytes = [data[offset], data[offset + 1]];
        if little_endian {
            Some(u16::from_le_bytes(bytes))
        } else {
            Some(u16::from_be_bytes(bytes))
        }
    };

    let read_u32 = |offset: usize| -> Option<u32> {
        if offset + 4 > data.len() {
            return None;
        }
        let bytes = [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ];
        if little_endian {
            Some(u32::from_le_bytes(bytes))
        } else {
            Some(u32::from_be_bytes(bytes))
        }
    };

    // TIFF magic should be 42
    let magic = read_u16(2)?;
    if magic != 42 {
        return None;
    }

    // IFD0 offset
    let ifd_offset = read_u32(4)? as usize;
    if ifd_offset + 2 > data.len() {
        return None;
    }

    let entry_count = read_u16(ifd_offset)? as usize;
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;

    for i in 0..entry_count {
        let entry_offset = ifd_offset + 2 + i * 12;
        if entry_offset + 12 > data.len() {
            break;
        }
        let tag = read_u16(entry_offset)?;
        // type (2 bytes) + count (4 bytes) – skip, we only need offset/value
        let value_raw = read_u32(entry_offset + 8)?;

        match tag {
            256 => width = Some(value_raw),  // ImageWidth
            257 => height = Some(value_raw), // ImageLength
            _ => {}
        }

        if width.is_some() && height.is_some() {
            break;
        }
    }

    match (width, height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => Some((w, h)),
        _ => None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    // ── is_raf detection ──────────────────────────────────────────────────────

    #[test]
    fn test_is_raf_correct_magic() {
        let mut data = vec![0u8; 32];
        data[..16].copy_from_slice(RAF_MAGIC);
        assert!(is_raf(&data), "Correct RAF magic should be detected");
    }

    #[test]
    fn test_is_raf_wrong_magic() {
        let data = vec![0u8; 32];
        assert!(!is_raf(&data), "All-zero bytes should not match RAF magic");
    }

    #[test]
    fn test_is_raf_partial_magic() {
        // Only 8 bytes – too short
        let data = b"FUJIFILM".to_vec();
        assert!(
            !is_raf(&data),
            "Partial magic (8 bytes) should not be detected"
        );
    }

    #[test]
    fn test_is_raf_almost_correct() {
        let mut data = vec![0u8; 32];
        data[..16].copy_from_slice(b"FUJIFILMCCD-RAW!"); // last byte differs
        assert!(!is_raf(&data), "Near-match magic should not be detected");
    }

    // ── is_xtrans_model ───────────────────────────────────────────────────────

    #[test]
    fn test_xtrans_detection_xt5() {
        assert!(is_xtrans_model("X-T5"), "X-T5 should be X-Trans");
    }

    #[test]
    fn test_xtrans_detection_xpro3() {
        assert!(is_xtrans_model("X-Pro3"), "X-Pro3 should be X-Trans");
    }

    #[test]
    fn test_xtrans_detection_x100v() {
        assert!(is_xtrans_model("X100V"), "X100V should be X-Trans");
    }

    #[test]
    fn test_xtrans_detection_xh2() {
        assert!(is_xtrans_model("X-H2"), "X-H2 should be X-Trans");
    }

    #[test]
    fn test_xtrans_detection_xe4() {
        assert!(is_xtrans_model("X-E4"), "X-E4 should be X-Trans");
    }

    #[test]
    fn test_bayer_detection_s_series() {
        // S-series uses Bayer RGGB
        assert!(
            !is_xtrans_model("S5 Pro"),
            "S5 Pro should not be detected as X-Trans"
        );
    }

    #[test]
    fn test_bayer_detection_empty_model() {
        assert!(!is_xtrans_model(""), "Empty model should default to Bayer");
    }

    // ── RafMetadata struct ────────────────────────────────────────────────────

    #[test]
    fn test_raf_metadata_fields() {
        let meta = RafMetadata {
            make: "FUJIFILM".to_string(),
            model: "X-T5".to_string(),
            sensor_size: Dimensions {
                width: 6240,
                height: 4168,
            },
            active_area: Rect::from_coords(0, 0, 6240, 4168),
            bit_depth: 14,
            cfa_pattern: CfaPattern::Rggb,
            xtrans_pattern: Some(XTransPattern::standard()),
            black_levels: [512; 4],
            white_level: 16383,
            jpeg_offset: 160,
            jpeg_size: 1024,
            raw_data_offset: 2048,
            raw_data_size: 52_000_000,
        };

        assert_eq!(meta.make, "FUJIFILM");
        assert_eq!(meta.model, "X-T5");
        assert_eq!(meta.sensor_size.width, 6240);
        assert_eq!(meta.sensor_size.height, 4168);
        assert_eq!(meta.bit_depth, 14);
        assert_eq!(meta.cfa_pattern, CfaPattern::Rggb);
        assert!(meta.xtrans_pattern.is_some());
        assert_eq!(meta.black_levels, [512; 4]);
        assert_eq!(meta.white_level, 16383);
        assert_eq!(meta.jpeg_offset, 160);
        assert_eq!(meta.raw_data_offset, 2048);
        assert_eq!(meta.raw_data_size, 52_000_000);
    }

    // ── RAF_HEADER_SIZE constant ──────────────────────────────────────────────

    #[test]
    fn test_header_size_constant() {
        assert_eq!(RAF_HEADER_SIZE, 160, "RAF header must be exactly 160 bytes");
    }

    // ── unpack_raw_16bit ──────────────────────────────────────────────────────

    #[test]
    fn test_unpack_raw_16bit_basic() {
        let bytes = [0x12u8, 0x34, 0xAB, 0xCD];
        let pixels = unpack_raw_16bit(&bytes);
        assert_eq!(pixels.len(), 2);
        assert_eq!(pixels[0], 0x1234);
        assert_eq!(pixels[1], 0xABCD);
    }

    #[test]
    fn test_unpack_raw_16bit_zeros() {
        let bytes = [0u8; 8];
        let pixels = unpack_raw_16bit(&bytes);
        assert_eq!(pixels, vec![0, 0, 0, 0]);
    }

    #[test]
    fn test_unpack_raw_16bit_max() {
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF];
        let pixels = unpack_raw_16bit(&bytes);
        assert_eq!(pixels, vec![65535, 65535]);
    }

    #[test]
    fn test_unpack_raw_16bit_odd_bytes_ignored() {
        // 5 bytes → 2 complete pairs → 2 pixels, last byte ignored
        let bytes = [0x00, 0x01, 0x00, 0x02, 0xFF];
        let pixels = unpack_raw_16bit(&bytes);
        assert_eq!(pixels.len(), 2);
        assert_eq!(pixels[0], 1);
        assert_eq!(pixels[1], 2);
    }

    // ── parse error on bad magic ──────────────────────────────────────────────

    #[test]
    fn test_parse_rejects_non_raf() {
        let data = vec![0u8; 256]; // all zeros, no RAF magic
        let cursor = Cursor::new(data);
        let result = RafFile::parse(cursor);
        assert!(
            matches!(result, Err(RawError::Format(FormatError::Raf(_)))),
            "Non-RAF data should produce RafError"
        );
    }

    #[test]
    fn test_parse_rejects_truncated_header() {
        // Only 8 bytes — cannot even read the magic
        let data = b"FUJIFILM".to_vec();
        let cursor = Cursor::new(data);
        let result = RafFile::parse(cursor);
        assert!(
            matches!(result, Err(RawError::Format(FormatError::Raf(_)))),
            "Truncated header should produce RafError"
        );
    }

    // ── parse extracts model and sets X-Trans pattern ─────────────────────────

    fn make_minimal_raf_header(model: &str, raw_data_offset: u32, raw_data_size: u32) -> Vec<u8> {
        let mut header = vec![0u8; RAF_HEADER_SIZE];
        // Magic
        header[..16].copy_from_slice(RAF_MAGIC);
        // Format version
        header[16..20].copy_from_slice(b"0200");
        // Camera model (null-padded)
        let model_bytes = model.as_bytes();
        let copy_len = model_bytes.len().min(MODEL_LEN);
        header[MODEL_OFFSET..MODEL_OFFSET + copy_len].copy_from_slice(&model_bytes[..copy_len]);
        // JPEG offset = 0, JPEG size = 0 (no embedded JPEG in test)
        // RAW data offset
        header[RAW_DATA_OFFSET_FIELD..RAW_DATA_OFFSET_FIELD + 4]
            .copy_from_slice(&raw_data_offset.to_be_bytes());
        // RAW data size
        header[RAW_DATA_SIZE_FIELD..RAW_DATA_SIZE_FIELD + 4]
            .copy_from_slice(&raw_data_size.to_be_bytes());
        header
    }

    #[test]
    fn test_parse_xtrans_model_sets_xtrans_pattern() {
        // Build a header for an X-T5 with raw data starting right after the header
        let raw_offset = RAF_HEADER_SIZE as u32;
        let pixel_count = DEFAULT_WIDTH * DEFAULT_HEIGHT;
        // pixel data: 32 bytes sub-header + pixel_count * 2 bytes
        let raw_size = 32 + pixel_count * 2;

        let mut data = make_minimal_raf_header("X-T5", raw_offset, raw_size);
        // Append dummy raw data (all zeros)
        data.resize(data.len() + raw_size as usize, 0);

        let cursor = Cursor::new(data);
        let raf = RafFile::parse(cursor).expect("Should parse minimal RAF header");
        let meta = raf.metadata().expect("Metadata should be present");

        assert_eq!(meta.make, "FUJIFILM");
        assert_eq!(meta.model, "X-T5");
        assert!(
            meta.xtrans_pattern.is_some(),
            "X-T5 should have an X-Trans pattern"
        );
        assert_eq!(meta.bit_depth, 14);
        assert_eq!(meta.white_level, 16383);
        assert_eq!(meta.black_levels, [512; 4]);
    }

    #[test]
    fn test_parse_bayer_model_no_xtrans_pattern() {
        let raw_offset = RAF_HEADER_SIZE as u32;
        let pixel_count = DEFAULT_WIDTH * DEFAULT_HEIGHT;
        let raw_size = 32 + pixel_count * 2;

        let mut data = make_minimal_raf_header("S5 Pro", raw_offset, raw_size);
        data.resize(data.len() + raw_size as usize, 0);

        let cursor = Cursor::new(data);
        let raf = RafFile::parse(cursor).expect("Should parse minimal RAF header");
        let meta = raf.metadata().expect("Metadata should be present");

        assert!(
            meta.xtrans_pattern.is_none(),
            "S5 Pro should use Bayer (no X-Trans pattern)"
        );
    }

    // ── parse_tiff_dimensions ─────────────────────────────────────────────────

    fn make_tiff_ifd(little_endian: bool, width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::new();

        let write_u16 =
            |v: u16, le: bool| -> [u8; 2] { if le { v.to_le_bytes() } else { v.to_be_bytes() } };
        let write_u32 =
            |v: u32, le: bool| -> [u8; 4] { if le { v.to_le_bytes() } else { v.to_be_bytes() } };

        // TIFF header: byte order (2) + magic 42 (2) + IFD offset (4) = 8 bytes
        if little_endian {
            data.extend_from_slice(b"II");
        } else {
            data.extend_from_slice(b"MM");
        }
        data.extend_from_slice(&write_u16(42, little_endian));
        // IFD starts right after the 8-byte TIFF header
        data.extend_from_slice(&write_u32(8, little_endian));

        // IFD: 2 entries (width + height)
        data.extend_from_slice(&write_u16(2, little_endian));

        // Entry: ImageWidth (256), type SHORT (3), count 1, value
        data.extend_from_slice(&write_u16(256, little_endian));
        data.extend_from_slice(&write_u16(3, little_endian)); // SHORT
        data.extend_from_slice(&write_u32(1, little_endian));
        data.extend_from_slice(&write_u32(width, little_endian));

        // Entry: ImageLength (257), type SHORT (3), count 1, value
        data.extend_from_slice(&write_u16(257, little_endian));
        data.extend_from_slice(&write_u16(3, little_endian)); // SHORT
        data.extend_from_slice(&write_u32(1, little_endian));
        data.extend_from_slice(&write_u32(height, little_endian));

        // Next IFD = 0
        data.extend_from_slice(&write_u32(0, little_endian));
        data
    }

    #[test]
    fn test_parse_tiff_dimensions_le() {
        let tiff = make_tiff_ifd(true, 6240, 4168);
        let result = parse_tiff_dimensions(&tiff);
        assert_eq!(result, Some((6240, 4168)));
    }

    #[test]
    fn test_parse_tiff_dimensions_be() {
        let tiff = make_tiff_ifd(false, 5640, 3760);
        let result = parse_tiff_dimensions(&tiff);
        assert_eq!(result, Some((5640, 3760)));
    }

    #[test]
    fn test_parse_tiff_dimensions_invalid() {
        let data = vec![0u8; 16];
        assert_eq!(parse_tiff_dimensions(&data), None);
    }
}
