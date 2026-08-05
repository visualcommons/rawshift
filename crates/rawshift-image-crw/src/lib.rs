//! Canon CRW format support.
#![forbid(unsafe_code)]

#[cfg(feature = "decode")]
mod decoder;
#[cfg(feature = "decode")]
pub use decoder::*;

/// Canon CRW format marker.
pub struct Crw;

#[cfg(feature = "decode")]
impl rawshift_image_core::FormatSniffer for Crw {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Crw;
    fn matches(data: &[u8]) -> bool {
        is_crw(data)
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Crw {
    type Options = ();
    type Output = rawshift_image_core::RawImage;
    fn decode(data: &[u8], _: &()) -> rawshift_image_core::RawResult<Self::Output> {
        CrwFile::parse(std::io::Cursor::new(data))?.decode_raw()
    }
}
