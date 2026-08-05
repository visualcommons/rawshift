//! PPM and Netpbm format support.
#![forbid(unsafe_code)]

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ZunePpmDecodeConfig {}

pub struct Ppm;
impl rawshift_image_core::FormatSniffer for Ppm {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Ppm;
    fn matches(data: &[u8]) -> bool {
        data.len() >= 3
            && data[0] == b'P'
            && matches!(data[1], b'5' | b'6' | b'7' | b'F' | b'f')
            && data[2].is_ascii_whitespace()
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Ppm {
    type Options = ZunePpmDecodeConfig;
    type Output = rawshift_image_core::RgbImage;
    fn decode(data: &[u8], _: &Self::Options) -> rawshift_image_core::RawResult<Self::Output> {
        use rawshift_image_core::{FormatError, RawError};
        use zune_core::{
            bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions,
            result::DecodingResult,
        };
        let mut decoder =
            zune_ppm::PPMDecoder::new_with_options(ZCursor::new(data), DecoderOptions::default());
        let result = decoder.decode().map_err(|error| {
            RawError::Format(FormatError::ImageDecode {
                format: "PPM",
                message: format!("{error:?}"),
            })
        })?;
        let (width, height) = decoder
            .dimensions()
            .map(|(w, h)| (w as u32, h as u32))
            .ok_or_else(|| {
                RawError::Format(FormatError::ImageDecode {
                    format: "PPM",
                    message: "could not read dimensions".to_owned(),
                })
            })?;
        let color = decoder.colorspace().unwrap_or(ColorSpace::RGB);
        let components = color.num_components();
        let samples = match result {
            DecodingResult::U8(v) => v.into_iter().map(|x| u16::from(x) * 257).collect(),
            DecodingResult::U16(v) => v,
            DecodingResult::F32(v) => v
                .into_iter()
                .map(|x| (x.clamp(0.0, 1.0) * 65535.0) as u16)
                .collect(),
            _ => {
                return Err(RawError::Format(FormatError::ImageDecode {
                    format: "PPM",
                    message: "unexpected pixel depth".to_owned(),
                }));
            }
        };
        let rgb = match color {
            ColorSpace::RGB => samples,
            ColorSpace::RGBA => samples
                .chunks_exact(components)
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect(),
            ColorSpace::Luma => samples.into_iter().flat_map(|v| [v; 3]).collect(),
            ColorSpace::LumaA => samples
                .chunks_exact(components)
                .flat_map(|p| [p[0]; 3])
                .collect(),
            _ => {
                return Err(RawError::Format(FormatError::ImageDecode {
                    format: "PPM",
                    message: format!("unsupported colorspace: {color:?}"),
                }));
            }
        };
        rawshift_image_core::RgbImage::new(width, height, rgb)
    }
}
