//! Encoder selection and per-implementation configuration.
//!
//! [`EncodeOptions`] mirrors [`DecodeOptions`](super::standard::DecodeOptions):
//! it is an *implementation-keyed* enum — each variant names one output format
//! *and* one backend, and carries that backend's configuration struct. A format
//! that gains an alternative encoder simply gains another variant; there is no
//! generic, implementation-agnostic option set.
//!
//! `EncodeOptions` is `#[non_exhaustive]` so the planned C/C++ encoder backends
//! (libjpeg-turbo, MozJPEG, libaom, SVT-AV1) can be added without a breaking
//! change. Their configuration structs are already defined below — see
//! [`MozjpegEncodeConfig`] and friends — so the API surface is stable ahead of
//! the implementations. The libjxl ([`LibjxlEncodeConfig`]) and jpegli
//! ([`JpegliEncodeConfig`]) backends are wired up behind the `jxl-encode-libjxl`
//! and `jpeg-encode-jpegli` features respectively.

#[cfg(feature = "dng-encode")]
use crate::formats::dng_export::DngExportConfig;

use super::standard::StandardFormat;
use crate::core::CodecId;

pub use crate::core::{BitDepth, MetadataEmbedOptions};

/// An output container format produced by an [`EncodeOptions`] variant.
///
/// Distinct from [`StandardFormat`] because it additionally includes `Dng`,
/// which is a RAW format rather than a standard delivery format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OutputFormat {
    /// Portable Network Graphics.
    Png,
    /// JPEG.
    Jpeg,
    /// WebP.
    WebP,
    /// AV1 Image File Format.
    Avif,
    /// JPEG XL.
    Jxl,
    /// Adobe Digital Negative.
    Dng,
}

impl OutputFormat {
    /// A short human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            OutputFormat::Png => "PNG",
            OutputFormat::Jpeg => "JPEG",
            OutputFormat::WebP => "WebP",
            OutputFormat::Avif => "AVIF",
            OutputFormat::Jxl => "JPEG XL",
            OutputFormat::Dng => "DNG",
        }
    }

    /// The conventional lowercase file extension (without a leading dot).
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpg",
            OutputFormat::WebP => "webp",
            OutputFormat::Avif => "avif",
            OutputFormat::Jxl => "jxl",
            OutputFormat::Dng => "dng",
        }
    }
}

/// Encoder-agnostic options shared by every backend.
///
/// Embedded as the `common` field of each per-implementation config struct so
/// that metadata-embedding and output bit-depth are configured uniformly,
/// independent of the chosen backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CommonEncodeOptions {
    /// Which metadata blocks to embed in the output container.
    pub metadata: MetadataEmbedOptions,
    /// Requested output bit depth.
    ///
    /// Encoders that cannot honour the request return
    /// [`EncodeError::UnsupportedBitDepth`](crate::error::EncodeError::UnsupportedBitDepth)
    /// rather than silently degrading.
    pub bit_depth: BitDepth,
}

/// WebP encoding mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WebPMode {
    /// VP8L lossless compression.
    Lossless,
    /// VP8 lossy compression.
    Lossy,
}

/// Chroma subsampling mode for JPEG encoders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum JpegSubsampling {
    /// 4:2:0 — chroma at quarter resolution. Smallest files; the usual default.
    #[default]
    Yuv420,
    /// 4:2:2 — chroma at half horizontal resolution.
    Yuv422,
    /// 4:4:4 — full-resolution chroma. Largest files, best chroma fidelity.
    Yuv444,
}

/// Rate-control strategy for AV1-based AVIF encoders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AvifRateControl {
    /// Constant quality / constant quantizer — quality knob drives file size.
    #[default]
    ConstantQuality,
    /// Constrained quality — quality target with a bitrate ceiling.
    Constrained,
}

// ── Currently-implemented backend configs ─────────────────────────────────────

