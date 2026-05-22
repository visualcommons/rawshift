//! TIFF/DNG file writer.
//!
//! This module provides low-level TIFF writing functionality for creating
//! DNG files with embedded metadata.

use binrw::{BinWrite, BinWriterExt, Endian};
use std::io::{Seek, SeekFrom, Write};

use crate::error::RawResult;
use crate::tiff::TiffTag;
use crate::tiff::types::{ByteOrder, TiffType};

/// A single IFD entry to be written.
#[derive(Debug, Clone)]
pub struct IfdEntry {
    /// Tag ID
    pub tag: u16,
    /// Data type
    pub typ: TiffType,
    /// Number of values
    pub count: u32,
    /// Raw value bytes (in file byte order)
    pub value_bytes: Vec<u8>,
}

/// On-disk representation of an IFD entry (12 bytes).
#[derive(Debug, Clone, BinWrite)]
pub struct RawIfdEntry {
    pub tag: u16,
    pub typ: u16,
    pub count: u32,
    pub value_offset: [u8; 4],
}

impl IfdEntry {
    /// Create a SHORT (u16) entry.
    pub fn short(tag: TiffTag, value: u16) -> Self {
        Self {
            tag: tag.as_u16(),
            typ: TiffType::Short,
            count: 1,
            value_bytes: value.to_le_bytes().to_vec(),
        }
    }

    /// Create an array of SHORTs.
    pub fn shorts(tag: TiffTag, values: &[u16]) -> Self {
        let mut bytes = Vec::with_capacity(values.len() * 2);
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        Self {
            tag: tag.as_u16(),
            typ: TiffType::Short,
            count: values.len() as u32,
            value_bytes: bytes,
        }
    }

    /// Create a LONG (u32) entry.
    pub fn long(tag: TiffTag, value: u32) -> Self {
        Self {
            tag: tag.as_u16(),
            typ: TiffType::Long,
            count: 1,
            value_bytes: value.to_le_bytes().to_vec(),
        }
    }

    /// Create an array of LONGs.
    pub fn longs(tag: TiffTag, values: &[u32]) -> Self {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        Self {
            tag: tag.as_u16(),
            typ: TiffType::Long,
            count: values.len() as u32,
            value_bytes: bytes,
        }
    }

    /// Create a RATIONAL (unsigned) entry.
    pub fn rational(tag: TiffTag, num: u32, den: u32) -> Self {
        let mut bytes = Vec::with_capacity(8);
        bytes.extend_from_slice(&num.to_le_bytes());
        bytes.extend_from_slice(&den.to_le_bytes());
        Self {
            tag: tag.as_u16(),
            typ: TiffType::Rational,
            count: 1,
            value_bytes: bytes,
        }
    }

    /// Create an array of RATIONALs.
    pub fn rationals(tag: TiffTag, values: &[(u32, u32)]) -> Self {
        let mut bytes = Vec::with_capacity(values.len() * 8);
        for (num, den) in values {
            bytes.extend_from_slice(&num.to_le_bytes());
            bytes.extend_from_slice(&den.to_le_bytes());
        }
        Self {
            tag: tag.as_u16(),
            typ: TiffType::Rational,
            count: values.len() as u32,
            value_bytes: bytes,
        }
    }

    /// Create an SRATIONAL (signed) entry.
    pub fn srational(tag: TiffTag, num: i32, den: i32) -> Self {
        let mut bytes = Vec::with_capacity(8);
        bytes.extend_from_slice(&num.to_le_bytes());
        bytes.extend_from_slice(&den.to_le_bytes());
        Self {
            tag: tag.as_u16(),
            typ: TiffType::SRational,
            count: 1,
            value_bytes: bytes,
        }
    }

    /// Create an array of SRATIONALs.
    pub fn srationals(tag: TiffTag, values: &[(i32, i32)]) -> Self {
        let mut bytes = Vec::with_capacity(values.len() * 8);
        for (num, den) in values {
            bytes.extend_from_slice(&num.to_le_bytes());
            bytes.extend_from_slice(&den.to_le_bytes());
        }
        Self {
            tag: tag.as_u16(),
            typ: TiffType::SRational,
            count: values.len() as u32,
            value_bytes: bytes,
        }
    }

