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

#[cfg(feature = "tiff-parser")]
use crate::tiff::TiffTag;

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

    /// Required tag not found.
    #[cfg(feature = "tiff-parser")]
    #[error("Required tag not found: {0}")]
    TagNotFound(TiffTag),

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

    /// Binary parse error (from binrw or other parsers).
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
#[derive(Debug, Error)]
pub enum EncodeError {
    /// Generic encoding/export error.
    #[error("Encoding error ({format}): {message}")]
    Encoding {
        /// Format name (e.g., "JPEG", "PNG")
        format: &'static str,
        /// Error description
        message: String,
    },

    /// JPEG encoding error.
    #[cfg(feature = "jpeg-encode")]
    #[error("JPEG encoding error: {0}")]
    Jpeg(#[from] jpeg_encoder::EncodingError),

    /// WebP encoding error.
    #[error("WebP error: {0}")]
    WebP(String),
}

#[cfg(feature = "tiff-parser")]
impl From<binrw::Error> for RawError {
    fn from(err: binrw::Error) -> Self {
        RawError::Parse(ParseError::BinaryParse(err.to_string()))
    }
}

#[cfg(feature = "jpeg-encode")]
impl From<jpeg_encoder::EncodingError> for RawError {
    fn from(err: jpeg_encoder::EncodingError) -> Self {
        RawError::Encode(EncodeError::Jpeg(err))
    }
}

impl From<zune_image::errors::ImageErrors> for RawError {
    fn from(err: zune_image::errors::ImageErrors) -> Self {
        use zune_image::errors::ImageErrors;
        match err {
            // I/O Errors
            ImageErrors::IoError(e) => RawError::Io(e),

            // Dimension Mismatches
            ImageErrors::DimensionsMisMatch(w, h) => {
                RawError::Parse(ParseError::InvalidDimensions {
                    width: w as u32,
                    height: h as u32,
                })
            }

            // Decoding/Decompression Errors
            ImageErrors::ImageDecodeErrors(desc) => {
                RawError::Format(FormatError::Decompression(desc))
            }

            // Unsupported Formats & Features
            ImageErrors::UnsupportedColorspace(cs, source, supported) => {
                RawError::Unsupported(format!(
                    "Colorspace {:?} in {}. Supported: {:?}",
                    cs, source, supported
                ))
            }
            ImageErrors::ImageDecoderNotIncluded(fmt) => {
                RawError::Unsupported(format!("Decoder for {:?} not included", fmt))
            }
            ImageErrors::ImageDecoderNotImplemented(fmt) => {
                RawError::Unsupported(format!("Decoder for {:?} not implemented", fmt))
            }
            ImageErrors::ImageOperationNotImplemented(op, bit) => {
                RawError::Unsupported(format!("Operation {} not implemented for {:?}", op, bit))
            }

            // State and Logic Errors
            ImageErrors::NoImageForOperations => RawError::Parse(ParseError::BinaryParse(
                "No image available for operations".to_string(),
            )),
            ImageErrors::NoImageForEncoding => RawError::Parse(ParseError::BinaryParse(
                "No image available for encoding".to_string(),
            )),
            ImageErrors::NoImageBuffer => RawError::Parse(ParseError::BinaryParse(
                "No image buffer available".to_string(),
            )),
            ImageErrors::WrongTypeId(expected, found) => RawError::Parse(ParseError::BinaryParse(
                format!("Type mismatch: expected {:?}, found {:?}", expected, found),
            )),

            // Generic String/Str Errors
            ImageErrors::GenericString(s) => RawError::Parse(ParseError::BinaryParse(s)),
            ImageErrors::GenericStr(s) => RawError::Parse(ParseError::BinaryParse(s.to_string())),

            // Nested Error Enums
            ImageErrors::OperationsError(e) => {
                RawError::Processing(ProcessingError::Color(format!("Operations error: {:?}", e)))
            }
            ImageErrors::EncodeErrors(e) => RawError::Encode(EncodeError::Encoding {
                format: "zune",
                message: format!("{:?}", e),
            }),
            ImageErrors::ChannelErrors(e) => {
                RawError::Parse(ParseError::BinaryParse(format!("Channel error: {:?}", e)))
            }
        }
    }
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

        #[cfg(feature = "tiff-parser")]
        {
            let err = RawError::Parse(ParseError::TagNotFound(crate::tiff::TiffTag::ImageWidth));
            let s = format!("{}", err);
            assert!(s.contains("ImageWidth"));
        }
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
