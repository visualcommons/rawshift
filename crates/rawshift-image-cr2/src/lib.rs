//! Canon CR2 format support.
#![forbid(unsafe_code)]

#[cfg(feature = "decode")]
mod decoder;
#[cfg(feature = "decode")]
pub use decoder::*;

/// Canon CR2 format marker.
pub struct Cr2;

#[cfg(feature = "decode")]
impl rawshift_image_core::FormatSniffer for Cr2 {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Cr2;
    fn matches(data: &[u8]) -> bool {
        is_cr2(data)
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Cr2 {
    type Options = ();
    type Output = rawshift_image_core::RawImage;
    fn decode(data: &[u8], _: &()) -> rawshift_image_core::RawResult<Self::Output> {
        Cr2File::parse(std::io::Cursor::new(data))?.decode_raw()
    }
}
