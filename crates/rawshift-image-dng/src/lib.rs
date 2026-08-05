//! Adobe DNG format support.
#![forbid(unsafe_code)]

#[cfg(feature = "decode")]
mod decoder;
#[cfg(feature = "decode")]
pub use decoder::*;
#[cfg(feature = "encode")]
mod encode;
#[cfg(feature = "encode")]
pub use encode::*;
#[cfg(feature = "decode")]
pub mod opcodes;

/// Adobe DNG format marker.
pub struct Dng;

impl rawshift_image_core::FormatSniffer for Dng {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Dng;
    fn matches(data: &[u8]) -> bool {
        #[cfg(feature = "decode")]
        {
            use gamut_ifd::{IfdReader, StreamSource};
            let mut cursor = std::io::Cursor::new(data);
            let Ok(mut reader) = IfdReader::open(StreamSource::new(&mut cursor)) else {
                return false;
            };
            let Ok(ifd) = reader.read_ifd(reader.first_ifd_offset()) else {
                return false;
            };
            return ifd.entry(rawshift_image_ifd::tags::DNG_VERSION).is_some();
        }
        #[cfg(not(feature = "decode"))]
        {
            let tiff = data.starts_with(b"II\x2a\0") || data.starts_with(b"MM\0\x2a");
            tiff && data.get(..data.len().min(4096)).is_some_and(|header| {
                header
                    .windows(2)
                    .any(|bytes| bytes == [0xc6, 0x12] || bytes == [0x12, 0xc6])
            })
        }
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Dng {
    type Options = ();
    type Output = rawshift_image_core::RawImage;
    fn decode(data: &[u8], _: &()) -> rawshift_image_core::RawResult<Self::Output> {
        DngFile::parse(std::io::Cursor::new(data))?.decode_raw()
    }
}

#[cfg(feature = "encode")]
impl rawshift_image_core::ImageEncoder for Dng {
    type Options = DngEncodeConfig;
    type Input = rawshift_image_core::RgbImage;
    fn encode_to_writer<W: std::io::Write>(
        input: &Self::Input,
        metadata: &rawshift_image_core::ImageMetadata,
        options: &Self::Options,
        writer: W,
    ) -> rawshift_image_core::RawResult<()> {
        export_dng_to_writer(writer, input, metadata, options)
    }
}
