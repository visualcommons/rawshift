//! HEIC and HEIF format support.
#![forbid(unsafe_code)]

#[cfg(feature = "decode")]
mod decoder;
#[cfg(feature = "decode")]
pub use decoder::*;

/// HEIC/HEIF format marker.
pub struct Heic;

impl rawshift_image_core::FormatSniffer for Heic {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Heic;
    fn matches(data: &[u8]) -> bool {
        data.len() >= 12
            && &data[4..8] == b"ftyp"
            && matches!(&data[8..12], b"heic" | b"heis" | b"hevc" | b"hevx")
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Heic {
    type Options = ();
    type Output = rawshift_image_core::RgbImage;
    fn decode(data: &[u8], _: &()) -> rawshift_image_core::RawResult<Self::Output> {
        HeicFile::open(data.to_vec())?.decode_primary()
    }
}
