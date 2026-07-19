//! Standard image format encode functions.
//!
//! [`encode_rgb_image_to_vec`] is the single in-memory entry point and supports
//! every output format. [`encode_rgb_image_to_writer`] and [`encode_rgb_image`]
//! are thin wrappers that stream or persist its result.
//!
//! Metadata (EXIF / ICC / XMP) is embedded entirely in-memory, so no format
//! needs a file path — including AVIF and JXL, which previously did.

use std::path::Path;

#[cfg(any_standard_encode)]
use crate::core::BitDepth;
use crate::core::RgbImage;
use crate::core::metadata::ImageMetadata;
#[cfg(any_standard_encode)]
use crate::error::EncodeError;
use crate::error::{RawError, RawResult};

use super::export::EncodeOptions;
#[cfg(feature = "webp-encode")]
use super::export::WebPMode;

/// Encode a linear RGB image to an in-memory byte buffer.
///
/// This is the core encode entry point — every output format (PNG, JPEG, WebP,
/// AVIF, JXL, DNG) is supported, with EXIF/ICC/XMP metadata embedded according
/// to the encoder's [`CommonEncodeOptions`](super::export::CommonEncodeOptions).
///
/// `image` must contain 16-bit RGB data normalized to `[0, 65535]`.
#[cfg_attr(not(any_standard_encode), allow(unused_variables, unreachable_code))]
pub fn encode_rgb_image_to_vec(
    image: &RgbImage,
    metadata: &ImageMetadata,
    encode_options: &EncodeOptions,
) -> RawResult<Vec<u8>> {
    match encode_options {
        #[cfg(feature = "png-encode")]
        EncodeOptions::PngGamut(cfg) => encode_png(image, metadata, cfg),
        #[cfg(feature = "jpeg-encode")]
        EncodeOptions::JpegJpegEnc(cfg) => encode_jpeg(image, metadata, cfg),
        #[cfg(feature = "jpeg-encode-jpegli")]
        EncodeOptions::JpegJpegli(cfg) => encode_jpeg_jpegli(image, metadata, cfg),
        #[cfg(feature = "webp-encode")]
        EncodeOptions::WebpLibwebp(cfg) => encode_webp(image, metadata, cfg),
        #[cfg(feature = "avif-encode")]
        EncodeOptions::Avif(cfg) => encode_avif(image, metadata, cfg),
        #[cfg(feature = "jxl-encode")]
        EncodeOptions::Jxl(cfg) => encode_jxl(image, metadata, cfg),
        #[cfg(feature = "dng-encode")]
        EncodeOptions::Dng(cfg) => {
            let mut buf = std::io::Cursor::new(Vec::new());
            super::dng_export::export_dng_to_writer(&mut buf, image, metadata, cfg)?;
            Ok(buf.into_inner())
        }
        #[allow(unreachable_patterns)]
        _ => Err(RawError::Unsupported(
            "This encode format is not available with the current feature flags.".to_string(),
        )),
    }
}

/// Encode a linear RGB image to any writer.
///
/// Convenience wrapper over [`encode_rgb_image_to_vec`]; supports every format.
pub fn encode_rgb_image_to_writer<W: std::io::Write>(
    image: &RgbImage,
    metadata: &ImageMetadata,
    writer: &mut W,
    encode_options: &EncodeOptions,
) -> RawResult<()> {
    let bytes = encode_rgb_image_to_vec(image, metadata, encode_options)?;
    writer.write_all(&bytes)?;
    Ok(())
}

