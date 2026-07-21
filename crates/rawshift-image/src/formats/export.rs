//! Encoder selection and per-format configuration.
//!
//! [`EncodeOptions`] mirrors [`DecodeOptions`](super::standard::DecodeOptions):
//! it is a *format-keyed* enum — each variant names one output format and
//! carries the configuration of the backend that encodes it. There is no
//! backend-selection axis: gamut is the encoder for every migrated format,
//! and the remaining non-gamut backends (libwebp, pending the gamut-webp
//! migration) are named honestly by their configuration struct
//! ([`LibwebpEncodeConfig`]) rather than by extra enum variants.
//!
//! `EncodeOptions` is `#[non_exhaustive]` so formats whose encoders are
//! pending upstream (e.g. TIFF encode via gamut-tiff) can be added without a
//! breaking change.

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

/// Pixel-density unit for the JFIF APP0 segment written by the JPEG encoder
/// (maps to `gamut_jpeg::DensityUnit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum JpegDensityUnit {
    /// No absolute unit; the densities express only the pixel aspect ratio.
    #[default]
    AspectRatio,
    /// Dots per inch.
    Dpi,
    /// Dots per centimetre.
    Dpcm,
}

/// Pixel density written to the JFIF APP0 segment by the JPEG encoder.
///
/// The default is a 1:1 aspect ratio with no absolute unit (JFIF `units = 0`),
/// matching gamut-jpeg's encoder default. Densities are clamped to be non-zero
/// at encode time, as T.871 §10.1 requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JpegDensity {
    /// The density unit.
    pub unit: JpegDensityUnit,
    /// Horizontal density.
    pub x: u16,
    /// Vertical density.
    pub y: u16,
}

impl Default for JpegDensity {
    fn default() -> Self {
        Self {
            unit: JpegDensityUnit::AspectRatio,
            x: 1,
            y: 1,
        }
    }
}

/// Configuration for the `gamut-jpeg` JPEG encoder (pure Rust — baseline or
/// progressive 8-bit DCT).
///
/// Exposes exactly gamut-jpeg's encoder options: the `1..=100` quality dial
/// (frozen IJG quality→quantization mapping), chroma subsampling, the
/// progressive (SOF2) process, an optional restart interval, and the JFIF
/// pixel density. EXIF / ICC / XMP metadata is written by the encoder itself
/// as APP1/APP2 segments. Output is always an 8-bit JPEG.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JpegEncodeConfig {
    /// Encoder-agnostic options. JPEG output is always 8-bit.
    pub common: CommonEncodeOptions,
    /// Quality, clamped to `1..=100` at encode time (matching libjpeg's
    /// `jpeg_set_quality`). Higher is better quality and larger files
    /// (monotonic). Default: `90`.
    pub quality: u8,
    /// Chroma subsampling mode. Default: 4:2:0.
    pub subsampling: JpegSubsampling,
    /// Emit a progressive (SOF2, multi-scan) JPEG instead of baseline. The
    /// decoded image is identical to the baseline encoding at the same
    /// quality/subsampling; only the stream structure differs. Default: `false`.
    pub progressive: bool,
    /// Restart interval in MCUs: a restart marker (RSTn) is inserted every
    /// this many MCUs, letting a decoder resynchronize after corruption.
    /// `0` (the default) disables restarts.
    pub restart_interval: u16,
    /// JFIF pixel density written to the APP0 segment.
    pub density: JpegDensity,
}

impl Default for JpegEncodeConfig {
    fn default() -> Self {
        Self {
            common: CommonEncodeOptions::default(),
            quality: 90,
            subsampling: JpegSubsampling::Yuv420,
            progressive: false,
            restart_interval: 0,
            density: JpegDensity::default(),
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
/// ([visualcommons/gamut#251](https://github.com/visualcommons/gamut/issues/251)).
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

// ── EncodeOptions ─────────────────────────────────────────────────────────────

/// Selects the output format and carries its encoder configuration.
///
/// Obtain one with [`EncodeOptions::default_for`] for a format's default
/// configuration, or with a constructor such as [`EncodeOptions::jpeg`], or by
/// naming a variant directly.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EncodeOptions {
    /// PNG via `gamut-png` (requires `png-encode`).
    #[cfg(feature = "png-encode")]
    Png(PngEncodeConfig),
    /// JPEG via `gamut-jpeg` (requires `jpeg-encode`).
    #[cfg(feature = "jpeg-encode")]
    Jpeg(JpegEncodeConfig),
    /// WebP via `libwebp` (requires `webp-encode`; the gamut-webp migration is
    /// blocked upstream — gamut#302, tracked by rawshift#24).
    #[cfg(feature = "webp-encode")]
    WebP(LibwebpEncodeConfig),
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
        Self::Png(PngEncodeConfig::default())
    }

    /// JPEG with default configuration.
    #[cfg(feature = "jpeg-encode")]
    pub fn jpeg() -> Self {
        Self::Jpeg(JpegEncodeConfig::default())
    }

    /// Lossy WebP with default configuration.
    #[cfg(feature = "webp-encode")]
    pub fn webp_lossy() -> Self {
        Self::WebP(LibwebpEncodeConfig::lossy())
    }

    /// Lossless WebP with default configuration.
    #[cfg(feature = "webp-encode")]
    pub fn webp_lossless() -> Self {
        Self::WebP(LibwebpEncodeConfig::lossless())
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
            EncodeOptions::Png(_) => OutputFormat::Png,
            #[cfg(feature = "jpeg-encode")]
            EncodeOptions::Jpeg(_) => OutputFormat::Jpeg,
            #[cfg(feature = "webp-encode")]
            EncodeOptions::WebP(_) => OutputFormat::WebP,
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
            EncodeOptions::Png(_) => CodecId::new("png/gamut"),
            #[cfg(feature = "jpeg-encode")]
            EncodeOptions::Jpeg(_) => CodecId::new("jpeg/gamut"),
            #[cfg(feature = "webp-encode")]
            EncodeOptions::WebP(_) => CodecId::new("webp/libwebp"),
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
            EncodeOptions::Png(c) => c.common,
            #[cfg(feature = "jpeg-encode")]
            EncodeOptions::Jpeg(c) => c.common,
            #[cfg(feature = "webp-encode")]
            EncodeOptions::WebP(c) => c.common,
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
        assert_eq!(opts.codec_id().id, "jpeg/gamut");
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
