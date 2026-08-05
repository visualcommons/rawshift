//! JPEG XL format support.
#![forbid(unsafe_code)]

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JxlDecodeConfig {}

pub struct Jxl;

impl rawshift_image_core::FormatSniffer for Jxl {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Jxl;
    fn matches(data: &[u8]) -> bool {
        data.starts_with(&[0xff, 0x0a]) || (data.len() >= 8 && &data[4..8] == b"JXL ")
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Jxl {
    type Options = JxlDecodeConfig;
    type Output = rawshift_image_core::RgbImage;
    fn decode(data: &[u8], _: &Self::Options) -> rawshift_image_core::RawResult<Self::Output> {
        use gamut_core::{DecodeImage, ImageBuf, Rgb16};
        let decoded: ImageBuf<Rgb16> =
            gamut_jxl::JxlDecoder::new()
                .decode_image(data)
                .map_err(|error| {
                    rawshift_image_core::RawError::Format(
                        rawshift_image_core::FormatError::ImageDecode {
                            format: "JXL",
                            message: error.to_string(),
                        },
                    )
                })?;
        let dims = decoded.dimensions();
        rawshift_image_core::RgbImage::new(dims.width, dims.height, decoded.into_samples())
    }
}
