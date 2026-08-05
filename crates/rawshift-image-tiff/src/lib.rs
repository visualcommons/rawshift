//! TIFF format support.
#![forbid(unsafe_code)]

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TiffDecodeConfig {}

pub struct Tiff;

impl rawshift_image_core::FormatSniffer for Tiff {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Tiff;
    fn matches(data: &[u8]) -> bool {
        data.starts_with(b"II\x2a\0") || data.starts_with(b"MM\0\x2a")
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Tiff {
    type Options = TiffDecodeConfig;
    type Output = rawshift_image_core::RgbImage;
    fn decode(data: &[u8], _: &Self::Options) -> rawshift_image_core::RawResult<Self::Output> {
        use rawshift_image_core::{FormatError, RawError};
        use tiff::{
            ColorType,
            decoder::{Decoder, DecodingResult},
        };
        let error = |error: tiff::TiffError| {
            RawError::Format(FormatError::ImageDecode {
                format: "TIFF",
                message: error.to_string(),
            })
        };
        let mut decoder = Decoder::new(std::io::Cursor::new(data)).map_err(error)?;
        let (width, height) = decoder.dimensions().map_err(error)?;
        let color = decoder.colortype().map_err(error)?;
        let samples = match decoder.read_image().map_err(error)? {
            DecodingResult::U8(values) => values.into_iter().map(|v| u16::from(v) * 257).collect(),
            DecodingResult::U16(values) => values,
            DecodingResult::U32(values) => values.into_iter().map(|v| (v >> 16) as u16).collect(),
            DecodingResult::F32(values) => values
                .into_iter()
                .map(|v| (v.clamp(0.0, 1.0) * 65535.0) as u16)
                .collect(),
            _ => {
                return Err(RawError::Format(FormatError::ImageDecode {
                    format: "TIFF",
                    message: "unsupported TIFF sample type".to_owned(),
                }));
            }
        };
        let rgb = match color {
            ColorType::RGB(_) => samples,
            ColorType::RGBA(_) => samples
                .chunks_exact(4)
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect(),
            ColorType::Gray(_) => samples.into_iter().flat_map(|v| [v; 3]).collect(),
            ColorType::GrayA(_) => samples.chunks_exact(2).flat_map(|p| [p[0]; 3]).collect(),
            ColorType::CMYK(_) => samples
                .chunks_exact(4)
                .flat_map(|p| {
                    let k = 1.0 - f64::from(p[3]) / 65535.0;
                    [
                        ((1.0 - f64::from(p[0]) / 65535.0) * k * 65535.0) as u16,
                        ((1.0 - f64::from(p[1]) / 65535.0) * k * 65535.0) as u16,
                        ((1.0 - f64::from(p[2]) / 65535.0) * k * 65535.0) as u16,
                    ]
                })
                .collect(),
            _ => {
                return Err(RawError::Format(FormatError::ImageDecode {
                    format: "TIFF",
                    message: format!("unsupported TIFF color type: {color:?}"),
                }));
            }
        };
        rawshift_image_core::RgbImage::new(width, height, rgb)
    }
}
