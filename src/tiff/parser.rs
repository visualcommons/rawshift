//! TIFF IFD parser and navigation.
//!
//! This module provides the core TIFF parsing functionality:
//! - Header parsing (byte order, magic number, IFD0 offset)
//! - IFD entry parsing and navigation
//! - SubIFD tree traversal
//! - Value resolution (inline vs offset)

use binrw::{binread, BinRead, BinReaderExt};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};

use crate::error::{RawError, RawResult};
use crate::tiff::tags::TiffTag;
use crate::tiff::types::{ByteOrder, Rational, SRational, TiffType, TiffValue};

/// Standard TIFF magic number (42).
pub const TIFF_MAGIC: u16 = 42;
/// BigTIFF magic number (43).
pub const BIGTIFF_MAGIC: u16 = 43;

// ============================================================================
// Raw binary structures for binrw parsing
// ============================================================================

/// Raw standard TIFF header (8 bytes) for binrw parsing.
#[derive(Debug, Clone, BinRead)]
pub struct RawTiffHeader {
    /// Byte order marker
    pub byte_order: ByteOrder,
    /// Magic number (should be 42)
    #[br(is_little = matches!(byte_order, ByteOrder::LittleEndian))]
    pub magic: u16,
    /// Offset to IFD0
    #[br(is_little = matches!(byte_order, ByteOrder::LittleEndian))]
    pub ifd0_offset: u32,
}

/// Raw BigTIFF header (16 bytes) for binrw parsing.
#[derive(Debug, Clone, BinRead)]
pub struct RawBigTiffHeader {
    /// Byte order marker
    pub byte_order: ByteOrder,
    /// Magic number (should be 43)
    #[br(is_little = matches!(byte_order, ByteOrder::LittleEndian))]
    pub magic: u16,
    /// Offset byte size (always 8 for BigTIFF)
    #[br(is_little = matches!(byte_order, ByteOrder::LittleEndian))]
    pub offset_bytesize: u16,
    /// Always zero
    #[br(is_little = matches!(byte_order, ByteOrder::LittleEndian))]
    pub always_zero: u16,
    /// Offset to IFD0
    #[br(is_little = matches!(byte_order, ByteOrder::LittleEndian))]
    pub ifd0_offset: u64,
}

/// Raw IFD entry for standard TIFF (12 bytes).
#[binread]
#[derive(Debug, Clone)]
#[br(import { is_little: bool })]
pub struct RawIfdEntry {
    /// Tag ID
    #[br(is_little = is_little)]
    pub tag_id: u16,
    /// Data type
    #[br(is_little = is_little)]
    pub data_type: u16,
    /// Count of values
    #[br(is_little = is_little)]
    pub count: u32,
    /// Value or offset
    #[br(is_little = is_little)]
    pub value_offset: u32,
}

/// Raw IFD entry for BigTIFF (20 bytes).
#[binread]
#[derive(Debug, Clone)]
#[br(import { is_little: bool })]
pub struct RawBigTiffIfdEntry {
    /// Tag ID
    #[br(is_little = is_little)]
    pub tag_id: u16,
    /// Data type
    #[br(is_little = is_little)]
    pub data_type: u16,
    /// Count of values
    #[br(is_little = is_little)]
    pub count: u64,
    /// Value or offset
    #[br(is_little = is_little)]
    pub value_offset: u64,
}

// ============================================================================
// Higher-level parsed structures
// ============================================================================

/// TIFF file header (first 8 bytes).
#[derive(Debug, Clone)]
pub struct TiffHeader {
    /// Byte order (LE or BE)
    pub byte_order: ByteOrder,
    /// Version/magic number (42 for TIFF, 43 for BigTIFF)
    pub magic: u16,
    /// Offset to the first IFD (IFD0)
    pub ifd0_offset: u64,
    /// Whether this is a BigTIFF file
    pub is_bigtiff: bool,
}