/// Encode a linear RGB image to a file.
///
/// Convenience wrapper over [`encode_rgb_image_to_vec`].
pub fn encode_rgb_image(
    image: &RgbImage,
    metadata: &ImageMetadata,
    path: &Path,
    encode_options: &EncodeOptions,
) -> RawResult<()> {
    let bytes = encode_rgb_image_to_vec(image, metadata, encode_options)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

// ── Sample packing helpers ────────────────────────────────────────────────────

/// Pack 16-bit RGB samples down to 8 bits each (the high byte of each sample).
#[cfg(any_standard_encode)]
#[allow(dead_code)] // unused when only `dng-encode` is enabled
fn pack_rgb8(image: &RgbImage) -> Vec<u8> {
    image.data().iter().map(|&p| (p >> 8) as u8).collect()
}

/// Validate a bit-depth request for a backend that only emits 8-bit output.
///
/// `Eight` and `Sixteen` are both accepted (`Sixteen` is down-converted, as
/// before); deeper requests yield [`EncodeError::UnsupportedBitDepth`].
#[cfg(any_standard_encode)]
#[allow(dead_code)] // unused when only `dng-encode` is enabled
fn check_8bit_backend(bit_depth: BitDepth, format: &'static str) -> Result<(), EncodeError> {
    match bit_depth {
        BitDepth::Eight | BitDepth::Sixteen => Ok(()),
        _ => Err(EncodeError::UnsupportedBitDepth {
            format,
            requested: bit_depth,
        }),
    }
}

// ── PNG ───────────────────────────────────────────────────────────────────────

#[cfg(feature = "png-encode")]
fn encode_png(
    image: &RgbImage,
    metadata: &ImageMetadata,
    cfg: &super::export::PngEncodeConfig,
) -> RawResult<Vec<u8>> {
    use super::export::{PngCompressionLevel, PngFilterStrategy, PngFilterType};
    use crate::metadata::exif::ExifBuilder;
    use crate::metadata::icc::IccProfile;
    use gamut_core::{Dimensions, EncodeImage, ImageRef, Rgb8, Rgb16};
    use gamut_png::{FilterStrategy, FilterType, Level, PngEncoder};

    let encoding_error = |e: gamut_core::Error| {
        RawError::Encode(EncodeError::Encoding {
            format: "PNG",
            message: format!("PNG encoding error: {e}"),
        })
    };

    let dims = Dimensions::new(image.width(), image.height()).map_err(encoding_error)?;

    let level = match cfg.compression {
        PngCompressionLevel::Store => Level::Store,
        PngCompressionLevel::Fast => Level::Fast,
        PngCompressionLevel::Default => Level::Default,
        PngCompressionLevel::Best => Level::Best,
    };
    let filter = match cfg.filter {
        PngFilterStrategy::None => FilterStrategy::None,
        PngFilterStrategy::Fixed(t) => FilterStrategy::Fixed(match t {
            PngFilterType::None => FilterType::None,
            PngFilterType::Sub => FilterType::Sub,
            PngFilterType::Up => FilterType::Up,
            PngFilterType::Average => FilterType::Average,
            PngFilterType::Paeth => FilterType::Paeth,
        }),
        PngFilterStrategy::MinSumAbs => FilterStrategy::MinSumAbs,
        PngFilterStrategy::BruteForce => FilterStrategy::BruteForce,
    };

    let mut encoder = PngEncoder::new()
        .with_compression(level)
        .with_filter(filter)
        .with_auto_reduce(cfg.auto_reduce);

    // Metadata is embedded by the encoder itself (eXIf / iCCP / XMP iTXt
    // chunks), so it is configured up front — no post-hoc chunk muxing.
    let m = &cfg.common.metadata;
    if m.embed_icc {
        // "ICC Profile" is the conventional iCCP profile name.
        encoder = encoder.with_icc_profile("ICC Profile", IccProfile::srgb().as_bytes());
    }
    if m.embed_exif {
        match ExifBuilder::new(metadata).build_bytes() {
            Ok(bytes) => encoder = encoder.with_exif(&bytes),
            Err(e) => tracing::warn!("Failed to embed EXIF in PNG: {e}"),
        }
    }
    if m.embed_xmp
        && let Some(xmp_data) = &metadata.xmp
    {
        match std::str::from_utf8(xmp_data) {
            Ok(xmp) => encoder = encoder.with_xmp(xmp),
            Err(e) => tracing::warn!("Failed to embed XMP in PNG (not valid UTF-8): {e}"),
        }
    }

    // PNG genuinely supports 8- and 16-bit output; gamut-png encodes Rgb16
    // directly (serialising big-endian itself), so no byte packing is needed.
    let mut output = Vec::new();
    match cfg.common.bit_depth {
        BitDepth::Eight => {
            let samples = pack_rgb8(image);
            let img = ImageRef::<Rgb8>::new(&samples, dims).map_err(encoding_error)?;
            encoder
                .encode_image(img, &mut output)
                .map_err(encoding_error)?;
        }
        BitDepth::Sixteen => {
            let img = ImageRef::<Rgb16>::new(image.data(), dims).map_err(encoding_error)?;
            encoder
                .encode_image(img, &mut output)
                .map_err(encoding_error)?;
        }
        other => {
            return Err(RawError::Encode(EncodeError::UnsupportedBitDepth {
                format: "PNG",
                requested: other,
            }));
        }
    }

    Ok(output)
}

// ── JPEG ──────────────────────────────────────────────────────────────────────

#[cfg(feature = "jpeg-encode")]
fn encode_jpeg(
    image: &RgbImage,
    metadata: &ImageMetadata,
    cfg: &super::export::JpegEncEncodeConfig,
) -> RawResult<Vec<u8>> {
    use crate::metadata::exif::ExifBuilder;
    use crate::metadata::icc::IccProfile;
    use jpeg_encoder::{ColorType, Encoder};

    check_8bit_backend(cfg.common.bit_depth, "JPEG")?;
    let data_8bit = pack_rgb8(image);

    let quality = if cfg.quality == 0 { 90 } else { cfg.quality };
    let mut jpeg_buf = Vec::new();
    let encoder = Encoder::new(&mut jpeg_buf, quality);
    encoder.encode(
        &data_8bit,
        image.width() as u16,
        image.height() as u16,
        ColorType::Rgb,
    )?;

    let m = &cfg.common.metadata;
    if m.embed_exif {
        let exif_builder = ExifBuilder::new(metadata);
        match exif_builder.append_to_jpeg(jpeg_buf.clone()) {
            Ok(data) => jpeg_buf = data,
            Err(e) => tracing::warn!("Failed to embed EXIF in JPEG: {e}"),
        }
    }
    if m.embed_icc {
        match IccProfile::srgb().append_to_jpeg(jpeg_buf.clone()) {
            Ok(data) => jpeg_buf = data,
            Err(e) => tracing::warn!("Failed to embed ICC in JPEG: {e}"),
        }
    }
    if m.embed_xmp
        && let Some(xmp_data) = &metadata.xmp
    {
        use crate::metadata::xmp::append_xmp_to_jpeg;
        match append_xmp_to_jpeg(xmp_data, jpeg_buf.clone()) {
            Ok(data) => jpeg_buf = data,
            Err(e) => tracing::warn!("Failed to embed XMP in JPEG: {e}"),
        }
    }

    Ok(jpeg_buf)
}

// ── JPEG (jpegli) ───────────────────────────────────────────────────────────────

#[cfg(feature = "jpeg-encode-jpegli")]
fn encode_jpeg_jpegli(
    image: &RgbImage,
    metadata: &ImageMetadata,
    cfg: &super::export::JpegliEncodeConfig,
) -> RawResult<Vec<u8>> {
    use super::export::JpegSubsampling;
    use crate::codecs::jpegli::{self, JpegliEncodeParams, Subsampling};
    use crate::metadata::exif::ExifBuilder;
    use crate::metadata::icc::IccProfile;

    // jpegli output is always an 8-bit JPEG, but it can quantise from 16-bit
    // input to reduce banding. `Eight` is packed; `Sixteen` is passed through
    // native-endian (matching the wrapper's `bits_per_sample == 16` contract);
    // deeper requests are unsupported.
    let (samples, bits_per_sample) = match cfg.common.bit_depth {
        BitDepth::Eight => (pack_rgb8(image), 8u32),
        BitDepth::Sixteen => {
            let mut bytes = Vec::with_capacity(image.data().len() * 2);
            for &sample in image.data() {
                bytes.extend_from_slice(&sample.to_ne_bytes());
            }
            (bytes, 16u32)
        }
        other => {
            return Err(RawError::Encode(EncodeError::UnsupportedBitDepth {
                format: "JPEG",
                requested: other,
            }));
        }
    };

    let params = JpegliEncodeParams {
        quality: cfg.quality,
        distance: cfg.distance,
        progressive: cfg.progressive,
        xyb: cfg.xyb,
        subsampling: match cfg.subsampling {
            JpegSubsampling::Yuv420 => Subsampling::Yuv420,
            JpegSubsampling::Yuv422 => Subsampling::Yuv422,
            JpegSubsampling::Yuv444 => Subsampling::Yuv444,
        },
    };

    let mut jpeg_buf = jpegli::encode(
        &samples,
        image.width(),
        image.height(),
        bits_per_sample,
        &params,
    )
    .map_err(|e| RawError::Encode(EncodeError::Jpegli(e)))?;

    // Metadata embedding mirrors the `encode_jpeg` path exactly.
    let m = &cfg.common.metadata;
    if m.embed_exif {
        let exif_builder = ExifBuilder::new(metadata);
        match exif_builder.append_to_jpeg(jpeg_buf.clone()) {
            Ok(data) => jpeg_buf = data,
            Err(e) => tracing::warn!("Failed to embed EXIF in JPEG: {e}"),
        }
    }
    if m.embed_icc {
        match IccProfile::srgb().append_to_jpeg(jpeg_buf.clone()) {
            Ok(data) => jpeg_buf = data,
            Err(e) => tracing::warn!("Failed to embed ICC in JPEG: {e}"),
        }
    }
    if m.embed_xmp
        && let Some(xmp_data) = &metadata.xmp
    {
        use crate::metadata::xmp::append_xmp_to_jpeg;
        match append_xmp_to_jpeg(xmp_data, jpeg_buf.clone()) {
            Ok(data) => jpeg_buf = data,
            Err(e) => tracing::warn!("Failed to embed XMP in JPEG: {e}"),
        }
    }

    Ok(jpeg_buf)
}

// ── WebP ──────────────────────────────────────────────────────────────────────

#[cfg(feature = "webp-encode")]
fn encode_webp(
    image: &RgbImage,
    metadata: &ImageMetadata,
    cfg: &super::export::LibwebpEncodeConfig,
) -> RawResult<Vec<u8>> {
    use crate::codecs::webp::{build_webp_config, encode_webp_rgb, mux_webp};
    use crate::metadata::exif::ExifBuilder;
    use crate::metadata::icc::IccProfile;

    check_8bit_backend(cfg.common.bit_depth, "WebP")?;

    let lossless = cfg.mode == WebPMode::Lossless;
    let config = build_webp_config(lossless, cfg.quality, cfg.method, cfg.near_lossless)
        .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

    let data_8bit = pack_rgb8(image);
    let encoded = encode_webp_rgb(&data_8bit, image.width(), image.height(), &config)
        .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

    let m = &cfg.common.metadata;
    let exif_bytes = if m.embed_exif {
        match ExifBuilder::new(metadata).build_bytes() {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                tracing::warn!("Failed to build EXIF for WebP: {e}");
                None
            }
        }
    } else {
        None
    };
    let icc_bytes = if m.embed_icc {
        Some(IccProfile::srgb().as_bytes().to_vec())
    } else {
        None
    };
    let xmp_bytes = if m.embed_xmp {
        metadata.xmp.as_deref()
    } else {
        None
    };

    mux_webp(
        &encoded,
        exif_bytes.as_deref(),
        icc_bytes.as_deref(),
        xmp_bytes,
    )
    .map_err(|e| RawError::Encode(EncodeError::WebP(e)))
}

