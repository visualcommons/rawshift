//! SVG rasterization support.
#![forbid(unsafe_code)]

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ResvgDecodeConfig {
    pub dpi: f32,
}
impl Default for ResvgDecodeConfig {
    fn default() -> Self {
        Self { dpi: 96.0 }
    }
}

pub struct Svg;
impl rawshift_image_core::FormatSniffer for Svg {
    const FORMAT: rawshift_image_core::FormatId = rawshift_image_core::FormatId::Svg;
    fn matches(data: &[u8]) -> bool {
        data.windows(4).any(|bytes| bytes == b"<svg")
    }
}

#[cfg(feature = "decode")]
impl rawshift_image_core::ImageDecoder for Svg {
    type Options = ResvgDecodeConfig;
    type Output = rawshift_image_core::RgbImage;
    fn decode(data: &[u8], config: &Self::Options) -> rawshift_image_core::RawResult<Self::Output> {
        use rawshift_image_core::{FormatError, RawError};
        use resvg::{tiny_skia, usvg};
        let options = usvg::Options {
            dpi: config.dpi,
            ..Default::default()
        };
        let tree = usvg::Tree::from_data(data, &options).map_err(|error| {
            RawError::Format(FormatError::ImageDecode {
                format: "SVG",
                message: error.to_string(),
            })
        })?;
        let size = tree.size().to_int_size();
        let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(|| {
            RawError::Format(FormatError::ImageDecode {
                format: "SVG",
                message: "failed to create pixmap".to_owned(),
            })
        })?;
        resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
        let samples = pixmap
            .data()
            .chunks_exact(4)
            .flat_map(|p| {
                [
                    u16::from(p[0]) * 257,
                    u16::from(p[1]) * 257,
                    u16::from(p[2]) * 257,
                ]
            })
            .collect();
        rawshift_image_core::RgbImage::new(size.width(), size.height(), samples)
    }
}
