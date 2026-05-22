//! Canon CR3 format decoder.
//!
//! This module provides parsing for Canon CR3 (Canon Raw version 3) files,
//! which use the ISOBMFF (ISO Base Media File Format) container — the same
//! container as MP4/MOV/HEIF files.
//!
//! ## Format Structure
//!
//! CR3 is an ISOBMFF container with the following key boxes:
//! - `ftyp`: File type box with brand `crx ` or `crx2`
//! - `moov`: Movie box containing metadata
//!   - `uuid` (Canon UUID): Canon-specific metadata containing CMT1/CMT2/CMT3/CMT4
//!   - `trak`: Track boxes with raw data location info
//! - `mdat`: Media data box containing the actual CRX-compressed raw pixel data
//!
//! ## CRX Codec
//!
//! The actual raw pixel data uses Canon's proprietary CRX codec (lossless or lossy).
//! This implementation fully extracts metadata but stubs the pixel decode, returning
//! a clear error indicating that CRX decoding is not yet implemented.

use std::io::{Cursor, Read, Seek, SeekFrom};

use tracing::instrument;

use crate::core::image::{CfaPattern, RawImage, Rect, Size, white_level_from_bit_depth};
use crate::error::{FormatError, RawError, RawResult};
use crate::tiff::{TiffParser, TiffTag, TiffValue};

// ── Canon UUID ────────────────────────────────────────────────────────────────

/// Canon-specific UUID: `{85c0b687-820f-11e0-8111-f4ce462b6a48}`.
/// Stored as raw bytes in the ISOBMFF uuid box.
const CANON_UUID: [u8; 16] = [
    0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48,
];

// ── ISOBMFF box types (as u32 big-endian) ────────────────────────────────────

const BOX_FTYP: [u8; 4] = *b"ftyp";
const BOX_MOOV: [u8; 4] = *b"moov";
const BOX_UUID: [u8; 4] = *b"uuid";
const BOX_TRAK: [u8; 4] = *b"trak";
const BOX_MDIA: [u8; 4] = *b"mdia";
const BOX_MINF: [u8; 4] = *b"minf";
const BOX_STBL: [u8; 4] = *b"stbl";
const BOX_STCO: [u8; 4] = *b"stco";
const BOX_CO64: [u8; 4] = *b"co64";
const BOX_STSZ: [u8; 4] = *b"stsz";
const BOX_STSD: [u8; 4] = *b"stsd";

/// Canon sub-boxes inside the Canon UUID payload.
const BOX_CMT1: [u8; 4] = *b"CMT1";

// ── Metadata ──────────────────────────────────────────────────────────────────

/// Metadata extracted from a Canon CR3 file.
#[derive(Debug, Clone)]
pub struct Cr3Metadata {
    /// Camera manufacturer (typically "Canon")
    pub make: String,
    /// Camera model (e.g., "Canon EOS R5")
    pub model: String,
    /// Full sensor dimensions
    pub sensor_size: Size,
    /// Active/crop area (full sensor size if unavailable)
    pub active_area: Rect,
    /// Bits per sample (typically 14)
    pub bit_depth: u8,
    /// CFA pattern (Bayer arrangement; Canon defaults to RGGB)
    pub cfa_pattern: CfaPattern,
    /// Black level values (per CFA channel)
    pub black_levels: [u16; 4],
    /// White/saturation level
    pub white_level: u16,
    /// Byte offset to the raw CRX data in the file
    pub raw_data_offset: u64,
    /// Size of the raw CRX data in bytes
    pub raw_data_size: u64,
}

// ── ISOBMFF box description ───────────────────────────────────────────────────

/// A parsed ISOBMFF box header.
#[derive(Debug, Clone)]
struct IsobmffBox {
    /// Four-character box type.
    box_type: [u8; 4],
    /// Absolute offset in the file where the payload (after the header) begins.
    payload_offset: u64,
    /// Size of the payload in bytes.
    payload_size: u64,
}

