//! JPEG format support.
#![forbid(unsafe_code)]

#[cfg(feature = "decode")]
use rawshift_image_core::FormatError;
use rawshift_image_core::{FormatId, FormatSniffer};
#[cfg(any(feature = "decode", feature = "encode"))]
use rawshift_image_core::{RawError, RawResult, RgbImage};

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

#[cfg(feature = "encode")]
impl rawshift_image_core::ImageEncoder for Jpeg {
    type Options = ();
    type Input = RgbImage;
    fn encode_to_writer<W: std::io::Write>(
        input: &Self::Input,
        metadata: &rawshift_image_core::ImageMetadata,
        _: &Self::Options,
        mut writer: W,
    ) -> RawResult<()> {
        use gamut_core::{Dimensions, EncodeImage, ImageRef, Rgb8};
        use gamut_jpeg::{ChromaSubsampling, JpegEncoder};
        use rawshift_image_metadata::{exif::ExifBuilder, icc::IccProfile};
        let error = |error: gamut_core::Error| {
            RawError::Encode(rawshift_image_core::EncodeError::Encoding {
                format: "JPEG",
                message: error.to_string(),
            })
        };
        let dimensions = Dimensions::new(input.width(), input.height()).map_err(error)?;
        let samples: Vec<u8> = input
            .data()
            .iter()
            .map(|value| (value >> 8) as u8)
            .collect();
        let image = ImageRef::<Rgb8>::new(&samples, dimensions).map_err(error)?;
        let exif = ExifBuilder::new(metadata).build_bytes().ok();
        let icc = IccProfile::srgb();
        let mut encoder = JpegEncoder::new()
            .with_quality(90)
            .with_subsampling(ChromaSubsampling::Ycbcr420)
            .with_icc_profile(icc.as_bytes());
        if let Some(bytes) = exif.as_deref() {
            encoder = encoder.with_exif(bytes);
        }
        if let Some(xmp) = metadata.xmp.as_deref()
            && gamut_xmp::XmpMeta::from_packet(xmp).is_ok()
        {
            encoder = encoder.with_xmp(xmp);
        }
        let mut output = Vec::new();
        encoder.encode_image(image, &mut output).map_err(error)?;
        writer.write_all(&output)?;
        Ok(())
    }
}