// ── AVIF ──────────────────────────────────────────────────────────────────────

#[cfg(feature = "avif-encode")]
fn encode_avif(
    image: &RgbImage,
    metadata: &ImageMetadata,
    cfg: &super::export::AvifEncodeConfig,
) -> RawResult<Vec<u8>> {
    use crate::metadata::exif::ExifBuilder;
    use crate::metadata::icc::IccProfile;
    use crate::metadata::xmp::append_xmp_to_avif;
    use gamut_avif::AvifEncoder;
    use gamut_core::{Dimensions, EncodeImage, ImageRef, Rgb8};

    // gamut-avif takes 8-bit RGB: `Eight` and `Sixteen` are accepted
    // (`Sixteen` is down-converted, as with every 8-bit-only backend).
    // 10/12-bit AVIF output is temporarily unavailable — it is pending
    // high-bit-depth support in gamut-avif (justin13888/gamut#251) — so those
    // requests are reported rather than silently degraded.
    match cfg.common.bit_depth {
        BitDepth::Eight | BitDepth::Sixteen => {}
        other => {
            return Err(RawError::Encode(EncodeError::UnsupportedBitDepth {
                format: "AVIF (10/12-bit output pending justin13888/gamut#251)",
                requested: other,
            }));
        }
    }

    let encoding_error = |e: gamut_core::Error| {
        RawError::Encode(EncodeError::Encoding {
            format: "AVIF",
            message: format!("AVIF encoding error: {e}"),
        })
    };

    let dims = Dimensions::new(image.width(), image.height()).map_err(encoding_error)?;
    let samples = pack_rgb8(image);
    let img = ImageRef::<Rgb8>::new(&samples, dims).map_err(encoding_error)?;

    let encoder = if cfg.lossless {
        AvifEncoder::lossless()
    } else {
        AvifEncoder::lossy(cfg.quality)
    };

    // Encode failures are domain errors, never panics — this runs on a worker
    // pool and a failed target must be reported, not crash the process.
    let mut avif_bytes = Vec::new();
    encoder
        .encode_image(img, &mut avif_bytes)
        .map_err(encoding_error)?;

    // gamut-avif does not emit metadata items yet (deferred upstream), so
    // EXIF / ICC / XMP are spliced into the encoded container as ISOBMFF
    // items by rawshift's own muxer (`metadata::isobmff::insert_item`).
    let m = &cfg.common.metadata;
    if m.embed_icc {
        match IccProfile::srgb().append_to_avif(avif_bytes.clone()) {
            Ok(data) => avif_bytes = data,
            Err(e) => tracing::warn!("Failed to embed ICC in AVIF: {e}"),
        }
    }
    if m.embed_exif {
        match ExifBuilder::new(metadata).append_to_avif(avif_bytes.clone()) {
            Ok(data) => avif_bytes = data,
            Err(e) => tracing::warn!("Failed to embed EXIF in AVIF: {e}"),
        }
    }
    if m.embed_xmp
        && let Some(xmp_data) = &metadata.xmp
    {
        match append_xmp_to_avif(xmp_data, avif_bytes.clone()) {
            Ok(data) => avif_bytes = data,
            Err(e) => tracing::warn!("Failed to embed XMP in AVIF: {e}"),
        }
    }

    Ok(avif_bytes)
}

