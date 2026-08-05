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

#[cfg(feature = "encode")]
impl rawshift_image_core::ImageEncoder for Jxl {
    type Options = ();
    type Input = rawshift_image_core::RgbImage;
    fn encode_to_writer<W: std::io::Write>(
        input: &Self::Input,
        metadata: &rawshift_image_core::ImageMetadata,
        _: &Self::Options,
        mut writer: W,
    ) -> rawshift_image_core::RawResult<()> {
        use gamut_core::{Dimensions, EncodeImage, ImageRef, Rgb16};
        use gamut_jxl::{ColorSpec, Container, JxlEncoder};
        use rawshift_image_metadata::{exif::ExifBuilder, icc::IccProfile};
        let error = |error: gamut_core::Error| {
            rawshift_image_core::RawError::Encode(rawshift_image_core::EncodeError::Encoding {
                format: "JXL",
                message: error.to_string(),
            })
        };
        let dimensions = Dimensions::new(input.width(), input.height()).map_err(error)?;
        let image = ImageRef::<Rgb16>::new(input.data(), dimensions).map_err(error)?;
        let icc = IccProfile::srgb();
        let mut encoder = JxlEncoder::lossless()
            .with_color(ColorSpec::Icc(icc.as_bytes().to_vec()))
            .with_container(Container::IsoBmff);
        if let Ok(exif) = ExifBuilder::new(metadata).build_bytes() {
            encoder = encoder.with_exif(&exif);
        }
        if let Some(xmp) = metadata
            .xmp
            .as_deref()
            .and_then(|value| std::str::from_utf8(value).ok())
        {
            encoder = encoder.with_xmp(xmp);
        }
        let mut output = Vec::new();
        encoder.encode_image(image, &mut output).map_err(error)?;
        writer.write_all(&output)?;
        Ok(())
    }
}