/// Configuration for the `zune-png` PNG encoder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ZunePngEncodeConfig {
    /// Encoder-agnostic options (metadata embedding, bit depth).
    ///
    /// PNG honours `BitDepth::Eight` and `BitDepth::Sixteen`.
    pub common: CommonEncodeOptions,
}

/// Configuration for the `jpeg-encoder` (pure-Rust) JPEG encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JpegEncEncodeConfig {
    /// Encoder-agnostic options. JPEG output is always 8-bit.
    pub common: CommonEncodeOptions,
    /// Quality, `1..=100`. Higher is better quality and larger files (monotonic).
    /// Default: `90`.
    pub quality: u8,
}

impl Default for JpegEncEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            quality: 90,
        }
    }
}

/// Configuration for the `libwebp` WebP encoder.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibwebpEncodeConfig {
    /// Encoder-agnostic options. WebP output is always 8-bit.
    pub common: CommonEncodeOptions,
    /// Lossy or lossless mode.
    pub mode: WebPMode,
    /// Quality, `0.0..=100.0`. For lossy this is image quality; for lossless it
    /// is the compression effort. Higher quality is larger (lossy). Default: `75.0`.
    pub quality: f32,
    /// Compression method, `0` (fast) to `6` (slowest, best). Default: `4`.
    pub method: u32,
    /// Near-lossless preprocessing, `0..=100` (`100` = off). Lossless mode only.
    /// Default: `100`.
    pub near_lossless: u32,
}

impl LibwebpEncodeConfig {
    /// Lossy encoding with sensible defaults.
    pub fn lossy() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            mode: WebPMode::Lossy,
            quality: 75.0,
            method: 4,
            near_lossless: 100,
        }
    }

    /// Lossless encoding with sensible defaults.
    pub fn lossless() -> Self {
        Self {
            mode: WebPMode::Lossless,
            ..Self::lossy()
        }
    }
}

impl Default for LibwebpEncodeConfig {
    fn default() -> Self {
        Self::lossy()
    }
}

/// Configuration for the `ravif` (rav1e) AVIF encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RavifEncodeConfig {
    /// Encoder-agnostic options. This backend currently produces 8-bit AVIF.
    pub common: CommonEncodeOptions,
    /// Quality, `0..=100`. Higher is better quality and larger files (monotonic).
    /// Default: `80`.
    pub quality: u8,
    /// Encoding speed, `1` (slowest, best) to `10` (fastest). Default: `6`.
    pub speed: u8,
}

impl Default for RavifEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            quality: 80,
            speed: 6,
        }
    }
}

/// Configuration for the `zune-jpegxl` JPEG XL encoder (`JxlSimpleEncoder`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ZuneJxlEncodeConfig {
    /// Encoder-agnostic options. This backend currently produces 8-bit JXL.
    pub common: CommonEncodeOptions,
    /// Quality, `0.0..=100.0` (`0.0` requests lossless). Default: `0.0`.
    pub quality: f32,
    /// Effort, `1..=9` (higher is slower). `JxlSimpleEncoder` may ignore this.
    /// Default: `7`.
    pub effort: u8,
}

impl Default for ZuneJxlEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            quality: 0.0,
            effort: 7,
        }
    }
}

/// Modular vs VarDCT mode for the [`libjxl`](LibjxlEncodeConfig) encoder
/// (`JXL_ENC_FRAME_SETTING_MODULAR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LibjxlModular {
    /// Let libjxl decide (VarDCT for lossy, modular for lossless). The default.
    #[default]
    Auto,
    /// Force VarDCT — best for photographic, lossy content.
    VarDct,
    /// Force modular — best for lossless, non-photographic, or high-bit-depth content.
    Modular,
}

