//! GIF format support.
#![forbid(unsafe_code)]

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GifDecodeConfig {}

pub struct Gif;

impl rawshift_image_core::FormatSniffer for Gif {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Gif;
    fn matches(data: &[u8]) -> bool {
        data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Gif {
    type Options = GifDecodeConfig;
    type Output = rawshift_image_core::RgbImage;
    fn decode(data: &[u8], _: &Self::Options) -> rawshift_image_core::RawResult<Self::Output> {
        use gif::{ColorOutput, DecodeOptions};
        use rawshift_image_core::{FormatError, RawError};
        let error = |error: gif::DecodingError| {
            RawError::Format(FormatError::ImageDecode {
                format: "GIF",
                message: error.to_string(),
            })
        };
        let mut options = DecodeOptions::new();
        options.set_color_output(ColorOutput::RGBA);
        let mut decoder = options
            .read_info(std::io::Cursor::new(data))
            .map_err(error)?;
        let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
        let frame = decoder.read_next_frame().map_err(error)?.ok_or_else(|| {
            RawError::Format(FormatError::ImageDecode {
                format: "GIF",
                message: "no frames in GIF".to_owned(),
            })
        })?;
        let mut output = vec![0; width as usize * height as usize * 3];
        let (fw, fh, left, top) = (
            frame.width as usize,
            frame.height as usize,
            frame.left as usize,
            frame.top as usize,
        );
        if frame.buffer.len() < fw * fh * 4 {
            return Err(RawError::Format(FormatError::ImageDecode {
                format: "GIF",
                message: "frame buffer too small".to_owned(),
            }));
        }
        for row in 0..fh {
            for column in 0..fw {
                let (x, y) = (left + column, top + row);
                if x >= width as usize || y >= height as usize {
                    continue;
                }
                let source = (row * fw + column) * 4;
                let target = (y * width as usize + x) * 3;
                output[target..target + 3].copy_from_slice(&[
                    u16::from(frame.buffer[source]) * 257,
                    u16::from(frame.buffer[source + 1]) * 257,
                    u16::from(frame.buffer[source + 2]) * 257,
                ]);
            }
        }
        rawshift_image_core::RgbImage::new(width, height, output)
    }
}
