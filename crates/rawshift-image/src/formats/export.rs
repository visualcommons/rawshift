//! Encoder selection and per-implementation configuration.
//!
//! [`EncodeOptions`] mirrors [`DecodeOptions`](super::standard::DecodeOptions):
//! it is an *implementation-keyed* enum — each variant names one output format
//! *and* one backend, and carries that backend's configuration struct. A format
//! that gains an alternative encoder simply gains another variant; there is no
//! generic, implementation-agnostic option set.
//!
//! `EncodeOptions` is `#[non_exhaustive]` so the planned C/C++ encoder backends
//! (libjpeg-turbo, MozJPEG, SVT-AV1) can be added without a breaking
//! change. Their configuration structs are already defined below — see
//! [`MozjpegEncodeConfig`] and friends — so the API surface is stable ahead of
//! the implementations. The jpegli ([`JpegliEncodeConfig`]) backend is wired
//! up behind the `jpeg-encode-jpegli` feature.

#[cfg(feature = "dng-encode")]
use crate::formats::dng_export::DngEncodeConfig;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CommonEncodeOptions {
    /// Which metadata blocks to embed in the output container.
    pub metadata: MetadataEmbedOptions,
    /// Requested output bit depth.
    ///
    /// Encoders that cannot honour the request return
    /// [`EncodeError::UnsupportedBitDepth`](crate::error::EncodeError::UnsupportedBitDepth)
    /// rather than silently degrading.
    #[cfg_attr(
        feature = "serde",
        serde(with = "rawshift_core::color::bit_depth_serde")
    )]
    pub bit_depth: BitDepth,
}

impl Default for CommonEncodeOptions {
    /// Defaults to 16-bit output ([`BitDepth::Sixteen`]) with default metadata
    /// embedding options.
    fn default() -> Self {
        Self {
            metadata: MetadataEmbedOptions::default(),
            bit_depth: BitDepth::Sixteen,
        }
    }
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

/// DEFLATE compression level for the `gamut-png` PNG encoder
/// (maps to `gamut_deflate::Level`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PngCompressionLevel {
    /// Stored (uncompressed) blocks only — fastest, largest.
    Store,
    /// Fast: greedy matching with fixed Huffman codes.
    Fast,
    /// Balanced default: lazy matching with per-block dynamic Huffman codes.
    #[default]
    Default,
    /// Smallest output: zopfli-style optimal parse. Slowest; for write-once
    /// assets where size dominates.
    Best,
}

/// A fixed PNG scanline filter type (PNG §9.1) for
/// [`PngFilterStrategy::Fixed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PngFilterType {
    /// No filtering; bytes stored as-is.
    None,
    /// Residual from the byte one pixel to the left.
    Sub,
    /// Residual from the byte directly above.
    Up,
    /// Residual from the floor-average of the left and above bytes.
    Average,
    /// Residual from the Paeth predictor of left, above, and above-left.
    Paeth,
}

/// Scanline filter strategy for the `gamut-png` PNG encoder
/// (maps to `gamut_png::FilterStrategy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PngFilterStrategy {
    /// Filter every scanline with [`PngFilterType::None`] (fastest).
    None,
    /// Use one fixed filter for every scanline.
    Fixed(PngFilterType),
    /// Per scanline, pick the filter minimising the sum of absolute residuals
    /// — the standard libpng heuristic and the default.
    #[default]
    MinSumAbs,
    /// Try several whole-image strategies, DEFLATE each, keep the smallest.
    /// Pairs with [`PngCompressionLevel::Best`] for maximum compression; slowest.
    BruteForce,
}

// ── Currently-implemented backend configs ─────────────────────────────────────

/// Configuration for the `gamut-png` PNG encoder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PngEncodeConfig {
    /// Encoder-agnostic options (metadata embedding, bit depth).
    ///
    /// PNG honours `BitDepth::Eight` and `BitDepth::Sixteen`.
    pub common: CommonEncodeOptions,
    /// DEFLATE compression level for the image data.
    pub compression: PngCompressionLevel,
    /// Scanline filter strategy.
    pub filter: PngFilterStrategy,
    /// Losslessly reduce truecolour input to a smaller colour type (greyscale,
    /// palette, or alpha-dropped) when no pixel changes. Off by default so the
    /// output colour type matches the input.
    pub auto_reduce: bool,
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

