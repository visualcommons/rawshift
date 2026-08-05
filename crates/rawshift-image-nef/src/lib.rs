//! Nikon NEF format support.
#![forbid(unsafe_code)]

#[cfg(feature = "decode")]
mod decoder;
#[cfg(feature = "decode")]
pub use decoder::*;

/// Nikon NEF format marker.
pub struct Nef;

#[cfg(feature = "decode")]
impl rawshift_image_core::FormatSniffer for Nef {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Nef;
    fn matches(data: &[u8]) -> bool {
        tiff_make(data).is_some_and(|make| make.to_ascii_lowercase().contains("nikon"))
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Nef {
    type Options = ();
    type Output = rawshift_image_core::RawImage;
    fn decode(data: &[u8], _: &()) -> rawshift_image_core::RawResult<Self::Output> {
        NefFile::parse(std::io::Cursor::new(data))?.decode_raw()
    }
}

#[cfg(feature = "decode")]
fn tiff_make(data: &[u8]) -> Option<String> {
    use gamut_ifd::{IfdReader, StreamSource};
    let mut cursor = std::io::Cursor::new(data);
    let mut reader = IfdReader::open(StreamSource::new(&mut cursor)).ok()?;
    let ifd = reader.read_ifd(reader.first_ifd_offset()).ok()?;
    let entry = ifd.entry(rawshift_image_ifd::tags::MAKE)?;
    reader.value(entry).ok()?.as_str().map(str::to_owned)
}