    /// Create an ASCII string entry (null-terminated).
    pub fn ascii(tag: TiffTag, s: &str) -> Self {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0); // Null terminator
        Self {
            tag: tag.as_u16(),
            typ: TiffType::Ascii,
            count: bytes.len() as u32,
            value_bytes: bytes,
        }
    }

    /// Create a BYTE array entry.
    pub fn bytes(tag: TiffTag, data: &[u8]) -> Self {
        Self {
            tag: tag.as_u16(),
            typ: TiffType::Byte,
            count: data.len() as u32,
            value_bytes: data.to_vec(),
        }
    }

    /// Create an UNDEFINED byte array entry.
    pub fn undefined(tag: TiffTag, data: &[u8]) -> Self {
        Self {
            tag: tag.as_u16(),
            typ: TiffType::Undefined,
            count: data.len() as u32,
            value_bytes: data.to_vec(),
        }
    }

    /// Create a DOUBLE array entry.
    pub fn doubles(tag: TiffTag, values: &[f64]) -> Self {
        let mut bytes = Vec::with_capacity(values.len() * 8);
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        Self {
            tag: tag.as_u16(),
            typ: TiffType::Double,
            count: values.len() as u32,
            value_bytes: bytes,
        }
    }

    /// Size of the value in bytes.
    fn value_size(&self) -> usize {
        self.value_bytes.len()
    }

    /// Whether the value fits inline in the 4-byte value/offset field.
    fn fits_inline(&self) -> bool {
        self.value_size() <= 4
    }
}

/// Low-level TIFF file writer.
pub struct TiffWriter<W> {
    writer: W,
    byte_order: ByteOrder,
    position: u64,
}

impl<W: Write + Seek> TiffWriter<W> {
    /// Create a new TIFF writer (does not write header yet).
    pub fn new(writer: W, byte_order: ByteOrder) -> Self {
        Self {
            writer,
            byte_order,
            position: 0,
        }
    }

    /// Convert internal ByteOrder to binrw Endian
    fn endian(&self) -> Endian {
        match self.byte_order {
            ByteOrder::LittleEndian => Endian::Little,
            ByteOrder::BigEndian => Endian::Big,
        }
    }

    /// Write the TIFF header. Returns the offset where IFD0 pointer should go.
    pub fn write_header(&mut self) -> RawResult<()> {
        let endian = self.endian();

        // Byte order marker
        self.writer.write_type(&self.byte_order, endian)?;
        self.position += 2;

        // Magic number (42 for standard TIFF)
        self.writer.write_type(&42u16, endian)?;
        self.position += 2;

        // IFD0 offset placeholder (will be updated later)
        // For now, write 8 (immediately after header)
        self.writer.write_type(&8u32, endian)?;
        self.position += 4;

        Ok(())
    }

    /// Write an IFD at the current position.
    /// Returns the offset of the IFD.
    /// `next_ifd_offset` is the offset to the next IFD (0 if none).
    pub fn write_ifd(&mut self, entries: &mut [IfdEntry], next_ifd_offset: u32) -> RawResult<u64> {
        let endian = self.endian();

        // Sort entries by tag number (TIFF spec requirement)
        entries.sort_by_key(|e| e.tag);

        let ifd_start = self.position;
        let entry_count = entries.len() as u16;

        // Write entry count
        self.writer.write_type(&entry_count, endian)?;
        self.position += 2;

        // Calculate where overflow data will start
        // IFD: 2 (count) + 12*entries + 4 (next IFD pointer)
        let overflow_start = ifd_start + 2 + (12 * entries.len() as u64) + 4;
        let mut overflow_offset = overflow_start;

        // First pass: calculate overflow offsets
        let mut offsets: Vec<Option<u64>> = Vec::with_capacity(entries.len());
        for entry in entries.iter() {
            if entry.fits_inline() {
                offsets.push(None);
            } else {
                offsets.push(Some(overflow_offset));
                overflow_offset += entry.value_size() as u64;
                // Pad to word boundary
                if !overflow_offset.is_multiple_of(2) {
                    overflow_offset += 1;
                }
            }
        }

        // Write entries
        for (entry, offset) in entries.iter().zip(offsets.iter()) {
            // field calculation
            let val_field: [u8; 4] = if let Some(off) = offset {
                // It is an offset. Write as u32 in correct endianness.
                let off_u32 = *off as u32;
                match endian {
                    Endian::Little => off_u32.to_le_bytes(),
                    Endian::Big => off_u32.to_be_bytes(),
                }
            } else {
                // It is inline data.
                // Data in `entry.value_bytes` is already LE (from `IfdEntry` methods).

                // If we are writing LE file:
                // We want `entry.value_bytes` padded to 4 bytes.
                // `IfdEntry::short(5)` -> `[05, 00]`.
                // Pad -> `[05, 00, 00, 00]`.

                // If we are writing BE file:
                // `IfdEntry` logic is flawed for BE currently.
                // If we assume `entry.value_bytes` needs to be BE for BE file.
                // But `IfdEntry` structs are hardcoded to `to_le_bytes`.
                //
                // Let's assume for this specific refactor step (preserving behavior) that we just write `value_bytes` padded.
                // The previous code did:
                // `inline[..len].copy_from_slice(&entry.value_bytes[..len]); self.write_bytes(&inline)?;`
                // It wrote `value_bytes` directly.
                // So we should replicate that.

                let mut inline = [0u8; 4];
                let len = entry.value_bytes.len().min(4);
                inline[..len].copy_from_slice(&entry.value_bytes[..len]);
                inline
            };

            let raw_entry = RawIfdEntry {
                tag: entry.tag,
                typ: entry.typ as u16,
                count: entry.count,
                value_offset: val_field,
            };

            self.writer.write_type(&raw_entry, endian)?;
            self.position += 12;
        }

        // Write next IFD offset
        self.writer.write_type(&next_ifd_offset, endian)?;
        self.position += 4;

        // Write overflow data
        for (entry, offset) in entries.iter().zip(offsets.iter()) {
            if offset.is_some() {
                self.write_bytes(&entry.value_bytes)?;
                // Pad to word boundary
                if entry.value_bytes.len() % 2 != 0 {
                    self.write_bytes(&[0])?;
                }
            }
        }

        Ok(ifd_start)
    }

