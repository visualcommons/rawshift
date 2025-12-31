//! Error types for RAW image processing.
//!
//! This module defines comprehensive error types for TIFF parsing,
//! format-specific errors, and I/O errors.

use std::io;
use thiserror::Error;

use crate::tiff::TiffTag;

/// Main error type for the rawshift library.
#[derive(Debug, Error)]
pub enum RawError {
    /// I/O error during file operations.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Invalid TIFF magic number.
    #[error("Invalid TIFF magic number: expected {expected}, found {found}")]
    InvalidMagic {
        /// Expected magic number
        expected: u16,
        /// Actual magic number found
        found: u16,
    },

    /// Invalid byte order marker.
    #[error("Invalid byte order marker: 0x{0:04X} (expected 'II' or 'MM')")]
    InvalidByteOrder(u16),

    /// Unsupported TIFF version.
    #[error("Unsupported TIFF version: {0} (expected 42 for TIFF or 43 for BigTIFF)")]
    UnsupportedTiffVersion(u16),

    /// Invalid or malformed IFD.
    #[error("Invalid IFD at offset {offset}: {reason}")]
    InvalidIfd {
        /// Offset where the IFD was expected
        offset: u64,
        /// Description of what's wrong
        reason: String,
    },

    /// Required tag not found.
    #[error("Required tag not found: {0}")]
    TagNotFound(TiffTag),

    /// Required tag not found (by ID).
    #[error("Required tag not found: 0x{0:04X}")]
    TagIdNotFound(u16),

    /// Invalid tag value.
    #[error("Invalid value for tag {tag}: {reason}")]
    InvalidTagValue {
        /// The tag with the invalid value
        tag: TiffTag,
        /// Description of what's wrong
        reason: String,
    },

    /// Offset exceeds file boundaries.
    #[error("Offset out of bounds: offset {offset} + size {size} exceeds file size {file_size}")]
    OffsetOutOfBounds {
        /// The offset that's out of bounds
        offset: u64,
        /// Size of data being accessed
        size: u64,
        /// Total file size
        file_size: u64,
    },

    /// Unsupported format or feature.
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    /// Unknown TIFF data type.
    #[error("Unknown TIFF data type: {0}")]
    UnknownDataType(u16),

    /// Decompression error.
    #[error("Decompression error: {0}")]
    DecompressionError(String),

    /// Invalid image dimensions.
    #[error("Invalid image dimensions: {width}x{height}")]
    InvalidDimensions {
        /// Image width
        width: u32,
        /// Image height
        height: u32,
    },

    /// Parse error from binrw.
    #[error("Binary parse error: {0}")]
    ParseError(String),

    /// Unexpected end of data.
    #[error("Unexpected end of data at offset {offset}, needed {needed} bytes")]
    UnexpectedEof {
        /// Offset where data ended
        offset: u64,
        /// Number of bytes needed
        needed: usize,
    },

    /// Circular reference detected in IFD chain.
    #[error("Circular reference detected in IFD chain at offset {0}")]
    CircularReference(u64),

    /// Unaccounted data found in file (gaps or trailing bytes).
    #[error("Unaccounted data: {size} bytes at offset {offset}")]
    UnaccountedData {
        /// Offset where unaccounted data starts
        offset: u64,
        /// Size of unaccounted region
        size: u64,
    },

    /// Overlapping data regions detected.
    #[error("Overlapping data regions at offset {offset}")]
    OverlappingData {
        /// Offset where overlap occurs
        offset: u64,
    },

    /// Unknown/unhandled TIFF tag found.
    #[error("Unknown tag 0x{tag_id:04X} at IFD offset {ifd_offset}")]
    UnknownTag {
        /// The unknown tag ID
        tag_id: u16,
        /// Offset of the IFD containing the tag
        ifd_offset: u64,
    },
}

impl From<binrw::Error> for RawError {
    fn from(err: binrw::Error) -> Self {
        RawError::ParseError(err.to_string())
    }
}

/// Result type alias using RawError.
pub type RawResult<T> = Result<T, RawError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = RawError::InvalidMagic {
            expected: 42,
            found: 0,
        };
        let s = format!("{}", err);
        assert!(s.contains("Invalid TIFF magic"));

        let err = RawError::TagNotFound(TiffTag::ImageWidth);
        let s = format!("{}", err);
        assert!(s.contains("ImageWidth"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let raw_err: RawError = io_err.into();
        assert!(matches!(raw_err, RawError::Io(_)));
    }
}
