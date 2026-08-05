//! AVIF format support.
#![forbid(unsafe_code)]

#[cfg(feature = "decode")]
mod decoder;
#[cfg(feature = "decode")]
pub use decoder::*;

/// AVIF format marker.
pub struct Avif;

impl rawshift_image_core::FormatSniffer for Avif {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Avif;
    fn matches(data: &[u8]) -> bool {
        data.len() >= 12
            && &data[4..8] == b"ftyp"
            && matches!(&data[8..12], b"avif" | b"avis" | b"mif1")
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Avif {
    type Options = ();
    type Output = rawshift_image_core::RgbImage;
    fn decode(data: &[u8], _: &()) -> rawshift_image_core::RawResult<Self::Output> {
        AvifFile::open(data.to_vec())?.decode_primary()
    }
}

#[cfg(feature = "encode")]
impl rawshift_image_core::ImageEncoder for Avif {
    type Options = ();
    type Input = rawshift_image_core::RgbImage;
    fn encode_to_writer<W: std::io::Write>(
        input: &Self::Input,
        metadata: &rawshift_image_core::ImageMetadata,
        _: &Self::Options,
        mut writer: W,
    ) -> rawshift_image_core::RawResult<()> {
        use gamut_core::{Dimensions, EncodeImage, ImageRef, Rgb8};
        use rawshift_image_metadata::{
            exif::ExifBuilder, icc::IccProfile, xmp::append_xmp_to_avif,
        };
        let error = |error: gamut_core::Error| {
            rawshift_image_core::RawError::Encode(rawshift_image_core::EncodeError::Encoding {
                format: "AVIF",
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
        let mut output = Vec::new();
        gamut_avif::AvifEncoder::lossless()
            .encode_image(image, &mut output)
            .map_err(error)?;
        if let Ok(with_icc) = IccProfile::srgb().append_to_avif(output.clone()) {
            output = with_icc;
        }
        if let Ok(with_exif) = ExifBuilder::new(metadata).append_to_avif(output.clone()) {
            output = with_exif;
        }
        if let Some(xmp) = metadata.xmp.as_deref()
            && let Ok(with_xmp) = append_xmp_to_avif(xmp, output.clone())
        {
            output = with_xmp;
        }
        writer.write_all(&output)?;
        Ok(())
    }
}
