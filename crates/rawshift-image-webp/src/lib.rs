//! WebP format support and the isolated libwebp FFI boundary.

#[cfg(any(feature = "decode", feature = "encode"))]
mod ffi;
#[cfg(any(feature = "decode", feature = "encode"))]
pub use ffi::*;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibwebpDecodeConfig {}

pub struct WebP;

impl rawshift_image_core::FormatSniffer for WebP {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::WebP;
    fn matches(data: &[u8]) -> bool {
        data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP"
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for WebP {
    type Options = LibwebpDecodeConfig;
    type Output = rawshift_image_core::RgbImage;
    fn decode(data: &[u8], _: &Self::Options) -> rawshift_image_core::RawResult<Self::Output> {
        let (width, height, samples) = decode_webp_rgb(data).map_err(|message| {
            rawshift_image_core::RawError::Format(rawshift_image_core::FormatError::ImageDecode {
                format: "WebP",
                message,
            })
        })?;
        rawshift_image_core::RgbImage::new(
            width,
            height,
            samples
                .into_iter()
                .map(|value| u16::from(value) * 257)
                .collect(),
        )
    }
}

#[cfg(feature = "encode")]
impl rawshift_image_core::ImageEncoder for WebP {
    type Options = ();
    type Input = rawshift_image_core::RgbImage;
    fn encode_to_writer<W: std::io::Write>(
        input: &Self::Input,
        metadata: &rawshift_image_core::ImageMetadata,
        _: &Self::Options,
        mut writer: W,
    ) -> rawshift_image_core::RawResult<()> {
        use rawshift_image_metadata::{exif::ExifBuilder, icc::IccProfile};
        let map = |message| {
            rawshift_image_core::RawError::Encode(rawshift_image_core::EncodeError::WebP(message))
        };
        let config = build_webp_config(false, 75.0, 4, 100).map_err(map)?;
        let samples: Vec<u8> = input
            .data()
            .iter()
            .map(|value| (value >> 8) as u8)
            .collect();
        let encoded =
            encode_webp_rgb(&samples, input.width(), input.height(), &config).map_err(map)?;
        let exif = ExifBuilder::new(metadata).build_bytes().ok();
        let icc = IccProfile::srgb();
        let output = mux_webp(
            &encoded,
            exif.as_deref(),
            Some(icc.as_bytes()),
            metadata.xmp.as_deref(),
        )
        .map_err(map)?;
        writer.write_all(&output)?;
        Ok(())
    }
}