/// Read a single ISOBMFF box header from `reader` at the current position.
///
/// Returns `Ok(None)` if there are fewer than 8 bytes remaining.
fn read_box_header<R: Read + Seek>(reader: &mut R) -> RawResult<Option<IsobmffBox>> {
    let header_start = reader.stream_position()?;

    // Read 4-byte size + 4-byte type
    let mut header = [0u8; 8];
    match reader.read_exact(&mut header) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(RawError::Io(e)),
    }

    let size_u32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let box_type = [header[4], header[5], header[6], header[7]];

    let (header_size, total_size): (u64, u64) = if size_u32 == 1 {
        // Extended 64-bit size follows
        let mut ext = [0u8; 8];
        reader.read_exact(&mut ext)?;
        let ext_size = u64::from_be_bytes(ext);
        if ext_size < 16 {
            return Err(RawError::Format(FormatError::Cr3(format!(
                "ISOBMFF box at {header_start}: extended size {ext_size} < 16"
            ))));
        }
        (16, ext_size)
    } else if size_u32 == 0 {
        // Box extends to end of file
        let current = reader.stream_position()?;
        let end = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(current))?;
        let total = end - header_start;
        (8, total)
    } else {
        (8, size_u32 as u64)
    };

    if total_size < header_size {
        return Err(RawError::Format(FormatError::Cr3(format!(
            "ISOBMFF box at {header_start}: total_size {total_size} < header_size {header_size}"
        ))));
    }

    let payload_offset = header_start + header_size;
    let payload_size = total_size - header_size;

    Ok(Some(IsobmffBox {
        box_type,
        payload_offset,
        payload_size,
    }))
}

/// Read all ISOBMFF boxes within a region `[start, start+limit)` of the file.
///
/// `reader` must be positioned at `start` before calling.
fn read_boxes<R: Read + Seek>(reader: &mut R, limit: u64) -> RawResult<Vec<IsobmffBox>> {
    let start = reader.stream_position()?;
    let end = start + limit;
    let mut boxes = Vec::new();

    loop {
        let pos = reader.stream_position()?;
        if pos >= end {
            break;
        }

        match read_box_header(reader)? {
            None => break,
            Some(b) => {
                // Advance past this box's payload to the next box
                let next = b.payload_offset + b.payload_size;
                boxes.push(b);
                if next > end {
                    break;
                }
                reader.seek(SeekFrom::Start(next))?;
            }
        }
    }

    Ok(boxes)
}

// ── CR3 detection ─────────────────────────────────────────────────────────────

/// Return `true` if `data` starts with an ISOBMFF `ftyp` box whose major brand
/// is `crx ` or `crx2`, identifying the file as a Canon CR3.
pub fn is_cr3(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }
    // Bytes 4-7 must be "ftyp"; bytes 8-11 are the major brand
    &data[4..8] == BOX_FTYP.as_ref() && (&data[8..12] == b"crx " || &data[8..12] == b"crx2")
}

// ── CR3 file parser ───────────────────────────────────────────────────────────

/// Parsed Canon CR3 file.
pub struct Cr3File<R> {
    reader: R,
    metadata: Option<Cr3Metadata>,
}