impl TiffHeader {
    /// Parse a TIFF header from a reader.
    pub fn parse<R: Read + Seek>(reader: &mut R) -> RawResult<Self> {
        reader.seek(SeekFrom::Start(0))?;

        // First, read just the byte order and magic to determine TIFF type
        let raw_header: RawTiffHeader = reader
            .read_ne()
            .map_err(|_| RawError::InvalidByteOrder(0))?;

        // Validate magic number and determine if BigTIFF
        let is_bigtiff = match raw_header.magic {
            TIFF_MAGIC => false,
            BIGTIFF_MAGIC => true,
            _ => {
                return Err(RawError::InvalidMagic {
                    expected: TIFF_MAGIC,
                    found: raw_header.magic,
                });
            }
        };

        // For BigTIFF, we need to re-parse with the full header
        let ifd0_offset = if is_bigtiff {
            reader.seek(SeekFrom::Start(0))?;
            let bigtiff_header: RawBigTiffHeader = reader
                .read_ne()
                .map_err(|e| RawError::ParseError(e.to_string()))?;
            bigtiff_header.ifd0_offset
        } else {
            raw_header.ifd0_offset as u64
        };

        Ok(TiffHeader {
            byte_order: raw_header.byte_order,
            magic: raw_header.magic,
            ifd0_offset,
            is_bigtiff,
        })
    }
}

/// Raw IFD entry as stored in the file (before value resolution).
#[derive(Debug, Clone)]
pub struct IfdEntry {
    /// Tag ID
    pub tag_id: u16,
    /// Data type
    pub data_type: u16,
    /// Number of values
    pub count: u64,
    /// Value or offset to value (raw 4 or 8 bytes depending on TIFF/BigTIFF)
    pub value_offset: u64,
    /// The resolved type, if known
    pub tiff_type: Option<TiffType>,
    /// The resolved tag, if known
    pub tag: Option<TiffTag>,
}

impl IfdEntry {
    /// Check if this entry's value is stored inline (in the value_offset field).
    pub fn is_inline(&self, is_bigtiff: bool) -> bool {
        if let Some(tiff_type) = self.tiff_type {
            if is_bigtiff {
                tiff_type.fits_inline_bigtiff(self.count)
            } else {
                tiff_type.fits_inline(self.count as u32)
            }
        } else {
            false // Unknown type, assume offset
        }
    }

    /// Get the total byte size of this entry's value.
    pub fn value_size(&self) -> u64 {
        self.tiff_type
            .map(|t| t.size() as u64 * self.count)
            .unwrap_or(0)
    }
}

/// Storage for unknown/proprietary tags.
#[derive(Debug, Clone)]
pub struct UnknownTag {
    /// Raw tag ID
    pub tag_id: u16,
    /// Raw type code
    pub type_code: u16,
    /// Number of values
    pub count: u64,
    /// Raw binary data (if resolved)
    pub data: Option<Vec<u8>>,
    /// Offset to data (if not inline)
    pub offset: Option<u64>,
}

/// A parsed IFD (Image File Directory).
#[derive(Debug, Clone)]
pub struct Ifd {
    /// Offset of this IFD in the file
    pub offset: u64,
    /// Parsed entries (known tags)
    pub entries: HashMap<TiffTag, IfdEntry>,
    /// Unknown tags with their raw data
    pub unknown_tags: HashMap<u16, UnknownTag>,
    /// Offset to next IFD (0 if none)
    pub next_ifd_offset: u64,
    /// SubIFDs (parsed from SubIFDs tag)
    pub sub_ifds: Vec<Ifd>,
    /// EXIF IFD (if present)
    pub exif_ifd: Option<Box<Ifd>>,
}

impl Ifd {
    /// Get a tag value if present.
    pub fn get(&self, tag: TiffTag) -> Option<&IfdEntry> {
        self.entries.get(&tag)
    }

