use crate::formats::dng_export::DngExportConfig;

/// Options for encoding the output image.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EncodeOptions {
    /// PNG format options
    Png(PngOptions),
    /// JPEG format options
    Jpeg(JpegOptions),
    /// WebP format options
    WebP(WebPOptions),
    /// AVIF format options (requires `avif` feature)
    #[cfg(feature = "avif")]
    Avif(AvifOptions),
    /// JPEG XL format options (requires `jxl-encode` feature)
    #[cfg(feature = "jxl-encode")]
    Jxl(JxlOptions),
    /// DNG format options
    Dng(DngExportConfig),
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self::Png(PngOptions::default())
    }
}

impl EncodeOptions {
    pub fn png() -> Self {
        Self::Png(PngOptions::default())
    }

    pub fn jpeg() -> Self {
        Self::Jpeg(JpegOptions::default())
    }

    pub fn webp() -> Self {
        Self::WebP(WebPOptions::default())
    }

    #[cfg(feature = "avif")]
    pub fn avif() -> Self {
        Self::Avif(AvifOptions::default())
    }

    #[cfg(feature = "jxl-encode")]
    pub fn jxl() -> Self {
        Self::Jxl(JxlOptions::default())
    }

    pub fn dng() -> Self {
        Self::Dng(DngExportConfig::default())
    }
}

/// Options for PNG encoding.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PngOptions {
    /// Bit depth (8 or 16). Default: 16
    #[cfg_attr(feature = "serde", serde(skip))]
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
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JpegOptions {
    /// Quality (1-100). Default: 90
    pub quality: u8,
    /// Whether to embed EXIF metadata. Default: true
    pub embed_exif: bool,
    /// Whether to embed ICC profile. Default: true
    pub embed_icc: bool,
}

impl Default for JpegOptions {
    fn default() -> Self {
        Self {
            quality: 90,
            embed_exif: true,
            embed_icc: true,
        }
    }
}

/// Options for WebP encoding.
///
/// Encoding uses VP8L (lossless). Lossy encoding will be supported
/// when a suitable encoder dependency becomes available.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WebPOptions {
    /// Whether to embed EXIF metadata. Default: true
    pub embed_exif: bool,
    /// Whether to embed ICC profile. Default: true
    pub embed_icc: bool,
}

impl Default for WebPOptions {
    fn default() -> Self {
        Self {
            embed_exif: true,
            embed_icc: true,
        }
    }
}

/// Options for AVIF encoding (requires `avif` feature).
#[cfg(feature = "avif")]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AvifOptions {
    /// Quality (0-100, lower is better). Default: 80
    pub quality: u8,
    /// Speed (1-10, higher is faster). Default: 6
    pub speed: u8,
    /// Whether to embed EXIF metadata. Default: true
    pub embed_exif: bool,
}

#[cfg(feature = "avif")]
impl Default for AvifOptions {
    fn default() -> Self {
        Self {
            quality: 80,
            speed: 6,
            embed_exif: true,
        }
    }
}

/// Options for JPEG XL encoding (requires `jxl-encode` feature).
#[cfg(feature = "jxl-encode")]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JxlOptions {
    /// Quality (0.0-100.0, 0 for lossless). Default: 0.0 (lossless)
    pub quality: f32,
    /// Effort (1-9, higher is slower). Default: 7
    pub effort: u8,
    /// Whether to embed EXIF metadata. Default: true
    pub embed_exif: bool,
}

#[cfg(feature = "jxl-encode")]
impl Default for JxlOptions {
    fn default() -> Self {
        Self {
            quality: 0.0,
            effort: 7,
            embed_exif: true,
        }
    }
}