impl<R> std::fmt::Debug for Cr3File<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cr3File")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek> Cr3File<R> {
    /// Parse a Canon CR3 file.
    ///
    /// Reads the ISOBMFF container, validates the CR3 brand, and extracts all
    /// available metadata (Make, Model, dimensions, CFA pattern, raw data location).
    #[instrument(skip(reader), name = "Cr3File::parse")]
    pub fn parse(mut reader: R) -> RawResult<Self> {
        // Read enough bytes for CR3 detection
        let mut magic = [0u8; 12];
        reader.read_exact(&mut magic)?;
        if !is_cr3(&magic) {
            return Err(RawError::Format(FormatError::Cr3(
                "Not a CR3 file: ftyp brand is not crx /crx2".to_string(),
            )));
        }
        reader.seek(SeekFrom::Start(0))?;

        // Determine total file size
        let file_size = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;

        let mut cr3 = Cr3File {
            reader,
            metadata: None,
        };

        cr3.parse_isobmff(file_size)?;
        Ok(cr3)
    }

    /// Get extracted metadata.
    pub fn metadata(&self) -> Option<&Cr3Metadata> {
        self.metadata.as_ref()
    }

    /// Attempt to decode the raw image.
    /// Extract the embedded JPEG thumbnail.
    ///
    /// CR3 thumbnail extraction from ISOBMFF tracks is not yet implemented.
    pub fn thumbnail(&mut self) -> RawResult<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Decode the raw image data.
    ///
    /// CR3 uses Canon's proprietary CRX codec; full decoding is not yet
    /// implemented. This method always returns an informative error.
    #[instrument(skip(self), name = "Cr3File::decode_raw")]
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        Err(RawError::Format(FormatError::Cr3(
            "CRX codec not yet implemented; metadata extraction only".to_string(),
        )))
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Walk the top-level ISOBMFF boxes and extract metadata.
    fn parse_isobmff(&mut self, file_size: u64) -> RawResult<()> {
        self.reader.seek(SeekFrom::Start(0))?;
        let top_boxes = read_boxes(&mut self.reader, file_size)?;

        // Locate the moov box
        let moov = top_boxes
            .iter()
            .find(|b| b.box_type == BOX_MOOV)
            .ok_or_else(|| {
                RawError::Format(FormatError::Cr3(
                    "No moov box found in CR3 file".to_string(),
                ))
            })?
            .clone();

        self.parse_moov(moov)?;
        Ok(())
    }

    /// Parse the `moov` box, looking for the Canon UUID and trak boxes.
    fn parse_moov(&mut self, moov: IsobmffBox) -> RawResult<()> {
        self.reader.seek(SeekFrom::Start(moov.payload_offset))?;
        let moov_boxes = read_boxes(&mut self.reader, moov.payload_size)?;

        // Extract make/model from the Canon UUID box
        let (make, model, cfa_pattern_opt) = self.parse_canon_uuid_boxes(&moov_boxes)?;

        // Extract raw data location from trak boxes
        let (raw_offset, raw_size, sensor_size_opt) = self.parse_trak_boxes(&moov_boxes)?;

        // Choose sensible defaults
        let bit_depth: u8 = 14;
        let white_level = white_level_from_bit_depth(bit_depth);

        let sensor_size = sensor_size_opt.unwrap_or(Size::new(0, 0));
        let active_area = Rect::from_coords(0, 0, sensor_size.width, sensor_size.height);
        let cfa_pattern = cfa_pattern_opt.unwrap_or(CfaPattern::Rggb);

        self.metadata = Some(Cr3Metadata {
            make,
            model,
            sensor_size,
            active_area,
            bit_depth,
            cfa_pattern,
            black_levels: [0, 0, 0, 0],
            white_level,
            raw_data_offset: raw_offset,
            raw_data_size: raw_size,
        });

        Ok(())
    }

    /// Find the Canon UUID box within `moov_boxes` and parse CMT1 for Make/Model.
    ///
    /// Returns `(make, model, optional_cfa_pattern)`.
    fn parse_canon_uuid_boxes(
        &mut self,
        moov_boxes: &[IsobmffBox],
    ) -> RawResult<(String, String, Option<CfaPattern>)> {
        for b in moov_boxes {
            if b.box_type != BOX_UUID {
                continue;
            }
            if b.payload_size < 16 {
                continue;
            }

            // Read the 16-byte UUID
            self.reader.seek(SeekFrom::Start(b.payload_offset))?;
            let mut uuid = [0u8; 16];
            self.reader.read_exact(&mut uuid)?;
            if uuid != CANON_UUID {
                continue;
            }

            // This is the Canon UUID box; parse sub-boxes after the 16-byte UUID
            let sub_payload_offset = b.payload_offset + 16;
            let sub_payload_size = b.payload_size - 16;

            self.reader.seek(SeekFrom::Start(sub_payload_offset))?;
            let sub_boxes = read_boxes(&mut self.reader, sub_payload_size)?;

            return self.parse_canon_cmt_boxes(&sub_boxes);
        }

        // No Canon UUID found — return empty strings (non-fatal)
        Ok((String::new(), String::new(), None))
    }

    /// Parse the CMT1/CMT2/CMT3/CMT4 sub-boxes of the Canon UUID.
    fn parse_canon_cmt_boxes(
        &mut self,
        cmt_boxes: &[IsobmffBox],
    ) -> RawResult<(String, String, Option<CfaPattern>)> {
        let mut make = String::new();
        let mut model = String::new();
        let mut cfa_pattern: Option<CfaPattern> = None;

        for b in cmt_boxes {
            if b.box_type == BOX_CMT1 && b.payload_size > 0 {
                // CMT1 contains a standard TIFF/IFD structure
                let size = b.payload_size.min(65536) as usize;
                self.reader.seek(SeekFrom::Start(b.payload_offset))?;
                let mut buf = vec![0u8; size];
                self.reader.read_exact(&mut buf)?;

                if let Ok(mut parser) = TiffParser::new(Cursor::new(&buf))
                    && let Ok(ifd0) = parser.parse_ifd0()
                {
                    // Make
                    if let Some(entry) = ifd0.get(TiffTag::Make)
                        && let Ok(val) = parser.read_value(entry)
                    {
                        make = val.as_str().unwrap_or("").trim().to_string();
                    }
                    // Model
                    if let Some(entry) = ifd0.get(TiffTag::Model)
                        && let Ok(val) = parser.read_value(entry)
                    {
                        model = val.as_str().unwrap_or("").trim().to_string();
                    }
                    // CFA pattern (if present)
                    if let Some(entry) = ifd0.get(TiffTag::CFAPattern)
                        && let Ok(TiffValue::Bytes(bytes)) = parser.read_value(entry)
                        && bytes.len() >= 4
                    {
                        let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
                        cfa_pattern = CfaPattern::from_array(arr);
                    }
                }
            }
        }

        Ok((make, model, cfa_pattern))
    }

    /// Find the raw-data track (`trak`) and extract chunk offsets and sample sizes.
    ///
    /// CR3 files have multiple tracks (JPEG preview, raw CRX, etc.).
    /// We pick the largest track (by total size) as the raw track.
    ///
    /// Returns `(raw_data_offset, raw_data_size, optional_sensor_size)`.
    fn parse_trak_boxes(
        &mut self,
        moov_boxes: &[IsobmffBox],
    ) -> RawResult<(u64, u64, Option<Size>)> {
        let mut best_offset: u64 = 0;
        let mut best_size: u64 = 0;
        let mut best_sensor_size: Option<Size> = None;

        for b in moov_boxes {
            if b.box_type != BOX_TRAK {
                continue;
            }

            if let Ok((offset, size, sensor_size)) = self.parse_single_trak(b)
                && size > best_size
            {
                best_size = size;
                best_offset = offset;
                best_sensor_size = sensor_size;
            }
        }

        Ok((best_offset, best_size, best_sensor_size))
    }

    /// Parse a single `trak` box and extract raw data location.
    fn parse_single_trak(&mut self, trak: &IsobmffBox) -> RawResult<(u64, u64, Option<Size>)> {
        self.reader.seek(SeekFrom::Start(trak.payload_offset))?;
        let trak_boxes = read_boxes(&mut self.reader, trak.payload_size)?;

        // Find mdia
        let mdia = match trak_boxes.iter().find(|b| b.box_type == BOX_MDIA) {
            Some(b) => b.clone(),
            None => return Ok((0, 0, None)),
        };

        self.reader.seek(SeekFrom::Start(mdia.payload_offset))?;
        let mdia_boxes = read_boxes(&mut self.reader, mdia.payload_size)?;

        // Find minf inside mdia
        let minf = match mdia_boxes.iter().find(|b| b.box_type == BOX_MINF) {
            Some(b) => b.clone(),
            None => return Ok((0, 0, None)),
        };

        self.reader.seek(SeekFrom::Start(minf.payload_offset))?;
        let minf_boxes = read_boxes(&mut self.reader, minf.payload_size)?;

        // Find stbl inside minf
        let stbl = match minf_boxes.iter().find(|b| b.box_type == BOX_STBL) {
            Some(b) => b.clone(),
            None => return Ok((0, 0, None)),
        };

        self.reader.seek(SeekFrom::Start(stbl.payload_offset))?;
        let stbl_boxes = read_boxes(&mut self.reader, stbl.payload_size)?;

        // Extract sensor dimensions from stsd (sample description)
        let sensor_size = self.parse_stsd_for_size(&stbl_boxes);

        // Extract chunk offsets: prefer co64 (64-bit) over stco (32-bit)
        let chunk_offset = if let Some(co64) = stbl_boxes.iter().find(|b| b.box_type == BOX_CO64) {
            self.read_co64(co64)?
        } else if let Some(stco) = stbl_boxes.iter().find(|b| b.box_type == BOX_STCO) {
            self.read_stco(stco)?
        } else {
            return Ok((0, 0, sensor_size));
        };

        // Extract total sample size from stsz
        let total_size = if let Some(stsz) = stbl_boxes.iter().find(|b| b.box_type == BOX_STSZ) {
            self.read_stsz_total(stsz)?
        } else {
            0
        };

        Ok((chunk_offset, total_size, sensor_size))
    }

    /// Try to extract image dimensions from the `stsd` (sample description) box.
    fn parse_stsd_for_size(&mut self, stbl_boxes: &[IsobmffBox]) -> Option<Size> {
        let stsd = stbl_boxes.iter().find(|b| b.box_type == BOX_STSD)?;

        // stsd layout: version(1) + flags(3) + entry_count(4) + entries…
        // Each visual sample entry: size(4) + type(4) + reserved(6) + data_ref_idx(2) + ...
        // For visual entries: + reserved2(16) + width(2) + height(2) at offset 24 in entry
        if stsd.payload_size < 8 {
            return None;
        }

        self.reader
            .seek(SeekFrom::Start(stsd.payload_offset))
            .ok()?;
        let mut header = [0u8; 8];
        self.reader.read_exact(&mut header).ok()?;
        // header[0] = version, header[1..4] = flags, header[4..8] = entry_count
        let entry_count = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
        if entry_count == 0 {
            return None;
        }

        // Read the first sample entry header (8 bytes: size + type)
        let entry_start = self.reader.stream_position().ok()?;
        let mut entry_header = [0u8; 8];
        self.reader.read_exact(&mut entry_header).ok()?;
        let entry_size = u32::from_be_bytes([
            entry_header[0],
            entry_header[1],
            entry_header[2],
            entry_header[3],
        ]);

        // Visual sample entries have width/height at byte offset 24 within the entry
        // (after: 4-byte size, 4-byte type, 6-byte reserved, 2-byte data_ref_idx,
        //  16-byte pre_defined/reserved2 = 28 bytes total before w/h, but
        //  the 8-byte header (size+type) is already read, so we need 24 - 8 = 16 more bytes
        //  before width/height: 6 (reserved) + 2 (data_ref_idx) + 16 (pre_defined) = 24 bytes
        //  offset from start of entry = 28 bytes, minus 8 already read = 20 bytes to skip)
        if entry_size < 28 {
            return None;
        }

        // Skip to width/height: 6 + 2 + 16 = 24 bytes after the 8-byte header
        let mut skip = [0u8; 24];
        self.reader.read_exact(&mut skip).ok()?;

        let mut wh = [0u8; 4];
        self.reader.read_exact(&mut wh).ok()?;
        let width = u16::from_be_bytes([wh[0], wh[1]]) as u32;
        let height = u16::from_be_bytes([wh[2], wh[3]]) as u32;

        // Restore position to entry_start + entry_size
        let _ = self
            .reader
            .seek(SeekFrom::Start(entry_start + entry_size as u64));

        if width > 0 && height > 0 {
            Some(Size::new(width, height))
        } else {
            None
        }
    }

    /// Read the first 32-bit chunk offset from a `stco` box.
    fn read_stco(&mut self, stco: &IsobmffBox) -> RawResult<u64> {
        if stco.payload_size < 8 {
            return Ok(0);
        }
        self.reader.seek(SeekFrom::Start(stco.payload_offset))?;
        // version(1) + flags(3) + entry_count(4)
        let mut header = [0u8; 8];
        self.reader.read_exact(&mut header)?;
        let entry_count = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
        if entry_count == 0 {
            return Ok(0);
        }
        let mut buf = [0u8; 4];
        self.reader.read_exact(&mut buf)?;
        Ok(u32::from_be_bytes(buf) as u64)
    }

    /// Read the first 64-bit chunk offset from a `co64` box.
    fn read_co64(&mut self, co64: &IsobmffBox) -> RawResult<u64> {
        if co64.payload_size < 12 {
            return Ok(0);
        }
        self.reader.seek(SeekFrom::Start(co64.payload_offset))?;
        // version(1) + flags(3) + entry_count(4)
        let mut header = [0u8; 8];
        self.reader.read_exact(&mut header)?;
        let entry_count = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
        if entry_count == 0 {
            return Ok(0);
        }
        let mut buf = [0u8; 8];
        self.reader.read_exact(&mut buf)?;
        Ok(u64::from_be_bytes(buf))
    }

    /// Sum all sample sizes from a `stsz` box to get total raw data size.
    fn read_stsz_total(&mut self, stsz: &IsobmffBox) -> RawResult<u64> {
        if stsz.payload_size < 12 {
            return Ok(0);
        }
        self.reader.seek(SeekFrom::Start(stsz.payload_offset))?;
        // version(1) + flags(3) + sample_size(4) + sample_count(4)
        let mut header = [0u8; 12];
        self.reader.read_exact(&mut header)?;
        let sample_size = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
        let sample_count = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);

        if sample_size > 0 {
            // All samples have the same size
            return Ok(sample_size as u64 * sample_count as u64);
        }

        // Variable-size entries: read and sum
        let mut total: u64 = 0;
        for _ in 0..sample_count {
            let mut buf = [0u8; 4];
            match self.reader.read_exact(&mut buf) {
                Ok(_) => total += u32::from_be_bytes(buf) as u64,
                Err(_) => break,
            }
        }
        Ok(total)
    }
}