/// Internal color transform for the [`libjxl`](LibjxlEncodeConfig) encoder
/// (`JXL_ENC_FRAME_SETTING_COLOR_TRANSFORM`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LibjxlColorTransform {
    /// Let libjxl decide (XYB for lossy). The default.
    #[default]
    Auto,
    /// XYB — the JPEG XL perceptual space; best lossy compression.
    Xyb,
    /// No transform — encode the RGB channels directly.
    None,
    /// YCbCr.
    YCbCr,
}

/// Configuration for the **libjxl** JPEG XL encoder (the reference encoder).
///
/// Unlike the `zune-jpegxl` default ([`ZuneJxlEncodeConfig`]), libjxl honours
/// 8- and 16-bit output, mathematically lossless encoding, and the full set of
/// `JxlEncoderFrameSettingId` toggles. Requires the `jxl-encode-libjxl` feature.
///
/// Integer fields default to a `-1` sentinel and `Option` fields to `None`,
/// meaning "leave at libjxl's own default". This is a plain struct (not
/// `#[non_exhaustive]`): construct it with a struct literal plus
/// `..Default::default()`. The two `extra_*_options` vectors are escape hatches
/// that pass raw frame-setting ids through verbatim, so every libjxl toggle is
/// reachable even if it has no dedicated field.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibjxlEncodeConfig {
    /// Encoder-agnostic options. libjxl honours `BitDepth::Eight` and `BitDepth::Sixteen`.
    pub common: CommonEncodeOptions,
    /// Butteraugli distance: `0.0` is mathematically lossless, `1.0` visually
    /// lossless, up to `25.0` (smaller files, monotonic). Ignored when `quality`
    /// is `Some` or `lossless` is `true`.
    pub distance: f32,
    /// Optional JPEG-style quality, `0.0..=100.0`. When `Some`, libjxl converts
    /// it to a distance and it overrides `distance`.
    pub quality: Option<f32>,
    /// Force bit-exact lossless encoding (overrides `distance`/`quality`).
    pub lossless: bool,
    /// Effort / speed-quality trade-off, `1` (fastest) ..= `10` (slowest, best).
    pub effort: u8,
    /// Brotli effort for the metadata/modular streams, `-1` (default) or `0..=11`.
    pub brotli_effort: i8,
    /// Decoder-speed tier the stream is optimised for, `0` (default, best) ..= `4`.
    pub decoding_speed: u8,
    /// Emit a responsive / progressive stream (sets `RESPONSIVE` + `PROGRESSIVE_DC`).
    pub progressive: bool,
    /// Modular vs VarDCT mode.
    pub modular: LibjxlModular,
    /// Internal color transform.
    pub color_transform: LibjxlColorTransform,
    /// Edge-preserving filter strength, `-1` (default) ..= `3`.
    pub epf: i8,
    /// Gaborish deblocking filter: `None` = default, `Some(false)`/`Some(true)` = off/on.
    pub gaborish: Option<bool>,
    /// Adaptive noise synthesis: `None` = default, `Some(false)`/`Some(true)` = off/on.
    pub noise: Option<bool>,
    /// Dots synthesis: `None` = default, `Some(false)`/`Some(true)` = off/on.
    pub dots: Option<bool>,
    /// Patches synthesis: `None` = default, `Some(false)`/`Some(true)` = off/on.
    pub patches: Option<bool>,
    /// Photon-noise simulation strength in ISO units (`0.0` = off).
    pub photon_noise_iso: f32,
    /// Downsampling factor, `-1` (default), `1`, `2`, `4`, or `8`.
    pub resampling: i8,
    /// Force the BMFF container even when a bare codestream would do.
    pub use_container: bool,
    /// JPEG XL codestream feature level: `-1` (auto), `5`, or `10`.
    pub codestream_level: i8,
    /// Escape hatch: raw `(JxlEncoderFrameSettingId, value)` integer settings
    /// applied verbatim, for any toggle without a dedicated field above.
    pub extra_int_options: Vec<(i32, i64)>,
    /// Escape hatch: raw `(JxlEncoderFrameSettingId, value)` float settings.
    pub extra_float_options: Vec<(i32, f32)>,
}

