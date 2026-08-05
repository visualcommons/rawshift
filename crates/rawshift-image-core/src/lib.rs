//! Shared still-image contracts for rawshift's per-format crates.
#![forbid(unsafe_code)]

pub mod error;
mod rgb_image;

use std::io::Write;

pub use error::{EncodeError, FormatError, ParseError, ProcessingError, RawError, RawResult};
pub use rawshift_core::*;
pub use rgb_image::RgbImage;

/// Stable identity for every image format known to rawshift.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FormatId {
    Gif,
    Jpeg,
    Png,
    WebP,
    Jxl,
    Tiff,
    Avif,
    Heic,
    Svg,
    Apv,
    Ppm,
    Arw,
    Cr2,
    Cr3,
    Crw,
    Dng,
    Nef,
    Raf,
}

/// Header-level facts available without a full image decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageProbe {
    pub format: FormatId,
    pub dimensions: Dimensions,
    pub bit_depth: Option<u8>,
}

/// Compile-time capabilities for one known format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatCapabilities {
    pub format: FormatId,
    pub sniff: bool,
    pub probe: bool,
    pub metadata: bool,
    pub decode: bool,
    pub encode: bool,
    pub decoder: Option<CodecInfo>,
    pub encoder: Option<CodecInfo>,
}

impl FormatCapabilities {
    pub fn unavailable(format: FormatId) -> Self {
        Self {
            format,
            sniff: false,
            probe: false,
            metadata: false,
            decode: false,
            encode: false,
            decoder: None,
            encoder: None,
        }
    }
}

/// Cheap signature detection for one encoded format.
pub trait FormatSniffer {
    const FORMAT: FormatId;
    fn matches(data: &[u8]) -> bool;
}

/// Header probing for one encoded format.
pub trait FormatProber: FormatSniffer {
    fn probe(data: &[u8]) -> RawResult<ImageProbe>;
}

/// Embedded metadata extraction for one encoded format.
pub trait MetadataReader: FormatSniffer {
    fn read_metadata(data: &[u8]) -> RawResult<ImageMetadata>;
}

/// Pixel or sensor decode for one encoded format.
pub trait ImageDecoder: FormatSniffer {
    type Options: Default;
    type Output;

    fn decode(data: &[u8], options: &Self::Options) -> RawResult<Self::Output>;
}

/// Encoding contract shared by all encoding-capable format crates.
pub trait ImageEncoder: FormatSniffer {
    type Options: Default;
    type Input: ?Sized;

    fn encode_to_writer<W: Write>(
        input: &Self::Input,
        metadata: &ImageMetadata,
        options: &Self::Options,
        writer: W,
    ) -> RawResult<()>;

    fn encode_to_vec(
        input: &Self::Input,
        metadata: &ImageMetadata,
        options: &Self::Options,
    ) -> RawResult<Vec<u8>> {
        let mut output = Vec::new();
        Self::encode_to_writer(input, metadata, options, &mut output)?;
        Ok(output)
    }
}
