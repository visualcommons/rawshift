//! Fujifilm RAF format support.
#![forbid(unsafe_code)]

#[cfg(feature = "decode")]
mod decoder;
#[cfg(feature = "decode")]
pub use decoder::*;

/// Fujifilm RAF format marker.
pub struct Raf;

#[cfg(feature = "decode")]
impl rawshift_image_core::FormatSniffer for Raf {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Raf;
    fn matches(data: &[u8]) -> bool {
        is_raf(data)
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Raf {
    type Options = ();
    type Output = rawshift_image_core::RawImage;
    fn decode(data: &[u8], _: &()) -> rawshift_image_core::RawResult<Self::Output> {
        RafFile::parse(std::io::Cursor::new(data))?.decode_raw()
    }
}
