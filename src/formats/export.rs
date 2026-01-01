use crate::formats::dng_export::DngExportConfig;

/// Options for encoding the output image.
#[derive(Debug, Clone)]
pub enum EncodeOptions {
    /// PNG format options
    Png(PngOptions),
    /// JPEG format options
    Jpeg(JpegOptions),
    /// AVIF format options
    Avif(AvifOptions),
    /// HEIC format options
    Heic(HeicOptions),
    /// JPEG XL format options
    Jxl(JxlOptions),
    /// WebP format options
    WebP(WebPOptions),
    /// TIFF format options
    Tiff(TiffOptions),
    /// DNG format options
    Dng(DngExportConfig),
}

impl EncodeOptions {
    pub fn png() -> Self {
        Self::Png(PngOptions::default())
    }

    pub fn jpeg() -> Self {
        Self::Jpeg(JpegOptions::default())
    }

    pub fn avif() -> Self {
        Self::Avif(AvifOptions::default())
    }

    pub fn heic() -> Self {
        Self::Heic(HeicOptions::default())
    }

    pub fn jxl() -> Self {
        Self::Jxl(JxlOptions::default())
    }

    pub fn webp() -> Self {
        Self::WebP(WebPOptions::default())
    }

    pub fn tiff() -> Self {
        Self::Tiff(TiffOptions::default())
    }

    pub fn dng() -> Self {
        Self::Dng(DngExportConfig::default())
    }
}

/// Options for PNG encoding.
#[derive(Debug, Clone)]
pub struct PngOptions {
    /// Bit depth (8 or 16). Default: 16
    pub bit_depth: zune_core::bit_depth::BitDepth,
}

impl Default for PngOptions {
    fn default() -> Self {
        Self {
            bit_depth: zune_core::bit_depth::BitDepth::Sixteen,
        }
    }
}

/// Options for JPEG encoding.
#[derive(Debug, Clone, Default)]
pub struct JpegOptions {
    /// Quality (1-100). Default: 90
    pub quality: u8,
}

/// Options for AVIF encoding.
#[derive(Debug, Clone, Default)]
pub struct AvifOptions {
    /// Quality (1-100). Default: 80
    pub quality: u8,
    /// Speed (0-10). Default: 6
    pub speed: u8,
}

/// Options for HEIC encoding.
#[derive(Debug, Clone, Default)]
pub struct HeicOptions {
    /// Quality (1-100). Default: 80
    pub quality: u8,
}

/// Options for JPEG XL encoding.
#[derive(Debug, Clone, Default)]
pub struct JxlOptions {
    /// Quality (1-100, or 0 for lossless). Default: 0 (Lossless)
    pub quality: f32,
    /// Effort (1-9). Default: 7
    pub effort: u8,
}

/// Options for WebP encoding.
#[derive(Debug, Clone, Default)]
pub struct WebPOptions {
    /// Quality (1-100). Default: 80
    pub quality: f32,
    /// Lossless mode. Default: true
    pub lossless: bool,
}

/// Options for TIFF encoding.
#[derive(Debug, Clone, Default)]
pub struct TiffOptions {
    // TODO: Add options
}