    /// Check if a tag is present.
    pub fn contains(&self, tag: TiffTag) -> bool {
        self.entries.contains_key(&tag)
    }

    /// Get all tag IDs present in this IFD (both known and unknown).
    pub fn all_tag_ids(&self) -> Vec<u16> {
        let mut ids: Vec<u16> = self.entries.keys().map(|t| t.as_u16()).collect();
        ids.extend(self.unknown_tags.keys().copied());
        ids.sort();
        ids
    }
}

/// TIFF file parser.
pub struct TiffParser<R> {
    reader: R,
    header: TiffHeader,
    /// Cache of parsed IFDs by offset
    ifd_cache: HashMap<u64, Ifd>,
    /// Set of visited offsets (for circular reference detection)
    visited_offsets: HashSet<u64>,
}

impl<R: Read + Seek> TiffParser<R> {
    /// Create a new parser and parse the header.
    pub fn new(mut reader: R) -> RawResult<Self> {
        let header = TiffHeader::parse(&mut reader)?;
        Ok(TiffParser {
            reader,
            header,
            ifd_cache: HashMap::new(),
            visited_offsets: HashSet::new(),
        })
    }

    /// Get the parsed header.
    pub fn header(&self) -> &TiffHeader {
        &self.header
    }

    /// Get the byte order.
    pub fn byte_order(&self) -> ByteOrder {
        self.header.byte_order
    }

    /// Check if this is a BigTIFF file.
    pub fn is_bigtiff(&self) -> bool {
        self.header.is_bigtiff
    }

    /// Parse an IFD at the given offset.
    pub fn parse_ifd_at(&mut self, offset: u64) -> RawResult<Ifd> {
        // Check for circular references
        if self.visited_offsets.contains(&offset) {
            return Err(RawError::CircularReference(offset));
        }
        self.visited_offsets.insert(offset);

        // Check cache
        if let Some(ifd) = self.ifd_cache.get(&offset) {
            return Ok(ifd.clone());
        }

        self.reader.seek(SeekFrom::Start(offset))?;

        // Read entry count
        let entry_count: u64 = if self.header.is_bigtiff {
            self.read_u64()?
        } else {
            self.read_u16()? as u64
        };

        // Sanity check on entry count
        if entry_count > 65535 {
            return Err(RawError::InvalidIfd {
                offset,
                reason: format!("Entry count {} is unreasonably large", entry_count),
            });
        }

        let mut entries = HashMap::new();
        let mut unknown_tags = HashMap::new();

        // Parse each entry
        for _ in 0..entry_count {
            let entry = self.parse_ifd_entry()?;

            if let Some(tag) = entry.tag {
                entries.insert(tag, entry);
            } else {
                // Unknown tag - store with raw data
                let unknown = UnknownTag {
                    tag_id: entry.tag_id,
                    type_code: entry.data_type,
                    count: entry.count,
                    data: None, // TODO: Resolve data lazily or eagerly
                    offset: if entry.is_inline(self.header.is_bigtiff) {
                        None
                    } else {
                        Some(entry.value_offset)
                    },
                };
                unknown_tags.insert(entry.tag_id, unknown);
            }
        }

        // Read next IFD offset
        let next_ifd_offset = if self.header.is_bigtiff {
            self.read_u64()?
        } else {
            self.read_u32()? as u64
        };

        let mut ifd = Ifd {
            offset,
            entries,
            unknown_tags,
            next_ifd_offset,
            sub_ifds: Vec::new(),
            exif_ifd: None,
        };

        // Parse SubIFDs if present
        if let Some(sub_ifd_entry) = ifd.entries.get(&TiffTag::SubIFDs).cloned() {
            let offsets = self.read_value_as_u64_vec(&sub_ifd_entry)?;
            for sub_offset in offsets {
                if sub_offset != 0 {
                    match self.parse_ifd_at(sub_offset) {
                        Ok(sub_ifd) => ifd.sub_ifds.push(sub_ifd),
                        Err(e) => {
                            log::warn!("Failed to parse SubIFD at offset {}: {}", sub_offset, e);
                        }
                    }
                }
            }
        }

        // Parse EXIF IFD if present
        if let Some(exif_entry) = ifd.entries.get(&TiffTag::ExifIFDPointer).cloned() {
            if let Some(exif_offset) = self.read_value_as_u64(&exif_entry)? {
                if exif_offset != 0 {
                    match self.parse_ifd_at(exif_offset) {
                        Ok(exif_ifd) => ifd.exif_ifd = Some(Box::new(exif_ifd)),
                        Err(e) => {
                            log::warn!("Failed to parse EXIF IFD at offset {}: {}", exif_offset, e);
                        }
                    }
                }
            }
        }

        // Cache the result
        self.ifd_cache.insert(offset, ifd.clone());

        Ok(ifd)
    }