/// Configuration for the `gamut-avif` AVIF encoder (pure Rust — the AV1 intra
/// codestream from gamut-av1 wrapped in a gamut-isobmff container).
///
/// Exposes exactly gamut-avif's encoder options: lossless (the default) or
/// lossy AV1 intra coding at identity-matrix 4:4:4, with a `0..=100` quality
/// factor in lossy mode. The encoder takes 8-bit RGB input, so `common.bit_depth`
/// honours `BitDepth::Eight` and `BitDepth::Sixteen` (16-bit samples are
/// down-converted to 8-bit, as with every 8-bit-only backend); `Ten` and
/// `Twelve` return
/// [`EncodeError::UnsupportedBitDepth`](crate::error::EncodeError::UnsupportedBitDepth)
/// — high-bit-depth AVIF encode is **temporarily unavailable** pending
/// 10/12-bit support in gamut-avif
/// ([justin13888/gamut#251](https://github.com/justin13888/gamut/issues/251)).
///
/// EXIF / ICC / XMP metadata is spliced into the encoded container as ISOBMFF
/// items by rawshift (gamut-avif does not emit metadata items yet).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AvifEncodeConfig {
    /// Encoder-agnostic options (metadata embedding, bit depth).
    pub common: CommonEncodeOptions,
    /// Lossless encoding (decoded output bit-exact to the 8-bit input). When
    /// set, `quality` is ignored. Default: `true`, matching gamut-avif's
    /// default mode.
    pub lossless: bool,
    /// Quality for lossy encoding, `0..=100` (values above `100` are clamped).
    /// Higher is better quality and larger files (monotonic). Used only when
    /// `lossless` is `false`. Default: `80`.
    pub quality: u8,
}

impl Default for AvifEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            lossless: true,
            quality: 80,
        }
    }
}

/// Configuration for the `gamut-jxl` JPEG XL encoder (the reference libjxl,
/// statically linked through `gamut-jxl-sys`).
///
/// Exposes exactly gamut-jxl's encoder options: the lossless/lossy mode with a
/// Butteraugli distance, the `1..=10` effort dial, ISO BMFF container framing,
/// and an optional coded-bit-depth override. Honours `BitDepth::Eight` and
/// `BitDepth::Sixteen` output (true 16-bit — including mathematically lossless
/// 16-bit round-trips). EXIF / XMP metadata is written by the encoder itself
/// as container boxes, and an sRGB ICC profile (when
/// [`MetadataEmbedOptions::embed_icc`] is set) is embedded in the codestream's
/// colour metadata.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JxlEncodeConfig {
    /// Encoder-agnostic options (metadata embedding, bit depth).
    pub common: CommonEncodeOptions,
    /// Mathematically lossless encoding (bit-exact round-trip). When set,
    /// `distance` is ignored. Default: `true`, matching gamut-jxl's default
    /// mode and the previous default JXL backend (quality 0.0 = lossless).
    pub lossless: bool,
    /// Butteraugli target distance for lossy encoding, finite and in
    /// `(0.0, 25.0]`: `1.0` is "visually lossless", larger is smaller files.
    /// Used only when `lossless` is `false`. Default: `1.0`.
    pub distance: f32,
    /// Effort / speed-density trade-off, `1` (fastest) ..= `10` (slowest,
    /// densest); maps onto libjxl's effort levels. Default: `7`.
    pub effort: u8,
    /// Force the ISO BMFF `.jxl` container even when no metadata box requires
    /// it. Embedding EXIF or XMP switches to the container automatically;
    /// otherwise the output is a bare codestream. Default: `false`.
    pub use_container: bool,
    /// Declared coded bit depth N (`1..=16`): 16-bit samples then carry N-bit
    /// code values (`0..=2^N - 1`) instead of full-range 16-bit. `None` (the
    /// default) declares the storage width.
    pub coded_bit_depth: Option<u8>,
}