    /// Write 16-bit RGB image data as a single strip.
    /// Returns (strip_offset, strip_byte_count).
    pub fn write_image_strip_rgb16(&mut self, data: &[u16]) -> RawResult<(u64, u64)> {
        let offset = self.position;
        let endian = self.endian();

        // Write pixel data element by element
        // Ideally block writing, but data is &[u16] and need swapping.
        // We can create a buffer or just loop. Loop is fine for now (buffered writer usually wraps this).
        for &pixel in data {
            self.writer.write_type(&pixel, endian)?;
        }
        self.position += data.len() as u64 * 2;

        let byte_count = data.len() as u64 * 2;
        Ok((offset, byte_count))
    }

    /// Get the current write position.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Seek to a specific position.
    pub fn seek_to(&mut self, offset: u64) -> RawResult<()> {
        self.writer.seek(SeekFrom::Start(offset))?;
        self.position = offset;
        Ok(())
    }

    /// Update the IFD0 offset in the header (at byte 4).
    pub fn update_ifd0_offset(&mut self, offset: u32) -> RawResult<()> {
        let current = self.position;
        self.seek_to(4)?;
        let endian = self.endian();
        self.writer.write_type(&offset, endian)?;
        self.seek_to(current)?;
        Ok(())
    }

    /// Finish writing and return the inner writer.
    pub fn finish(self) -> W {
        self.writer
    }

    // --- Helper methods ---

    fn write_bytes(&mut self, data: &[u8]) -> RawResult<()> {
        self.writer.write_all(data)?;
        self.position += data.len() as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_ifd_entry_short() {
        let entry = IfdEntry::short(TiffTag::ImageWidth, 1024);
        assert_eq!(entry.tag, 0x0100);
        assert_eq!(entry.count, 1);
        assert!(entry.fits_inline());
    }

    #[test]
    fn test_ifd_entry_ascii() {
        let entry = IfdEntry::ascii(TiffTag::Make, "SONY");
        assert_eq!(entry.tag, 0x010F);
        assert_eq!(entry.count, 5); // "SONY" + null
        assert!(!entry.fits_inline()); // 5 bytes > 4
    }

    #[test]
    fn test_tiff_writer_header() {
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = TiffWriter::new(&mut buffer, ByteOrder::LittleEndian);
        writer.write_header().unwrap();

        let data = buffer.into_inner();
        assert_eq!(&data[0..2], b"II"); // Little-endian marker
        assert_eq!(&data[2..4], &[42, 0]); // Magic 42
        assert_eq!(&data[4..8], &[8, 0, 0, 0]); // IFD0 at offset 8
    }

    #[test]
    fn test_write_ifd() {
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = TiffWriter::new(&mut buffer, ByteOrder::LittleEndian);
        writer.write_header().unwrap();

        let mut entries = vec![
            IfdEntry::short(TiffTag::ImageWidth, 100),
            IfdEntry::short(TiffTag::ImageLength, 50),
        ];
        let ifd_offset = writer.write_ifd(&mut entries, 0).unwrap();
        assert_eq!(ifd_offset, 8);
    }
}
