//! Pure-Rust WebP decoding, encoding, and metadata support via `gamut-webp`.
#![forbid(unsafe_code)]

use rawshift_image_core::{FormatId, FormatSniffer};
#[cfg(any(feature = "decode", feature = "encode"))]
use rawshift_image_core::{RawError, RawResult, RgbImage};

/// Configuration for the pure-Rust `gamut-webp` decoder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WebpDecodeConfig {}

/// WebP format marker.
pub struct WebP;

impl FormatSniffer for WebP {
    const FORMAT: FormatId = FormatId::WebP;

    fn matches(data: &[u8]) -> bool {
        data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP"
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for WebP {
    type Options = WebpDecodeConfig;
    type Output = RgbImage;

    fn decode(data: &[u8], options: &Self::Options) -> RawResult<Self::Output> {
        decode(data, options)
    }
}

/// Decode a WebP still image into 16-bit RGB samples.
#[cfg(feature = "decode")]
pub fn decode(data: &[u8], _config: &WebpDecodeConfig) -> RawResult<RgbImage> {
    use gamut_core::{DecodeImage, ImageBuf, Rgb8};
    use gamut_webp::WebpDecoder;
    use rawshift_image_core::FormatError;

    let decoded: ImageBuf<Rgb8> = WebpDecoder::new().decode_image(data).map_err(|error| {
        RawError::Format(FormatError::ImageDecode {
            format: "WebP",
            message: error.to_string(),
        })
    })?;
    let dimensions = decoded.dimensions();
    let samples = decoded
        .as_samples()
        .iter()
        .map(|&value| u16::from(value) * 257)
        .collect();
    RgbImage::new(dimensions.width, dimensions.height, samples)
}

#[cfg(feature = "encode")]
impl rawshift_image_core::ImageEncoder for WebP {
    type Options = ();
    type Input = RgbImage;

    fn encode_to_writer<W: std::io::Write>(
        input: &Self::Input,
        metadata: &rawshift_image_core::ImageMetadata,
        _: &Self::Options,
        mut writer: W,
    ) -> RawResult<()> {
        use gamut_core::{Dimensions, EncodeImage, ImageRef, Rgb8};
        use gamut_webp::WebpEncoder;
        use rawshift_image_core::EncodeError;
        use rawshift_image_metadata::{exif::ExifBuilder, icc::IccProfile};

        let encoding_error = |error: gamut_core::Error| {
            RawError::Encode(EncodeError::Encoding {
                format: "WebP",
                message: error.to_string(),
            })
        };
        let dimensions = Dimensions::new(input.width(), input.height()).map_err(encoding_error)?;
        let samples: Vec<u8> = input
            .data()
            .iter()
            .map(|value| (value >> 8) as u8)
            .collect();
        let image = ImageRef::<Rgb8>::new(&samples, dimensions).map_err(encoding_error)?;

        let icc = IccProfile::srgb();
        let mut encoder = WebpEncoder::lossy(75).with_icc_profile(icc.as_bytes());
        if let Ok(exif) = ExifBuilder::new(metadata).build_bytes() {
            encoder = encoder.with_exif(&exif);
        }
        if let Some(xmp) = metadata.xmp.as_deref()
            && gamut_xmp::XmpMeta::from_packet(xmp).is_ok()
        {
            encoder = encoder.with_xmp(xmp);
        }

        let output = encoder.encode_to_vec(image).map_err(encoding_error)?;
        writer.write_all(&output)?;
        Ok(())
    }
}

/// Extract EXIF, ICC, and XMP metadata from a WebP container.
#[cfg(any(feature = "decode", feature = "encode"))]
pub fn read_metadata(data: &[u8]) -> rawshift_image_core::ImageMetadata {
    use gamut_metadata::{Metadata, MetadataBlock};

    let Ok(metadata) = gamut_webp::metadata(data) else {
        return rawshift_image_core::ImageMetadata::default();
    };
    let mut blocks = Vec::with_capacity(3);
    if let Some(exif) = metadata.exif.as_deref() {
        blocks.push(MetadataBlock::Exif(exif));
    }
    if let Some(xmp) = metadata.xmp.as_deref() {
        blocks.push(MetadataBlock::Xmp(xmp));
    }
    if let Some(icc) = metadata.icc.as_deref() {
        blocks.push(MetadataBlock::Icc(icc));
    }
    let Ok(model) = Metadata::from_blocks(&blocks) else {
        return rawshift_image_core::ImageMetadata::default();
    };
    rawshift_image_metadata::bridge::from_gamut(&model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_webp_container() {
        assert!(WebP::matches(b"RIFF\x04\x00\x00\x00WEBP"));
        assert!(!WebP::matches(b"not a WebP file"));
    }

    #[cfg(all(feature = "decode", feature = "encode"))]
    #[test]
    fn encode_decode_round_trip_preserves_dimensions_and_metadata() {
        use rawshift_image_core::{ImageDecoder, ImageEncoder, ImageMetadata};

        let image = RgbImage::new(
            2,
            2,
            vec![
                0xffff, 0, 0, 0, 0xffff, 0, 0, 0, 0xffff, 0xffff, 0xffff, 0xffff,
            ],
        )
        .expect("valid image");
        let mut encoded = Vec::new();
        WebP::encode_to_writer(&image, &ImageMetadata::default(), &(), &mut encoded)
            .expect("encode WebP");

        let decoded = WebP::decode(&encoded, &WebpDecodeConfig::default()).expect("decode WebP");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
        assert_eq!(decoded.data().len(), image.data().len());
        assert!(read_metadata(&encoded).icc_profile.is_some());
    }
}
