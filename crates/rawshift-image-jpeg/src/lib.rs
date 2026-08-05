//! JPEG format support.
#![forbid(unsafe_code)]

use rawshift_image_core::{FormatError, FormatId, FormatSniffer, RawError, RawResult, RgbImage};

/// JPEG decoder configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JpegDecodeConfig {}

/// JPEG format marker.
pub struct Jpeg;

impl FormatSniffer for Jpeg {
    const FORMAT: FormatId = FormatId::Jpeg;
    fn matches(data: &[u8]) -> bool {
        data.starts_with(&[0xff, 0xd8, 0xff])
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Jpeg {
    type Options = JpegDecodeConfig;
    type Output = RgbImage;
    fn decode(data: &[u8], options: &Self::Options) -> RawResult<Self::Output> {
        decode(data, options)
    }
}

#[cfg(feature = "decode")]
pub fn decode(data: &[u8], _config: &JpegDecodeConfig) -> RawResult<RgbImage> {
    use gamut_core::{Cmyk8, DecodeImage, ImageBuf, Rgb8};
    use gamut_jpeg::JpegDecoder;

    let jpeg_err = |error: gamut_core::Error| {
        RawError::Format(FormatError::ImageDecode {
            format: "JPEG",
            message: error.to_string(),
        })
    };
    let info = gamut_jpeg::info(data).map_err(jpeg_err)?;
    if info.components == 4 {
        let decoded: ImageBuf<Cmyk8> = JpegDecoder::new().decode_image(data).map_err(jpeg_err)?;
        let dims = decoded.dimensions();
        let samples = decoded
            .as_samples()
            .chunks_exact(4)
            .flat_map(|px| {
                [
                    scale(blinn(px[0], px[3])),
                    scale(blinn(px[1], px[3])),
                    scale(blinn(px[2], px[3])),
                ]
            })
            .collect();
        return RgbImage::new(dims.width, dims.height, samples);
    }
    let decoded: ImageBuf<Rgb8> = JpegDecoder::new().decode_image(data).map_err(jpeg_err)?;
    let dims = decoded.dimensions();
    RgbImage::new(
        dims.width,
        dims.height,
        decoded.as_samples().iter().map(|&v| scale(v)).collect(),
    )
}

#[cfg(feature = "decode")]
fn blinn(value: u8, factor: u8) -> u8 {
    let product = i32::from(value) * i32::from(factor) + 128;
    ((product + (product >> 8)) >> 8) as u8
}

#[cfg(feature = "decode")]
fn scale(value: u8) -> u16 {
    u16::from(value) * 257
}
