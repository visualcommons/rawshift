#[cfg(feature = "dng-encode")]
use crate::formats::dng_export::DngExportConfig;

/// Controls which metadata blocks are embedded in the exported image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MetadataEmbedOptions {
    /// Embed EXIF metadata. Default: `true`
    pub embed_exif: bool,
    /// Embed ICC colour profile. Default: `true`
    pub embed_icc: bool,
    /// Embed XMP metadata (JPEG, PNG, WebP, AVIF, JXL). Default: `true`
    pub embed_xmp: bool,
}

impl Default for MetadataEmbedOptions {
    fn default() -> Self {
        Self {
            embed_exif: true,
            embed_icc: true,
            embed_xmp: true,
        }
    }
}

/// Selects which encoder implementation produces the output image, and carries
/// that implementation's configuration.
///
/// Each variant pairs an output format with one backend. Compressed formats may
/// gain alternative encoder implementations over time; each such implementation
/// gets its own variant and its own configuration struct, so there is never a
/// generic, implementation-agnostic option set. `Dng` is the exception — DNG is
/// a RAW format with a single in-repo encoder.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EncodeOptions {
    /// PNG format options (requires `png-encode` feature)
    #[cfg(feature = "png-encode")]
    Png(PngOptions),
    /// JPEG format options (requires `jpeg-encode` feature)
    #[cfg(feature = "jpeg-encode")]
    Jpeg(JpegOptions),
    /// WebP format options (requires `webp-encode` feature)
    #[cfg(feature = "webp-encode")]
    WebP(WebPOptions),
    /// AVIF format options (requires `avif-encode` feature)
    #[cfg(feature = "avif-encode")]
    Avif(AvifOptions),
    /// JPEG XL format options (requires `jxl-encode` feature)
    #[cfg(feature = "jxl-encode")]
    Jxl(JxlOptions),
    /// DNG format options (requires `dng-encode` feature)
    #[cfg(feature = "dng-encode")]
    Dng(DngExportConfig),
}

#[cfg(feature = "png-encode")]
impl Default for EncodeOptions {
    fn default() -> Self {
        Self::Png(PngOptions::default())
    }
}

impl EncodeOptions {
    #[cfg(feature = "png-encode")]
    pub fn png() -> Self {
        Self::Png(PngOptions::default())
    }

    #[cfg(feature = "jpeg-encode")]
    pub fn jpeg() -> Self {
        Self::Jpeg(JpegOptions::default())
    }

    #[cfg(feature = "webp-encode")]
    pub fn webp_lossy() -> Self {
        Self::WebP(WebPOptions::lossy())
    }

    #[cfg(feature = "webp-encode")]
    pub fn webp_lossless() -> Self {
        Self::WebP(WebPOptions::lossless())
    }

    #[cfg(feature = "avif-encode")]
    pub fn avif() -> Self {
        Self::Avif(AvifOptions::default())
    }

    #[cfg(feature = "jxl-encode")]
    pub fn jxl() -> Self {
        Self::Jxl(JxlOptions::default())
    }

    #[cfg(feature = "dng-encode")]
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
    /// Metadata embedding options.
    pub metadata: MetadataEmbedOptions,
}

impl Default for PngOptions {
    fn default() -> Self {
        Self {
            bit_depth: zune_core::bit_depth::BitDepth::Sixteen,
            metadata: MetadataEmbedOptions::default(),
        }
    }
}

/// Options for JPEG encoding.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JpegOptions {
    /// Quality (1-100). Default: 90
    pub quality: u8,
    /// Metadata embedding options.
    pub metadata: MetadataEmbedOptions,
}

impl Default for JpegOptions {
    fn default() -> Self {
        Self {
            quality: 90,
            metadata: MetadataEmbedOptions::default(),
        }
    }
}

/// WebP encoding mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WebPMode {
    /// VP8L lossless compression
    Lossless,
    /// VP8 lossy compression
    Lossy,
}

/// Options for WebP encoding.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WebPOptions {
    /// Lossy or lossless mode
    pub mode: WebPMode,
    /// Quality (0-100). For lossy: image quality. For lossless: compression effort. Default: 75
    pub quality: f32,
    /// Compression method (0=fast, 6=slowest/best). Default: 4
    pub method: u32,
    /// Near-lossless preprocessing (0-100, 100=off). Only used in Lossless mode. Default: 100
    pub near_lossless: u32,
    /// Metadata embedding options.
    pub metadata: MetadataEmbedOptions,
}

impl Default for WebPOptions {
    fn default() -> Self {
        Self::lossy()
    }
}

impl WebPOptions {
    /// Lossy encoding with sensible defaults.
    pub fn lossy() -> Self {
        Self {
            mode: WebPMode::Lossy,
            quality: 75.0,
            method: 4,
            near_lossless: 100,
            metadata: MetadataEmbedOptions::default(),
        }
    }

    /// Lossless encoding with sensible defaults.
    pub fn lossless() -> Self {
        Self {
            mode: WebPMode::Lossless,
            quality: 75.0,
            method: 4,
            near_lossless: 100,
            metadata: MetadataEmbedOptions::default(),
        }
    }
}

/// Options for AVIF encoding (requires `avif-encode` feature).
#[cfg(feature = "avif-encode")]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AvifOptions {
    /// Quality (0-100, lower is better). Default: 80
    pub quality: u8,
    /// Speed (1-10, higher is faster). Default: 6
    pub speed: u8,
    /// Metadata embedding options.
    pub metadata: MetadataEmbedOptions,
}

#[cfg(feature = "avif-encode")]
impl Default for AvifOptions {
    fn default() -> Self {
        Self {
            quality: 80,
            speed: 6,
            metadata: MetadataEmbedOptions::default(),
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
    /// Metadata embedding options.
    pub metadata: MetadataEmbedOptions,
}

#[cfg(feature = "jxl-encode")]
impl Default for JxlOptions {
    fn default() -> Self {
        Self {
            quality: 0.0,
            effort: 7,
            metadata: MetadataEmbedOptions::default(),
        }
    }
}