impl Default for LibjxlEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            distance: 1.0,
            quality: None,
            lossless: false,
            effort: 7,
            brotli_effort: -1,
            decoding_speed: 0,
            progressive: false,
            modular: LibjxlModular::Auto,
            color_transform: LibjxlColorTransform::Auto,
            epf: -1,
            gaborish: None,
            noise: None,
            dots: None,
            patches: None,
            photon_noise_iso: 0.0,
            resampling: -1,
            use_container: false,
            codestream_level: -1,
            extra_int_options: Vec::new(),
            extra_float_options: Vec::new(),
        }
    }
}

/// Configuration for the **jpegli** JPEG encoder (libjxl's perceptual encoder).
///
/// Unlike the pure-Rust default ([`JpegEncEncodeConfig`]), jpegli offers
/// Butteraugli-distance rate control, XYB high-fidelity mode, and quantises from
/// the source's full precision when fed 16-bit input. Output is always an 8-bit
/// JPEG. Requires the `jpeg-encode-jpegli` feature.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JpegliEncodeConfig {
    /// Encoder-agnostic options. JPEG output is always 8-bit; with
    /// `BitDepth::Sixteen`, 16-bit samples are fed to jpegli at full precision.
    pub common: CommonEncodeOptions,
    /// Butteraugli distance: `0.0` is visually lossless; higher values produce
    /// smaller files (monotonic). Used when `quality` is `None`.
    pub distance: f32,
    /// Optional `1..=100` quality; when `Some`, overrides `distance`.
    pub quality: Option<u8>,
    /// Emit a progressive JPEG.
    pub progressive: bool,
    /// Encode in the XYB color space (jpegli high-fidelity mode). When set,
    /// jpegli chooses its own chroma sampling and `subsampling` is ignored.
    pub xyb: bool,
    /// Chroma subsampling mode (non-XYB mode only).
    pub subsampling: JpegSubsampling,
}

impl Default for JpegliEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            distance: 1.0,
            quality: None,
            progressive: true,
            xyb: false,
            subsampling: JpegSubsampling::Yuv420,
        }
    }
}

// ── Planned backend configs (API surface only — implementations pending) ──────
//
// These structs are defined now so the encode API is feature-complete and
// documented ahead of the C/C++ backend work. They are not yet wired to an
// `EncodeOptions` variant; each lands together with its backend in a follow-up.

/// Configuration for the planned **libjpeg-turbo** JPEG encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibjpegTurboEncodeConfig {
    /// Encoder-agnostic options. JPEG output is always 8-bit.
    pub common: CommonEncodeOptions,
    /// Quality, `1..=100`. Higher is better quality and larger files (monotonic).
    pub quality: u8,
    /// Emit a progressive (multi-scan) JPEG instead of baseline.
    pub progressive: bool,
    /// Chroma subsampling mode.
    pub subsampling: JpegSubsampling,
}

impl Default for LibjpegTurboEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            quality: 90,
            progressive: false,
            subsampling: JpegSubsampling::Yuv420,
        }
    }
}

/// Configuration for the planned **MozJPEG** JPEG encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MozjpegEncodeConfig {
    /// Encoder-agnostic options. JPEG output is always 8-bit.
    pub common: CommonEncodeOptions,
    /// Quality, `1..=100`. Higher is better quality and larger files (monotonic).
    pub quality: u8,
    /// Emit a progressive JPEG (MozJPEG enables this by default).
    pub progressive: bool,
    /// Enable trellis quantisation — slower, produces smaller files.
    pub trellis: bool,
    /// Chroma subsampling mode.
    pub subsampling: JpegSubsampling,
}

impl Default for MozjpegEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            quality: 85,
            progressive: true,
            trellis: true,
            subsampling: JpegSubsampling::Yuv420,
        }
    }
}