    /// Parse a single IFD entry (12 bytes for TIFF, 20 bytes for BigTIFF).
    fn parse_ifd_entry(&mut self) -> RawResult<IfdEntry> {
        let is_little = matches!(self.header.byte_order, ByteOrder::LittleEndian);

        let (tag_id, data_type, count, value_offset) = if self.header.is_bigtiff {
            let raw: RawBigTiffIfdEntry = self
                .reader
                .read_ne_args::<RawBigTiffIfdEntry>(binrw::args! { is_little })
                .map_err(|e| RawError::ParseError(e.to_string()))?;
            (raw.tag_id, raw.data_type, raw.count, raw.value_offset)
        } else {
            let raw: RawIfdEntry = self
                .reader
                .read_ne_args::<RawIfdEntry>(binrw::args! { is_little })
                .map_err(|e| RawError::ParseError(e.to_string()))?;
            (
                raw.tag_id,
                raw.data_type,
                raw.count as u64,
                raw.value_offset as u64,
            )
        };

        let tiff_type = TiffType::from_u16(data_type);
        let tag = TiffTag::from_u16(tag_id);

        Ok(IfdEntry {
            tag_id,
            data_type,
            count,
            value_offset,
            tiff_type,
            tag,
        })
    }

    /// Walk the IFD chain starting from IFD0.
    pub fn walk_ifd_chain(&mut self) -> RawResult<Vec<Ifd>> {
        self.visited_offsets.clear();
        let mut ifds = Vec::new();
        let mut offset = self.header.ifd0_offset;

        while offset != 0 {
            let ifd = self.parse_ifd_at(offset)?;
            offset = ifd.next_ifd_offset;
            ifds.push(ifd);
        }

        Ok(ifds)
    }

    /// Parse IFD0 (the first/main IFD).
    pub fn parse_ifd0(&mut self) -> RawResult<Ifd> {
        self.visited_offsets.clear();
        self.parse_ifd_at(self.header.ifd0_offset)
    }

