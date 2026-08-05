//! AVIF format support.
#![forbid(unsafe_code)]

#[cfg(feature = "decode")]
mod decoder;
#[cfg(feature = "decode")]
pub use decoder::*;

/// AVIF format marker.
pub struct Avif;

impl rawshift_image_core::FormatSniffer for Avif {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Avif;
    fn matches(data: &[u8]) -> bool {
        data.len() >= 12
            && &data[4..8] == b"ftyp"
            && matches!(&data[8..12], b"avif" | b"avis" | b"mif1")
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Avif {
    type Options = ();
    type Output = rawshift_image_core::RgbImage;
    fn decode(data: &[u8], _: &()) -> rawshift_image_core::RawResult<Self::Output> {
        AvifFile::open(data.to_vec())?.decode_primary()
    }
}