// ── MetadataExtractor trait ───────────────────────────────────────────────────

impl<R: Read + Seek> crate::core::MetadataExtractor for Cr3File<R> {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // -------------------------------------------------------------------------
    // CR3 detection tests
    // -------------------------------------------------------------------------

    fn make_ftyp_box(brand: &[u8; 4]) -> Vec<u8> {
        // Minimal ftyp box: size(4) + type(4) + major_brand(4) + minor_version(4) + compat(4) = 20 bytes
        let mut data = Vec::with_capacity(20);
        data.extend_from_slice(&20u32.to_be_bytes()); // box size = 20
        data.extend_from_slice(b"ftyp"); // box type
        data.extend_from_slice(brand); // major brand
        data.extend_from_slice(&0u32.to_be_bytes()); // minor version
        data.extend_from_slice(brand); // compatible brand
        data
    }

    #[test]
    fn test_is_cr3_brand_crx_space() {
        let data = make_ftyp_box(b"crx ");
        assert!(is_cr3(&data), "Brand 'crx ' should be detected as CR3");
    }

    #[test]
    fn test_is_cr3_brand_crx2() {
        let data = make_ftyp_box(b"crx2");
        assert!(is_cr3(&data), "Brand 'crx2' should be detected as CR3");
    }

    #[test]
    fn test_is_cr3_non_cr3_brand() {
        let data = make_ftyp_box(b"mp41");
        assert!(!is_cr3(&data), "Brand 'mp41' must not be detected as CR3");
    }

