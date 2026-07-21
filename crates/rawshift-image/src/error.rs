//! Error types for RAW image processing.
//!
//! This module defines comprehensive error types for TIFF parsing,
//! format-specific errors, and I/O errors.
//!
//! Errors are organized into categories:
//! - [`ParseError`] — TIFF/binary parse issues
//! - [`FormatError`] — Format-specific decode failures
//! - [`ProcessingError`] — Demosaic, color, tonemap
//! - [`EncodeError`] — Output encoding
//! - [`RawError::Unsupported`] — Feature not implemented

use std::io;
use thiserror::Error;

use crate::core::BitDepth;

/// Main error type for the rawshift library.
#[derive(Debug, Error)]
pub enum RawError {
    /// I/O error during file operations.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// TIFF/binary parse error.
    #[error(transparent)]
    Parse(#[from] ParseError),

    /// Format-specific decode error.
    #[error(transparent)]
    Format(#[from] FormatError),

    /// Processing pipeline error.
    #[error(transparent)]
    Processing(#[from] ProcessingError),

    /// Output encoding error.
    #[error(transparent)]
    Encode(#[from] EncodeError),

    /// Feature not yet implemented.
    #[error("Unsupported: {0}")]
    Unsupported(String),

    /// Error surfaced by a gamut primitive (buffer/dimension validation,
    /// codec-independent invariants).
    ///
    /// `context` names the rawshift operation that invoked gamut, since the
    /// upstream error alone rarely identifies the call site (structured
    /// diagnostic context upstream is visualcommons/gamut#254).
    #[error("{context}: {source}")]
    Gamut {
        /// The rawshift operation that invoked gamut.
        context: &'static str,
        /// The underlying gamut error.
        #[source]
        source: gamut_core::Error,
    },

    /// Pixel decode was requested for a hardware-decoded codec (HEVC/HEIC,
    /// AV1/AVIF) but no hardware decoder is available — none compiled in
    /// (build without `hw`/`hw-*`), the target has no hardware decode API
    /// (see `docs/SUPPORT.md`), or the runtime probe failed.
    ///
    /// Container parsing, metadata, and auxiliary-image enumeration always
    /// work regardless; only pixel decode fails with this error. Probe
    /// availability up front with `formats::heic_hw_decode_available()`
    /// (requires the `heic` feature).
    #[error("no hardware decoder available for {codec}: {reason}")]
    HwDecoderUnavailable {
        /// The codec that needed a hardware decoder (e.g. `"HEVC"`).
        codec: &'static str,
        /// Why no decoder is available.
        reason: String,
    },
}

impl RawError {
    /// Wrap a gamut error with the rawshift operation it occurred in.
    pub fn gamut(context: &'static str, source: gamut_core::Error) -> Self {
        RawError::Gamut { context, source }
    }
}

/// TIFF and binary parse errors.
#[derive(Debug, Error)]
pub enum ParseError {
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

    /// Invalid or malformed IFD.
    #[error("Invalid IFD at offset {offset}: {reason}")]
    InvalidIfd {
        /// Offset where the IFD was expected
        offset: u64,
        /// Description of what's wrong
        reason: String,
    },

    /// Required tag not found, identified by its raw 16-bit id.
    ///
    /// Used by the gamut-ifd-based decoders, which address tags numerically.
    #[error("Required tag not found: 0x{0:04X}")]
    MissingTag(u16),

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

    /// Unknown TIFF data type.
    #[error("Unknown TIFF data type: {0}")]
    UnknownDataType(u16),

    /// Invalid image dimensions.
    #[error("Invalid image dimensions: {width}x{height}")]
    InvalidDimensions {
        /// Image width
        width: u32,
        /// Image height
        height: u32,
    },

    /// Circular reference detected in IFD chain.
    #[error("Circular reference detected in IFD chain at offset {0}")]
    CircularReference(u64),

    /// Binary parse error (from format-specific binary parsers).
    #[error("Binary parse error: {0}")]
    BinaryParse(String),
}

/// Format-specific decode errors.
#[derive(Debug, Error)]
pub enum FormatError {
    /// Canon CR2 format error.
    #[cfg(feature = "cr2-decode")]
    #[error("CR2 error: {0}")]
    Cr2(String),

    /// Nikon NEF format error.
    #[cfg(feature = "nef-decode")]
    #[error("NEF error: {0}")]
    Nef(String),

    /// Canon CR3/ISOBMFF format error.
    #[cfg(feature = "cr3-decode")]
    #[error("CR3 error: {0}")]
    Cr3(String),

    /// Fujifilm RAF format error.
    #[cfg(feature = "raf-decode")]
    #[error("RAF error: {0}")]
    Raf(String),

    /// Canon CRW/CIFF format error.
    #[cfg(feature = "crw-decode")]
    #[error("CRW error: {0}")]
    Crw(String),

    /// Standard image format decoding error.
    #[error("Image decode error ({format}): {message}")]
    ImageDecode {
        /// Format name (e.g., "JPEG", "PNG")
        format: &'static str,
        /// Error description
        message: String,
    },

    /// Decompression error.
    #[error("Decompression error: {0}")]
    Decompression(String),
}

/// Processing pipeline errors.
#[derive(Debug, Error)]
pub enum ProcessingError {
    /// Demosaicing error.
    #[error("Demosaic error: {0}")]
    Demosaic(String),

    /// Color processing error.
    #[error("Color processing error: {0}")]
    Color(String),
}

/// Output encoding errors.
///
/// `#[non_exhaustive]`: new encoder backends may introduce new error variants
/// without that being a breaking change.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EncodeError {
    /// Generic encoding/export error.
    #[error("Encoding error ({format}): {message}")]
    Encoding {
        /// Format name (e.g., "JPEG", "PNG")
        format: &'static str,
        /// Error description
        message: String,
    },

    /// The selected encoder does not support the requested output bit depth.
    #[error("{format} encoder does not support {requested:?} output")]
    UnsupportedBitDepth {
        /// Format name (e.g., "JPEG", "AVIF")
        format: &'static str,
        /// The bit depth that was requested but is not supported.
        requested: BitDepth,
    },

    /// WebP encoding error.
    #[error("WebP error: {0}")]
    WebP(String),
}

/// Result type alias using RawError.
pub type RawResult<T> = Result<T, RawError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = RawError::Parse(ParseError::InvalidMagic {
            expected: 42,
            found: 0,
        });
        let s = format!("{}", err);
        assert!(s.contains("Invalid TIFF magic"));

        let err = RawError::Parse(ParseError::MissingTag(0x0100));
        let s = format!("{}", err);
        assert!(s.contains("0x0100"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let raw_err: RawError = io_err.into();
        assert!(matches!(raw_err, RawError::Io(_)));
    }

    #[test]
    fn test_parse_error_conversion() {
        let parse_err = ParseError::InvalidByteOrder(0x1234);
        let raw_err: RawError = parse_err.into();
        assert!(matches!(
            raw_err,
            RawError::Parse(ParseError::InvalidByteOrder(0x1234))
        ));
    }

    #[cfg(feature = "cr2-decode")]
    #[test]
    fn test_format_error_conversion() {
        let fmt_err = FormatError::Cr2("test error".to_string());
        let raw_err: RawError = fmt_err.into();
        assert!(matches!(raw_err, RawError::Format(FormatError::Cr2(_))));
    }

    #[test]
    fn test_encode_error_conversion() {
        let enc_err = EncodeError::Encoding {
            format: "PNG",
            message: "test".to_string(),
        };
        let raw_err: RawError = enc_err.into();
        assert!(matches!(
            raw_err,
            RawError::Encode(EncodeError::Encoding { .. })
        ));
    }
}