/// Configuration for the planned **libaom** AVIF encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibaomEncodeConfig {
    /// Encoder-agnostic options. libaom can produce 8/10/12-bit AVIF.
    pub common: CommonEncodeOptions,
    /// Constant-quantiser level, `0..=63`. Lower is better quality and larger
    /// files (monotonic). Used under [`AvifRateControl::ConstantQuality`].
    pub cq_level: u8,
    /// Minimum quantizer, `0..=63`.
    pub min_quantizer: u8,
    /// Maximum quantizer, `0..=63`.
    pub max_quantizer: u8,
    /// Speed/quality trade-off, `0` (slowest, best) to `8` (fastest).
    pub cpu_used: u8,
    /// Rate-control strategy.
    pub rate_control: AvifRateControl,
}

impl Default for LibaomEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            cq_level: 30,
            min_quantizer: 0,
            max_quantizer: 63,
            cpu_used: 6,
            rate_control: AvifRateControl::ConstantQuality,
        }
    }
}

/// Configuration for the planned **SVT-AV1** AVIF encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SvtAv1EncodeConfig {
    /// Encoder-agnostic options. SVT-AV1 can produce 8/10-bit AVIF.
    pub common: CommonEncodeOptions,
    /// Constant-Rate-Factor, `0..=63`. Lower is better quality and larger files
    /// (monotonic).
    pub crf: u8,
    /// Encoder preset, `0` (slowest, best) to `13` (fastest).
    pub preset: u8,
}

impl Default for SvtAv1EncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            crf: 35,
            preset: 8,
        }
    }
}

// ── EncodeOptions ─────────────────────────────────────────────────────────────

/// Selects the encoder implementation and carries its configuration.
///
/// Obtain one with [`EncodeOptions::default_for`] for a format's default
/// backend, or with a constructor such as [`EncodeOptions::jpeg`], or by naming
/// a variant directly to pin a specific backend.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EncodeOptions {
    /// PNG via `zune-png` (requires `png-encode`).
    #[cfg(feature = "png-encode")]
    PngZune(ZunePngEncodeConfig),
    /// JPEG via the pure-Rust `jpeg-encoder` (requires `jpeg-encode`).
    #[cfg(feature = "jpeg-encode")]
    JpegJpegEnc(JpegEncEncodeConfig),
    /// JPEG via `jpegli`, libjxl's perceptual encoder (requires `jpeg-encode-jpegli`).
    #[cfg(feature = "jpeg-encode-jpegli")]
    JpegJpegli(JpegliEncodeConfig),
    /// WebP via `libwebp` (requires `webp-encode`).
    #[cfg(feature = "webp-encode")]
    WebpLibwebp(LibwebpEncodeConfig),
    /// AVIF via `ravif` / rav1e (requires `avif-encode`).
    #[cfg(feature = "avif-encode")]
    AvifRavif(RavifEncodeConfig),
    /// JPEG XL via `zune-jpegxl` (requires `jxl-encode`).
    #[cfg(feature = "jxl-encode")]
    JxlZune(ZuneJxlEncodeConfig),
    /// JPEG XL via `libjxl`, the reference encoder (requires `jxl-encode-libjxl`).
    #[cfg(feature = "jxl-encode-libjxl")]
    JxlLibjxl(LibjxlEncodeConfig),
    /// DNG via the in-repo encoder (requires `dng-encode`).
    #[cfg(feature = "dng-encode")]
    Dng(DngExportConfig),
}

#[cfg(feature = "png-encode")]
impl Default for EncodeOptions {
    fn default() -> Self {
        Self::png()
    }
}

impl EncodeOptions {
    /// PNG with default configuration.
    #[cfg(feature = "png-encode")]
    pub fn png() -> Self {
        Self::PngZune(ZunePngEncodeConfig::default())
    }