    #[test]
    fn test_is_cr3_not_ftyp_box() {
        // Box type "moov" instead of "ftyp"
        let mut data = make_ftyp_box(b"crx ");
        data[4] = b'm';
        data[5] = b'o';
        data[6] = b'o';
        data[7] = b'v';
        assert!(
            !is_cr3(&data),
            "Non-ftyp box with crx brand must not be CR3"
        );
    }

    #[test]
    fn test_is_cr3_too_short() {
        let data = b"ftyp";
        assert!(
            !is_cr3(data),
            "Too-short buffer must not be detected as CR3"
        );
    }

    #[test]
    fn test_is_cr3_all_zeros() {
        let data = [0u8; 32];
        assert!(!is_cr3(&data), "All-zero bytes must not be CR3");
    }

    #[test]
    fn test_is_cr3_tiff_magic_not_cr3() {
        // A TIFF-style header should not match CR3
        let mut data = vec![0u8; 32];
        data[0] = b'I';
        data[1] = b'I';
        data[2] = 0x2A;
        data[3] = 0x00;
        assert!(!is_cr3(&data), "TIFF header must not be detected as CR3");
    }

    // -------------------------------------------------------------------------
    // Cr3Metadata struct field tests (no real file)
    // -------------------------------------------------------------------------

    #[test]
    fn test_cr3_metadata_fields() {
        let meta = Cr3Metadata {
            make: "Canon".to_string(),
            model: "Canon EOS R5".to_string(),
            sensor_size: Size::new(8192, 5464),
            active_area: Rect::from_coords(0, 0, 8192, 5464),
            bit_depth: 14,
            cfa_pattern: CfaPattern::Rggb,
            black_levels: [512, 512, 512, 512],
            white_level: 16383,
            raw_data_offset: 2_097_152,
            raw_data_size: 50_000_000,
        };

        assert_eq!(meta.make, "Canon");
        assert_eq!(meta.model, "Canon EOS R5");
        assert_eq!(meta.sensor_size.width, 8192);
        assert_eq!(meta.sensor_size.height, 5464);
        assert_eq!(meta.bit_depth, 14);
        assert_eq!(meta.cfa_pattern, CfaPattern::Rggb);
        assert_eq!(meta.black_levels, [512, 512, 512, 512]);
        assert_eq!(meta.white_level, 16383);
        assert_eq!(meta.raw_data_offset, 2_097_152);
        assert_eq!(meta.raw_data_size, 50_000_000);
    }