impl Default for JxlEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            lossless: true,
            distance: 1.0,
            effort: 7,
            use_container: false,
            coded_bit_depth: None,
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
    /// PNG via `gamut-png` (requires `png-encode`).
    #[cfg(feature = "png-encode")]
    PngGamut(PngEncodeConfig),
    /// JPEG via the pure-Rust `jpeg-encoder` (requires `jpeg-encode`).
    #[cfg(feature = "jpeg-encode")]
    JpegJpegEnc(JpegEncEncodeConfig),
    /// JPEG via `jpegli`, libjxl's perceptual encoder (requires `jpeg-encode-jpegli`).
    #[cfg(feature = "jpeg-encode-jpegli")]
    JpegJpegli(JpegliEncodeConfig),
    /// WebP via `libwebp` (requires `webp-encode`).
    #[cfg(feature = "webp-encode")]
    WebpLibwebp(LibwebpEncodeConfig),
    /// AVIF via `gamut-avif` (requires `avif-encode`).
    #[cfg(feature = "avif-encode")]
    Avif(AvifEncodeConfig),
    /// JPEG XL via `gamut-jxl`, wrapping the reference libjxl encoder
    /// (requires `jxl-encode`).
    #[cfg(feature = "jxl-encode")]
    Jxl(JxlEncodeConfig),
    /// DNG via the in-repo encoder (requires `dng-encode`).
    #[cfg(feature = "dng-encode")]
    Dng(DngEncodeConfig),
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
        Self::PngGamut(PngEncodeConfig::default())
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

    /// AVIF with default configuration (lossless).
    #[cfg(feature = "avif-encode")]
    pub fn avif() -> Self {
        Self::Avif(AvifEncodeConfig::default())
    }

    /// JPEG XL with default configuration (lossless).
    #[cfg(feature = "jxl-encode")]
    pub fn jxl() -> Self {
        Self::Jxl(JxlEncodeConfig::default())
    }

    /// DNG with default configuration.
    #[cfg(feature = "dng-encode")]
    pub fn dng() -> Self {
        Self::Dng(DngEncodeConfig::default())
    }

    /// The output format this encoder produces.
    pub fn format(&self) -> OutputFormat {
        match self {
            #[cfg(feature = "png-encode")]
            EncodeOptions::PngGamut(_) => OutputFormat::Png,
            #[cfg(feature = "jpeg-encode")]
            EncodeOptions::JpegJpegEnc(_) => OutputFormat::Jpeg,
            #[cfg(feature = "jpeg-encode-jpegli")]
            EncodeOptions::JpegJpegli(_) => OutputFormat::Jpeg,
            #[cfg(feature = "webp-encode")]
            EncodeOptions::WebpLibwebp(_) => OutputFormat::WebP,
            #[cfg(feature = "avif-encode")]
            EncodeOptions::Avif(_) => OutputFormat::Avif,
            #[cfg(feature = "jxl-encode")]
            EncodeOptions::Jxl(_) => OutputFormat::Jxl,
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
            EncodeOptions::PngGamut(_) => CodecId::new("png/gamut"),
            #[cfg(feature = "jpeg-encode")]
            EncodeOptions::JpegJpegEnc(_) => CodecId::new("jpeg/jpeg-encoder"),
            #[cfg(feature = "jpeg-encode-jpegli")]
            EncodeOptions::JpegJpegli(_) => CodecId::new("jpeg/jpegli"),
            #[cfg(feature = "webp-encode")]
            EncodeOptions::WebpLibwebp(_) => CodecId::new("webp/libwebp"),
            #[cfg(feature = "avif-encode")]
            EncodeOptions::Avif(_) => CodecId::new("avif/gamut"),
            #[cfg(feature = "jxl-encode")]
            EncodeOptions::Jxl(_) => CodecId::new("jxl/gamut"),
            #[cfg(feature = "dng-encode")]
            EncodeOptions::Dng(_) => CodecId::new("dng/rawshift"),
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    /// The encoder-agnostic options (metadata embedding, bit depth) in effect.
    ///
    /// For DNG — which has its own configuration shape — a `CommonEncodeOptions`
    /// is synthesised from [`DngEncodeConfig`].
    pub fn common(&self) -> CommonEncodeOptions {
        match self {
            #[cfg(feature = "png-encode")]
            EncodeOptions::PngGamut(c) => c.common,
            #[cfg(feature = "jpeg-encode")]
            EncodeOptions::JpegJpegEnc(c) => c.common,
            #[cfg(feature = "jpeg-encode-jpegli")]
            EncodeOptions::JpegJpegli(c) => c.common,
            #[cfg(feature = "webp-encode")]
            EncodeOptions::WebpLibwebp(c) => c.common,
            #[cfg(feature = "avif-encode")]
            EncodeOptions::Avif(c) => c.common,
            #[cfg(feature = "jxl-encode")]
            EncodeOptions::Jxl(c) => c.common,
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