    /// Read the value for an IFD entry.
    pub fn read_value(&mut self, entry: &IfdEntry) -> RawResult<TiffValue> {
        let tiff_type = entry
            .tiff_type
            .ok_or(RawError::UnknownDataType(entry.data_type))?;

        // Determine if inline or offset
        let is_inline = entry.is_inline(self.header.is_bigtiff);

        if !is_inline {
            // Seek to the data offset
            self.reader.seek(SeekFrom::Start(entry.value_offset))?;
        }

        // For inline values, we need to handle them from the value_offset bytes
        let count = entry.count as usize;

        match tiff_type {
            TiffType::Byte => {
                let mut data = vec![0u8; count];
                if is_inline {
                    // Extract from value_offset bytes
                    let bytes = entry.value_offset.to_le_bytes();
                    data.copy_from_slice(&bytes[..count.min(8)]);
                } else {
                    self.reader.read_exact(&mut data)?;
                }
                Ok(TiffValue::Bytes(data))
            }
            TiffType::Ascii => {
                let mut data = vec![0u8; count];
                if is_inline {
                    let bytes = entry.value_offset.to_le_bytes();
                    data.copy_from_slice(&bytes[..count.min(8)]);
                } else {
                    self.reader.read_exact(&mut data)?;
                }
                // Remove null terminator and trailing garbage
                let s = String::from_utf8_lossy(&data)
                    .trim_end_matches('\0')
                    .trim()
                    .to_string();
                Ok(TiffValue::Ascii(s))
            }
            TiffType::Short => {
                let mut values = Vec::with_capacity(count);
                if is_inline {
                    let bytes = entry.value_offset.to_le_bytes();
                    for i in 0..count {
                        let idx = i * 2;
                        if idx + 1 < 8 {
                            let v = match self.header.byte_order {
                                ByteOrder::LittleEndian => {
                                    u16::from_le_bytes([bytes[idx], bytes[idx + 1]])
                                }
                                ByteOrder::BigEndian => {
                                    u16::from_be_bytes([bytes[idx], bytes[idx + 1]])
                                }
                            };
                            values.push(v);
                        }
                    }
                } else {
                    for _ in 0..count {
                        values.push(self.read_u16()?);
                    }
                }
                Ok(TiffValue::Shorts(values))
            }
            TiffType::Long | TiffType::Ifd => {
                let mut values = Vec::with_capacity(count);
                if is_inline && count == 1 {
                    values.push(entry.value_offset as u32);
                } else {
                    for _ in 0..count {
                        values.push(self.read_u32()?);
                    }
                }
                Ok(TiffValue::Longs(values))
            }
            TiffType::Rational => {
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    let num = self.read_u32()?;
                    let den = self.read_u32()?;
                    values.push(Rational::new(num, den));
                }
                Ok(TiffValue::Rationals(values))
            }
            TiffType::SByte => {
                let mut data = vec![0u8; count];
                if is_inline {
                    let bytes = entry.value_offset.to_le_bytes();
                    data.copy_from_slice(&bytes[..count.min(8)]);
                } else {
                    self.reader.read_exact(&mut data)?;
                }
                let signed: Vec<i8> = data.into_iter().map(|b| b as i8).collect();
                Ok(TiffValue::SBytes(signed))
            }
            TiffType::Undefined => {
                let mut data = vec![0u8; count];
                if is_inline {
                    let bytes = entry.value_offset.to_le_bytes();
                    data.copy_from_slice(&bytes[..count.min(8)]);
                } else {
                    self.reader.read_exact(&mut data)?;
                }
                Ok(TiffValue::Undefined(data))
            }
            TiffType::SShort => {
                let mut values = Vec::with_capacity(count);
                if is_inline {
                    let bytes = entry.value_offset.to_le_bytes();
                    for i in 0..count {
                        let idx = i * 2;
                        if idx + 1 < 8 {
                            let v = match self.header.byte_order {
                                ByteOrder::LittleEndian => {
                                    i16::from_le_bytes([bytes[idx], bytes[idx + 1]])
                                }
                                ByteOrder::BigEndian => {
                                    i16::from_be_bytes([bytes[idx], bytes[idx + 1]])
                                }
                            };
                            values.push(v);
                        }
                    }
                } else {
                    for _ in 0..count {
                        values.push(self.read_i16()?);
                    }
                }
                Ok(TiffValue::SShorts(values))
            }
            TiffType::SLong => {
                let mut values = Vec::with_capacity(count);
                if is_inline && count == 1 {
                    values.push(entry.value_offset as i32);
                } else {
                    for _ in 0..count {
                        values.push(self.read_i32()?);
                    }
                }
                Ok(TiffValue::SLongs(values))
            }
            TiffType::SRational => {
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    let num = self.read_i32()?;
                    let den = self.read_i32()?;
                    values.push(SRational::new(num, den));
                }
                Ok(TiffValue::SRationals(values))
            }
            TiffType::Float => {
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.read_f32()?);
                }
                Ok(TiffValue::Floats(values))
            }
            TiffType::Double => {
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.read_f64()?);
                }
                Ok(TiffValue::Doubles(values))
            }
            TiffType::Long8 | TiffType::Ifd8 => {
                let mut values = Vec::with_capacity(count);
                if is_inline && count == 1 {
                    values.push(entry.value_offset);
                } else {
                    for _ in 0..count {
                        values.push(self.read_u64()?);
                    }
                }
                Ok(TiffValue::Long8s(values))
            }
            TiffType::SLong8 => {
                let mut values = Vec::with_capacity(count);
                if is_inline && count == 1 {
                    values.push(entry.value_offset as i64);
                } else {
                    for _ in 0..count {
                        values.push(self.read_i64()?);
                    }
                }
                Ok(TiffValue::SLong8s(values))
            }
        }
    }

    /// Read entry value as a single u64 (for offset/pointer tags).
    fn read_value_as_u64(&mut self, entry: &IfdEntry) -> RawResult<Option<u64>> {
        if entry.is_inline(self.header.is_bigtiff) {
            Ok(Some(entry.value_offset))
        } else {
            self.reader.seek(SeekFrom::Start(entry.value_offset))?;
            Ok(Some(self.read_u32()? as u64))
        }
    }

    /// Read entry value as a vector of u64 (for SubIFDs, StripOffsets, etc.).
    fn read_value_as_u64_vec(&mut self, entry: &IfdEntry) -> RawResult<Vec<u64>> {
        let count = entry.count as usize;
        let mut values = Vec::with_capacity(count);

        if entry.is_inline(self.header.is_bigtiff) && count == 1 {
            values.push(entry.value_offset);
        } else {
            self.reader.seek(SeekFrom::Start(entry.value_offset))?;
            for _ in 0..count {
                let v = if self.header.is_bigtiff {
                    self.read_u64()?
                } else {
                    self.read_u32()? as u64
                };
                values.push(v);
            }
        }

        Ok(values)
    }

    // ========================================
    // Raw read helpers (respecting byte order)
    // ========================================

    fn read_u16(&mut self) -> RawResult<u16> {
        Ok(match self.header.byte_order {
            ByteOrder::LittleEndian => self.reader.read_le()?,
            ByteOrder::BigEndian => self.reader.read_be()?,
        })
    }

    fn read_i16(&mut self) -> RawResult<i16> {
        Ok(match self.header.byte_order {
            ByteOrder::LittleEndian => self.reader.read_le()?,
            ByteOrder::BigEndian => self.reader.read_be()?,
        })
    }

    fn read_u32(&mut self) -> RawResult<u32> {
        Ok(match self.header.byte_order {
            ByteOrder::LittleEndian => self.reader.read_le()?,
            ByteOrder::BigEndian => self.reader.read_be()?,
        })
    }

    fn read_i32(&mut self) -> RawResult<i32> {
        Ok(match self.header.byte_order {
            ByteOrder::LittleEndian => self.reader.read_le()?,
            ByteOrder::BigEndian => self.reader.read_be()?,
        })
    }

    fn read_u64(&mut self) -> RawResult<u64> {
        Ok(match self.header.byte_order {
            ByteOrder::LittleEndian => self.reader.read_le()?,
            ByteOrder::BigEndian => self.reader.read_be()?,
        })
    }

    fn read_i64(&mut self) -> RawResult<i64> {
        Ok(match self.header.byte_order {
            ByteOrder::LittleEndian => self.reader.read_le()?,
            ByteOrder::BigEndian => self.reader.read_be()?,
        })
    }

    fn read_f32(&mut self) -> RawResult<f32> {
        Ok(match self.header.byte_order {
            ByteOrder::LittleEndian => self.reader.read_le()?,
            ByteOrder::BigEndian => self.reader.read_be()?,
        })
    }

    fn read_f64(&mut self) -> RawResult<f64> {
        Ok(match self.header.byte_order {
            ByteOrder::LittleEndian => self.reader.read_le()?,
            ByteOrder::BigEndian => self.reader.read_be()?,
        })
    }

    // ========================================
    // Public read helpers for format modules
    // ========================================

    /// Seek to a specific offset in the file.
    pub fn seek_to(&mut self, offset: u64) -> RawResult<()> {
        self.reader.seek(SeekFrom::Start(offset))?;
        Ok(())
    }

    /// Read a specified number of bytes from the current position.
    pub fn read_bytes(&mut self, count: usize) -> RawResult<Vec<u8>> {
        let mut buffer = vec![0u8; count];
        self.reader.read_exact(&mut buffer)?;
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Create a minimal valid TIFF header (little-endian, no IFD entries).
    fn make_minimal_tiff() -> Vec<u8> {
        let mut data = Vec::new();
        // Byte order: II (little-endian)
        data.extend_from_slice(b"II");
        // Magic: 42
        data.extend_from_slice(&42u16.to_le_bytes());
        // IFD0 offset: 8 (right after header)
        data.extend_from_slice(&8u32.to_le_bytes());
        // IFD with 0 entries
        data.extend_from_slice(&0u16.to_le_bytes()); // entry count
        data.extend_from_slice(&0u32.to_le_bytes()); // next IFD offset
        data
    }

    #[test]
    fn test_parse_header_le() {
        let data = make_minimal_tiff();
        let mut cursor = Cursor::new(data);
        let header = TiffHeader::parse(&mut cursor).unwrap();

        assert_eq!(header.byte_order, ByteOrder::LittleEndian);
        assert_eq!(header.magic, 42);
        assert_eq!(header.ifd0_offset, 8);
        assert!(!header.is_bigtiff);
    }

    #[test]
    fn test_parse_header_be() {
        let mut data = Vec::new();
        data.extend_from_slice(b"MM"); // Big-endian
        data.extend_from_slice(&42u16.to_be_bytes());
        data.extend_from_slice(&8u32.to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes()); // entry count
        data.extend_from_slice(&0u32.to_be_bytes()); // next IFD

        let mut cursor = Cursor::new(data);
        let header = TiffHeader::parse(&mut cursor).unwrap();

        assert_eq!(header.byte_order, ByteOrder::BigEndian);
        assert_eq!(header.magic, 42);
    }

    #[test]
    fn test_invalid_byte_order() {
        let mut data = Vec::new();
        data.extend_from_slice(b"XX"); // Invalid
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());

        let mut cursor = Cursor::new(data);
        let result = TiffHeader::parse(&mut cursor);

        assert!(matches!(result, Err(RawError::InvalidByteOrder(_))));
    }

    #[test]
    fn test_invalid_magic() {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&99u16.to_le_bytes()); // Invalid magic
        data.extend_from_slice(&8u32.to_le_bytes());

        let mut cursor = Cursor::new(data);
        let result = TiffHeader::parse(&mut cursor);

        assert!(matches!(result, Err(RawError::InvalidMagic { .. })));
    }

    #[test]
    fn test_parse_empty_ifd() {
        let data = make_minimal_tiff();
        let cursor = Cursor::new(data);
        let mut parser = TiffParser::new(cursor).unwrap();

        let ifd = parser.parse_ifd0().unwrap();
        assert_eq!(ifd.offset, 8);
        assert!(ifd.entries.is_empty());
        assert_eq!(ifd.next_ifd_offset, 0);
    }

    #[test]
    fn test_walk_ifd_chain() {
        let data = make_minimal_tiff();
        let cursor = Cursor::new(data);
        let mut parser = TiffParser::new(cursor).unwrap();

        let ifds = parser.walk_ifd_chain().unwrap();
        assert_eq!(ifds.len(), 1);
    }
}