    // -------------------------------------------------------------------------
    // parse() on non-CR3 data must return Cr3Error
    // -------------------------------------------------------------------------

    #[test]
    fn test_parse_non_cr3_returns_error() {
        // A TIFF file (not a CR3) should be rejected by parse()
        let mut data = vec![0u8; 32];
        data[0] = b'I';
        data[1] = b'I';
        data[2] = 0x2A;
        data[3] = 0x00;
        let cursor = Cursor::new(data);
        let result = Cr3File::parse(cursor);
        assert!(
            matches!(result, Err(RawError::Format(FormatError::Cr3(_)))),
            "TIFF data should produce Cr3Error, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_parse_empty_returns_error() {
        let cursor = Cursor::new(vec![]);
        let result = Cr3File::parse(cursor);
        assert!(result.is_err(), "Empty data should fail to parse as CR3");
    }

    #[test]
    fn test_parse_ftyp_only_returns_error() {
        // Valid CR3 ftyp box but no moov box — parse should fail with Cr3Error
        let data = make_ftyp_box(b"crx ");
        let cursor = Cursor::new(data);
        let result = Cr3File::parse(cursor);
        assert!(
            matches!(result, Err(RawError::Format(FormatError::Cr3(_)))),
            "CR3 ftyp without moov should produce Cr3Error"
        );
    }

    // -------------------------------------------------------------------------
    // ISOBMFF box size logic tests with synthetic data
    // -------------------------------------------------------------------------

    #[test]
    fn test_read_box_header_normal_size() {
        // Box with size=16, type="test", 8-byte payload
        let mut data = Vec::new();
        data.extend_from_slice(&16u32.to_be_bytes()); // size
        data.extend_from_slice(b"test"); // type
        data.extend_from_slice(&[0u8; 8]); // payload

        let mut cursor = Cursor::new(data);
        let b = read_box_header(&mut cursor).unwrap().unwrap();
        assert_eq!(&b.box_type, b"test");
        assert_eq!(b.payload_offset, 8);
        assert_eq!(b.payload_size, 8);
    }

    #[test]
    fn test_read_box_header_extended_size() {
        // Extended size: size field = 1, then 8-byte size = 24 total (8 header + 8 ext + 8 payload)
        let total: u64 = 24;
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes()); // size = 1 (extended)
        data.extend_from_slice(b"ext1"); // type
        data.extend_from_slice(&total.to_be_bytes()); // extended size
        data.extend_from_slice(&[0u8; 8]); // payload

        let mut cursor = Cursor::new(data);
        let b = read_box_header(&mut cursor).unwrap().unwrap();
        assert_eq!(&b.box_type, b"ext1");
        assert_eq!(b.payload_offset, 16); // 4 + 4 + 8
        assert_eq!(b.payload_size, 8); // 24 - 16
    }