    /// JPEG with default configuration.
    #[cfg(feature = "jpeg-encode")]
    pub fn jpeg() -> Self {
        Self::JpegJpegEnc(JpegEncEncodeConfig::default())
    }

    /// JPEG via the jpegli encoder, with default configuration.
    #[cfg(feature = "jpeg-encode-jpegli")]
    pub fn jpeg_jpegli() -> Self {
        Self::JpegJpegli(JpegliEncodeConfig::default())
    }

    /// Lossy WebP with default configuration.
    #[cfg(feature = "webp-encode")]
    pub fn webp_lossy() -> Self {
        Self::WebpLibwebp(LibwebpEncodeConfig::lossy())
    }

    /// Lossless WebP with default configuration.
    #[cfg(feature = "webp-encode")]
    pub fn webp_lossless() -> Self {
        Self::WebpLibwebp(LibwebpEncodeConfig::lossless())
    }

    /// AVIF with default configuration.
    #[cfg(feature = "avif-encode")]
    pub fn avif() -> Self {
        Self::AvifRavif(RavifEncodeConfig::default())
    }

    /// JPEG XL with default configuration.
    #[cfg(feature = "jxl-encode")]
    pub fn jxl() -> Self {
        Self::JxlZune(ZuneJxlEncodeConfig::default())
    }

    /// JPEG XL via the libjxl reference encoder, with default configuration.
    #[cfg(feature = "jxl-encode-libjxl")]
    pub fn jxl_libjxl() -> Self {
        Self::JxlLibjxl(LibjxlEncodeConfig::default())
    }

    /// DNG with default configuration.
    #[cfg(feature = "dng-encode")]
    pub fn dng() -> Self {
        Self::Dng(DngExportConfig::default())
    }

