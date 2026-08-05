//! PNG format support.
#![forbid(unsafe_code)]

#[cfg(feature = "decode")]
use rawshift_image_core::FormatError;
use rawshift_image_core::{FormatId, FormatSniffer};
#[cfg(any(feature = "decode", feature = "encode"))]
use rawshift_image_core::{RawError, RawResult, RgbImage};

/// Hostile-input resource limits for PNG decoding.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PngDecodeConfig {
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub max_image_bytes: Option<usize>,
    pub max_metadata_bytes: Option<usize>,
}

/// PNG format marker.
pub struct Png;

impl FormatSniffer for Png {
    const FORMAT: FormatId = FormatId::Png;
    fn matches(data: &[u8]) -> bool {
        data.starts_with(b"\x89PNG\r\n\x1a\n")
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Png {
    type Options = PngDecodeConfig;
    type Output = RgbImage;
    fn decode(data: &[u8], options: &Self::Options) -> RawResult<Self::Output> {
        decode(data, options)
    }
}

#[cfg(feature = "decode")]
pub fn decode(data: &[u8], config: &PngDecodeConfig) -> RawResult<RgbImage> {
    use gamut_png::PngImage;
    let decoded = decoder(config).decode(data).map_err(|error| {
        RawError::Format(FormatError::ImageDecode {
            format: "PNG",
            message: error.to_string(),
        })
    })?;
    let (width, height) = (decoded.header.width, decoded.header.height);
    let samples = match decoded.image {
        PngImage::Gray8(image) => image
            .as_samples()
            .iter()
            .flat_map(|&v| [scale(v); 3])
            .collect(),
        PngImage::Gray16(image) => image.as_samples().iter().flat_map(|&v| [v; 3]).collect(),
        PngImage::GrayAlpha8(image) => image
            .as_samples()
            .chunks_exact(2)
            .flat_map(|px| [scale(px[0]); 3])
            .collect(),
        PngImage::GrayAlpha16(image) => image
            .as_samples()
            .chunks_exact(2)
            .flat_map(|px| [px[0]; 3])
            .collect(),
        PngImage::Rgb8(image) => image.as_samples().iter().map(|&v| scale(v)).collect(),
        PngImage::Rgb16(image) => image.into_samples(),
        PngImage::Rgba8(image) => image
            .as_samples()
            .chunks_exact(4)
            .flat_map(|px| [scale(px[0]), scale(px[1]), scale(px[2])])
            .collect(),
        PngImage::Rgba16(image) => image
            .as_samples()
            .chunks_exact(4)
            .flat_map(|px| [px[0], px[1], px[2]])
            .collect(),
        PngImage::Indexed8(image) => {
            let palette = decoded.palette.ok_or_else(|| {
                RawError::Format(FormatError::ImageDecode {
                    format: "PNG",
                    message: "indexed PNG without a palette".to_owned(),
                })
            })?;
            image
                .as_samples()
                .iter()
                .flat_map(|&index| {
                    let [r, g, b] = palette.rgb(index).unwrap_or_default();
                    [scale(r), scale(g), scale(b)]
                })
                .collect()
        }
    };
    RgbImage::new(width, height, samples)
}

#[cfg(feature = "decode")]
fn decoder(config: &PngDecodeConfig) -> gamut_png::PngDecoder {
    const SPEC_MAX: u32 = i32::MAX as u32;
    let mut decoder = gamut_png::PngDecoder::new();
    if config.max_width.is_some() || config.max_height.is_some() {
        decoder = decoder.with_max_dimensions(
            config.max_width.unwrap_or(SPEC_MAX),
            config.max_height.unwrap_or(SPEC_MAX),
        );
    }
    if let Some(bytes) = config.max_image_bytes {
        decoder = decoder.with_max_image_bytes(bytes);
    }
    if let Some(bytes) = config.max_metadata_bytes {
        decoder = decoder.with_max_metadata_bytes(bytes);
    }
    decoder
}

#[cfg(feature = "decode")]
fn scale(value: u8) -> u16 {
    u16::from(value) * 257
}

#[cfg(feature = "encode")]
impl rawshift_image_core::ImageEncoder for Png {
    type Options = ();
    type Input = RgbImage;
    fn encode_to_writer<W: std::io::Write>(
        input: &Self::Input,
        metadata: &rawshift_image_core::ImageMetadata,
        _: &Self::Options,
        mut writer: W,
    ) -> RawResult<()> {
        use gamut_core::{Dimensions, EncodeImage, ImageRef, Rgb16};
        use rawshift_image_metadata::{exif::ExifBuilder, icc::IccProfile};
        let error = |error: gamut_core::Error| {
            RawError::Encode(rawshift_image_core::EncodeError::Encoding {
                format: "PNG",
                message: error.to_string(),
            })
        };
        let dimensions = Dimensions::new(input.width(), input.height()).map_err(error)?;
        let image = ImageRef::<Rgb16>::new(input.data(), dimensions).map_err(error)?;
        let icc = IccProfile::srgb();
        let mut encoder =
            gamut_png::PngEncoder::new().with_icc_profile("ICC Profile", icc.as_bytes());
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