// ── JPEG XL ───────────────────────────────────────────────────────────────────

#[cfg(feature = "jxl-encode")]
fn encode_jxl(
    image: &RgbImage,
    metadata: &ImageMetadata,
    cfg: &super::export::JxlEncodeConfig,
) -> RawResult<Vec<u8>> {
    use crate::metadata::exif::ExifBuilder;
    use crate::metadata::icc::IccProfile;
    use gamut_core::{Dimensions, EncodeImage, ImageRef, Rgb8, Rgb16};
    use gamut_jxl::{ColorSpec, Container, Distance, Effort, JxlEncoder};

    let encoding_error = |e: gamut_core::Error| {
        RawError::Encode(EncodeError::Encoding {
            format: "JXL",
            message: format!("JXL encoding error: {e}"),
        })
    };

    let dims = Dimensions::new(image.width(), image.height()).map_err(encoding_error)?;

    let mut encoder = if cfg.lossless {
        JxlEncoder::lossless()
    } else {
        JxlEncoder::lossy(Distance::new(cfg.distance).map_err(encoding_error)?)
    };
    let effort = Effort::from_level(cfg.effort).ok_or_else(|| {
        RawError::Encode(EncodeError::Encoding {
            format: "JXL",
            message: format!("invalid JXL effort level {} (expected 1..=10)", cfg.effort),
        })
    })?;
    encoder = encoder.with_effort(effort);
    if let Some(bits) = cfg.coded_bit_depth {
        encoder = encoder.with_bit_depth(bits);
    }

    // Metadata is written by the encoder itself: EXIF and XMP become container
    // boxes (which forces ISO BMFF framing — gamut-jxl refuses metadata on a
    // bare codestream), and the ICC profile is carried in the codestream's
    // colour metadata rather than a sidecar box.
    let m = &cfg.common.metadata;
    if m.embed_icc {
        encoder = encoder.with_color(ColorSpec::Icc(IccProfile::srgb().as_bytes().to_vec()));
    }
    let mut needs_container = cfg.use_container;
    if m.embed_exif {
        match ExifBuilder::new(metadata).build_bytes() {
            Ok(bytes) => {
                encoder = encoder.with_exif(&bytes);
                needs_container = true;
            }
            Err(e) => tracing::warn!("Failed to embed EXIF in JXL: {e}"),
        }
    }
    if m.embed_xmp
        && let Some(xmp_data) = &metadata.xmp
    {
        match std::str::from_utf8(xmp_data) {
            Ok(xmp) => {
                encoder = encoder.with_xmp(xmp);
                needs_container = true;
            }
            Err(e) => tracing::warn!("Failed to embed XMP in JXL (not valid UTF-8): {e}"),
        }
    }
    if needs_container {
        encoder = encoder.with_container(Container::IsoBmff);
    }

    // JXL genuinely supports 8- and 16-bit input; gamut-jxl encodes Rgb16
    // directly (true 16-bit, lossless-capable), so no byte packing is needed.
    let mut output = Vec::new();
    match cfg.common.bit_depth {
        BitDepth::Eight => {
            let samples = pack_rgb8(image);
            let img = ImageRef::<Rgb8>::new(&samples, dims).map_err(encoding_error)?;
            encoder
                .encode_image(img, &mut output)
                .map_err(encoding_error)?;
        }
        BitDepth::Sixteen => {
            let img = ImageRef::<Rgb16>::new(image.data(), dims).map_err(encoding_error)?;
            encoder
                .encode_image(img, &mut output)
                .map_err(encoding_error)?;
        }
        other => {
            return Err(RawError::Encode(EncodeError::UnsupportedBitDepth {
                format: "JXL",
                requested: other,
            }));
        }
    }

    Ok(output)
}