    #[test]
    fn test_read_box_header_empty_returns_none() {
        let cursor = Cursor::new(vec![]);
        let result = read_box_header(&mut cursor.clone());
        // Empty reader should return None (UnexpectedEof handled as None)
        assert!(
            matches!(result, Ok(None)),
            "Empty reader should return Ok(None)"
        );
    }

    #[test]
    fn test_read_boxes_multiple() {
        // Two adjacent boxes
        let mut data = Vec::new();
        // Box 1: size=8, type="box1", no payload
        data.extend_from_slice(&8u32.to_be_bytes());
        data.extend_from_slice(b"box1");
        // Box 2: size=16, type="box2", 8-byte payload
        data.extend_from_slice(&16u32.to_be_bytes());
        data.extend_from_slice(b"box2");
        data.extend_from_slice(&[0xABu8; 8]);

        let total = data.len() as u64;
        let mut cursor = Cursor::new(data);
        let boxes = read_boxes(&mut cursor, total).unwrap();
        assert_eq!(boxes.len(), 2);
        assert_eq!(&boxes[0].box_type, b"box1");
        assert_eq!(boxes[0].payload_size, 0);
        assert_eq!(&boxes[1].box_type, b"box2");
        assert_eq!(boxes[1].payload_size, 8);
    }