    /// The output format this encoder produces.
    pub fn format(&self) -> OutputFormat {
        match self {
            #[cfg(feature = "png-encode")]
            EncodeOptions::PngZune(_) => OutputFormat::Png,
            #[cfg(feature = "jpeg-encode")]
            EncodeOptions::JpegJpegEnc(_) => OutputFormat::Jpeg,
            #[cfg(feature = "jpeg-encode-jpegli")]
            EncodeOptions::JpegJpegli(_) => OutputFormat::Jpeg,
            #[cfg(feature = "webp-encode")]
            EncodeOptions::WebpLibwebp(_) => OutputFormat::WebP,
            #[cfg(feature = "avif-encode")]
            EncodeOptions::AvifRavif(_) => OutputFormat::Avif,
            #[cfg(feature = "jxl-encode")]
            EncodeOptions::JxlZune(_) => OutputFormat::Jxl,
            #[cfg(feature = "jxl-encode-libjxl")]
            EncodeOptions::JxlLibjxl(_) => OutputFormat::Jxl,
            #[cfg(feature = "dng-encode")]
            EncodeOptions::Dng(_) => OutputFormat::Dng,
            // Unreachable: with no encode feature enabled `EncodeOptions` has
            // no variants and no value of it can be constructed.
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    /// The stable identifier of the selected encoder implementation.
    pub fn codec_id(&self) -> CodecId {
        match self {
            #[cfg(feature = "png-encode")]
            EncodeOptions::PngZune(_) => CodecId::new("png/zune"),
            #[cfg(feature = "jpeg-encode")]
            EncodeOptions::JpegJpegEnc(_) => CodecId::new("jpeg/jpeg-encoder"),
            #[cfg(feature = "jpeg-encode-jpegli")]
            EncodeOptions::JpegJpegli(_) => CodecId::new("jpeg/jpegli"),
            #[cfg(feature = "webp-encode")]
            EncodeOptions::WebpLibwebp(_) => CodecId::new("webp/libwebp"),
            #[cfg(feature = "avif-encode")]
            EncodeOptions::AvifRavif(_) => CodecId::new("avif/ravif"),
            #[cfg(feature = "jxl-encode")]
            EncodeOptions::JxlZune(_) => CodecId::new("jxl/zune"),
            #[cfg(feature = "jxl-encode-libjxl")]
            EncodeOptions::JxlLibjxl(_) => CodecId::new("jxl/libjxl"),
            #[cfg(feature = "dng-encode")]
            EncodeOptions::Dng(_) => CodecId::new("dng/rawshift"),
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    /// The encoder-agnostic options (metadata embedding, bit depth) in effect.
    ///
    /// For DNG — which has its own configuration shape — a `CommonEncodeOptions`
    /// is synthesised from [`DngExportConfig`].
    pub fn common(&self) -> CommonEncodeOptions {
        match self {
            #[cfg(feature = "png-encode")]
            EncodeOptions::PngZune(c) => c.common,
            #[cfg(feature = "jpeg-encode")]
            EncodeOptions::JpegJpegEnc(c) => c.common,
            #[cfg(feature = "jpeg-encode-jpegli")]
            EncodeOptions::JpegJpegli(c) => c.common,
            #[cfg(feature = "webp-encode")]
            EncodeOptions::WebpLibwebp(c) => c.common,
            #[cfg(feature = "avif-encode")]
            EncodeOptions::AvifRavif(c) => c.common,
            #[cfg(feature = "jxl-encode")]
            EncodeOptions::JxlZune(c) => c.common,
            #[cfg(feature = "jxl-encode-libjxl")]
            EncodeOptions::JxlLibjxl(c) => c.common,
            #[cfg(feature = "dng-encode")]
            EncodeOptions::Dng(c) => CommonEncodeOptions {
                metadata: MetadataEmbedOptions {
                    embed_exif: c.embed_exif,
                    embed_icc: false,
                    embed_xmp: false,
                },
                bit_depth: BitDepth::Sixteen,
            },
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    /// The default encoder backend for `format`, with default configuration.
    ///
    /// Returns `None` when no encoder for `format` is compiled in, or when the
    /// format has no encoder. DNG is absent — it is not a [`StandardFormat`];
    /// use [`EncodeOptions::dng`].
    pub fn default_for(format: StandardFormat) -> Option<EncodeOptions> {
        match format {
            #[cfg(feature = "png-encode")]
            StandardFormat::Png => Some(EncodeOptions::png()),
            #[cfg(feature = "jpeg-encode")]
            StandardFormat::Jpeg => Some(EncodeOptions::jpeg()),
            #[cfg(feature = "webp-encode")]
            StandardFormat::WebP => Some(EncodeOptions::webp_lossy()),
            #[cfg(feature = "avif-encode")]
            StandardFormat::Avif => Some(EncodeOptions::avif()),
            #[cfg(feature = "jxl-encode")]
            StandardFormat::Jxl => Some(EncodeOptions::jxl()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(any_standard_encode)]
    use super::*;

    #[test]
    #[cfg(feature = "jpeg-encode")]
    fn default_for_jpeg_is_jpeg() {
        let opts = EncodeOptions::default_for(StandardFormat::Jpeg).unwrap();
        assert_eq!(opts.format(), OutputFormat::Jpeg);
        assert_eq!(opts.codec_id().id, "jpeg/jpeg-encoder");
    }

    #[test]
    #[cfg(feature = "png-encode")]
    fn default_for_unencodable_format_is_none() {
        assert!(EncodeOptions::default_for(StandardFormat::Gif).is_none());
    }

    #[test]
    #[cfg(feature = "avif-encode")]
    fn avif_default_quality_and_common() {
        let opts = EncodeOptions::avif();
        assert_eq!(opts.format(), OutputFormat::Avif);
        assert_eq!(opts.common().bit_depth, BitDepth::Sixteen);
    }

    #[test]
    fn output_format_names_and_extensions() {
        assert_eq!(OutputFormat::Jxl.name(), "JPEG XL");
        assert_eq!(OutputFormat::Avif.extension(), "avif");
    }
}