    // -------------------------------------------------------------------------
    // decode_raw must return Cr3Error (CRX not implemented)
    // -------------------------------------------------------------------------

    #[test]
    fn test_decode_raw_returns_not_implemented() {
        // Build a minimal valid CR3 structure: ftyp + moov (with empty trak sub-structure)
        // so parse() succeeds, then decode_raw() should fail with Cr3Error.
        let ftyp = make_ftyp_box(b"crx ");

        // Minimal moov with no useful content: just moov box with 0 payload
        // Actually we need a moov that parse_moov can handle; it reads sub-boxes
        // so we give it a moov with just 8 bytes (empty payload after the header).
        // parse_moov will fail to find Canon UUID (ok, returns empty strings)
        // and parse_trak_boxes will find no trak (ok, returns 0,0,None).
        // So the whole parse() should succeed.
        let moov_payload: Vec<u8> = Vec::new();
        let moov_size = 8u32 + moov_payload.len() as u32;
        let mut moov = Vec::new();
        moov.extend_from_slice(&moov_size.to_be_bytes());
        moov.extend_from_slice(b"moov");
        moov.extend_from_slice(&moov_payload);

        let mut file_data = ftyp;
        file_data.extend_from_slice(&moov);

        let cursor = Cursor::new(file_data);
        let mut cr3 = Cr3File::parse(cursor).expect("Should parse minimal CR3");
        let result = cr3.decode_raw();
        assert!(
            matches!(result, Err(RawError::Format(FormatError::Cr3(_)))),
            "decode_raw must return Cr3Error for unimplemented CRX codec"
        );
    }
}
