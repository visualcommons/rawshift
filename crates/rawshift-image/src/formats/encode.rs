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
use crate::core::image::RgbImage;
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
        EncodeOptions::PngZune(cfg) => encode_png(image, metadata, cfg),
        #[cfg(feature = "jpeg-encode")]
        EncodeOptions::JpegJpegEnc(cfg) => encode_jpeg(image, metadata, cfg),
        #[cfg(feature = "jpeg-encode-jpegli")]
        EncodeOptions::JpegJpegli(cfg) => encode_jpeg_jpegli(image, metadata, cfg),
        #[cfg(feature = "webp-encode")]
        EncodeOptions::WebpLibwebp(cfg) => encode_webp(image, metadata, cfg),
        #[cfg(feature = "avif-encode")]
        EncodeOptions::AvifRavif(cfg) => encode_avif(image, metadata, cfg),
        #[cfg(feature = "jxl-encode")]
        EncodeOptions::JxlZune(cfg) => encode_jxl(image, metadata, cfg),
        #[cfg(feature = "jxl-encode-libjxl")]
        EncodeOptions::JxlLibjxl(cfg) => encode_jxl_libjxl(image, metadata, cfg),
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
    image.data.iter().map(|&p| (p >> 8) as u8).collect()
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
    cfg: &super::export::ZunePngEncodeConfig,
) -> RawResult<Vec<u8>> {
    use zune_core::colorspace::ColorSpace as ZuneColorSpace;
    use zune_core::options::EncoderOptions;
    use zune_png::PngEncoder;

    // PNG genuinely supports 8- and 16-bit output.
    let (data_bytes, depth) = match cfg.common.bit_depth {
        BitDepth::Eight => (pack_rgb8(image), zune_core::bit_depth::BitDepth::Eight),
        BitDepth::Sixteen => {
            let mut bytes = Vec::with_capacity(image.data.len() * 2);
            for &pixel in &image.data {
                bytes.extend_from_slice(&pixel.to_be_bytes());
            }
            (bytes, zune_core::bit_depth::BitDepth::Sixteen)
        }
        other => {
            return Err(RawError::Encode(EncodeError::UnsupportedBitDepth {
                format: "PNG",
                requested: other,
            }));
        }
    };

    let options = EncoderOptions::default()
        .set_width(image.width() as usize)
        .set_height(image.height() as usize)
        .set_colorspace(ZuneColorSpace::RGB)
        .set_depth(depth);

    let mut encoder = PngEncoder::new(&data_bytes, options);
    let mut output = Vec::new();
    encoder.encode(&mut output).map_err(|e| {
        RawError::Encode(EncodeError::Encoding {
            format: "PNG",
            message: format!("PNG encoding error: {e:?}"),
        })
    })?;

    let m = &cfg.common.metadata;
    if m.embed_exif || m.embed_icc || m.embed_xmp {
        use crate::metadata::exif::ExifBuilder;
        use crate::metadata::icc::IccProfile;
        use img_parts::png::{Png, PngChunk};
        use img_parts::{Bytes, ImageEXIF, ImageICC};

        match Png::from_bytes(Bytes::from(output.clone())) {
            Ok(mut png) => {
                if m.embed_icc {
                    let icc = IccProfile::srgb();
                    png.set_icc_profile(Some(Bytes::from(icc.as_bytes().to_vec())));
                }
                if m.embed_exif {
                    let exif_builder = ExifBuilder::new(metadata);
                    match exif_builder.build_bytes() {
                        Ok(bytes) => png.set_exif(Some(Bytes::from(bytes))),
                        Err(e) => tracing::warn!("Failed to embed EXIF in PNG: {e}"),
                    }
                }
                if m.embed_xmp
                    && let Some(xmp_data) = &metadata.xmp
                {
                    // iTXt: keyword\0 + compression_flag + compression_method
                    //       + language_tag\0 + translated_keyword\0 + text
                    let mut chunk_data = Vec::with_capacity(22 + xmp_data.len());
                    chunk_data.extend_from_slice(b"XML:com.adobe.xmp\0");
                    chunk_data.push(0); // compression_flag = 0
                    chunk_data.push(0); // compression_method = 0
                    chunk_data.push(0); // language_tag (empty)
                    chunk_data.push(0); // translated_keyword (empty)
                    chunk_data.extend_from_slice(xmp_data);
                    let chunk = PngChunk::new(*b"iTXt", Bytes::from(chunk_data));
                    let idx = png.chunks().len().saturating_sub(1);
                    png.chunks_mut().insert(idx, chunk);
                }
                use std::io::Cursor;
                let mut buf = Cursor::new(Vec::new());
                match png.encoder().write_to(&mut buf) {
                    Ok(_) => output = buf.into_inner(),
                    Err(e) => tracing::warn!("Failed to write PNG with metadata: {e}"),
                }
            }
            Err(e) => tracing::warn!("Failed to parse PNG for metadata embedding: {e}"),
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
            let mut bytes = Vec::with_capacity(image.data.len() * 2);
            for &sample in &image.data {
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
    cfg: &super::export::RavifEncodeConfig,
) -> RawResult<Vec<u8>> {
    use crate::metadata::exif::ExifBuilder;
    use crate::metadata::icc::IccProfile;
    use crate::metadata::xmp::append_xmp_to_avif;
    use ravif::{Encoder, Img, RGBA8};

    check_8bit_backend(cfg.common.bit_depth, "AVIF")?;

    let rgba_data: Vec<RGBA8> = image
        .data
        .chunks_exact(3)
        .map(|rgb| {
            RGBA8::new(
                (rgb[0] >> 8) as u8,
                (rgb[1] >> 8) as u8,
                (rgb[2] >> 8) as u8,
                255,
            )
        })
        .collect();

    let img = Img::new(
        rgba_data.as_slice(),
        image.width() as usize,
        image.height() as usize,
    );

    let encoder = Encoder::new()
        .with_quality(cfg.quality as f32)
        .with_speed(cfg.speed);

    // Encode failures are domain errors, never panics — this runs on a worker
    // pool and a failed target must be reported, not crash the process.
    let result = encoder.encode_rgba(img).map_err(|e| {
        RawError::Encode(EncodeError::Encoding {
            format: "AVIF",
            message: format!("{e:?}"),
        })
    })?;
    let mut avif_bytes = result.avif_file;

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
    cfg: &super::export::ZuneJxlEncodeConfig,
) -> RawResult<Vec<u8>> {
    use crate::metadata::exif::ExifBuilder;
    use crate::metadata::icc::IccProfile;
    use crate::metadata::xmp::append_xmp_to_jxl;
    use zune_core::colorspace::ColorSpace as ZuneColorSpace;
    use zune_core::options::EncoderOptions;
    use zune_jpegxl::JxlSimpleEncoder;

    check_8bit_backend(cfg.common.bit_depth, "JXL")?;
    let data_8bit = pack_rgb8(image);

    let quality = if cfg.quality == 0.0 {
        100
    } else {
        cfg.quality as u8
    };
    let enc_options = EncoderOptions::default()
        .set_width(image.width() as usize)
        .set_height(image.height() as usize)
        .set_colorspace(ZuneColorSpace::RGB)
        .set_quality(quality);

    let encoder = JxlSimpleEncoder::new(&data_8bit, enc_options);
    let mut encoded: Vec<u8> = Vec::new();
    encoder.encode(&mut encoded).map_err(|e| {
        RawError::Encode(EncodeError::Encoding {
            format: "JXL",
            message: format!("{e:?}"),
        })
    })?;

    let m = &cfg.common.metadata;
    if m.embed_exif {
        match ExifBuilder::new(metadata).append_to_jxl(encoded.clone()) {
            Ok(data) => encoded = data,
            Err(e) => tracing::warn!("Failed to embed EXIF in JXL: {e}"),
        }
    }
    if m.embed_icc {
        match IccProfile::srgb().append_to_jxl(encoded.clone()) {
            Ok(data) => encoded = data,
            Err(e) => tracing::warn!("Failed to embed ICC in JXL: {e}"),
        }
    }
    if m.embed_xmp
        && let Some(xmp_data) = &metadata.xmp
    {
        match append_xmp_to_jxl(xmp_data, encoded.clone()) {
            Ok(data) => encoded = data,
            Err(e) => tracing::warn!("Failed to embed XMP in JXL: {e}"),
        }
    }

    Ok(encoded)
}

// ── JPEG XL (libjxl reference encoder) ──────────────────────────────────────────

#[cfg(feature = "jxl-encode-libjxl")]
fn encode_jxl_libjxl(
    image: &RgbImage,
    metadata: &ImageMetadata,
    cfg: &super::export::LibjxlEncodeConfig,
) -> RawResult<Vec<u8>> {
    use super::export::{LibjxlColorTransform, LibjxlModular};
    use crate::codecs::jxl_libjxl::{self, JxlEncodeParams};
    use crate::metadata::exif::ExifBuilder;
    use crate::metadata::icc::IccProfile;
    use crate::metadata::xmp::append_xmp_to_jxl;

    // Bit depth → packed sample bytes. Unlike the 8-bit-only backends, libjxl
    // genuinely encodes 16-bit, so `Sixteen` is passed through (native-endian to
    // match the `JXL_NATIVE_ENDIAN` pixel format the wrapper requests).
    let (samples, bits_per_sample) = match cfg.common.bit_depth {
        BitDepth::Eight => (pack_rgb8(image), 8u32),
        BitDepth::Sixteen => {
            let mut bytes = Vec::with_capacity(image.data.len() * 2);
            for &sample in &image.data {
                bytes.extend_from_slice(&sample.to_ne_bytes());
            }
            (bytes, 16u32)
        }
        other => {
            return Err(RawError::Encode(EncodeError::UnsupportedBitDepth {
                format: "JXL",
                requested: other,
            }));
        }
    };

    // Resolve the typed config to libjxl's `-1`/`0`/`1`… frame-setting values.
    let modular = match cfg.modular {
        LibjxlModular::Auto => -1,
        LibjxlModular::VarDct => 0,
        LibjxlModular::Modular => 1,
    };
    let color_transform = match cfg.color_transform {
        LibjxlColorTransform::Auto => -1,
        LibjxlColorTransform::Xyb => 0,
        LibjxlColorTransform::None => 1,
        LibjxlColorTransform::YCbCr => 2,
    };
    let tri = |o: Option<bool>| match o {
        None => -1,
        Some(false) => 0,
        Some(true) => 1,
    };

    let params = JxlEncodeParams {
        distance: cfg.distance,
        quality: cfg.quality,
        lossless: cfg.lossless,
        effort: i64::from(cfg.effort),
        brotli_effort: i64::from(cfg.brotli_effort),
        decoding_speed: i64::from(cfg.decoding_speed),
        progressive: cfg.progressive,
        modular,
        color_transform,
        epf: i64::from(cfg.epf),
        gaborish: tri(cfg.gaborish),
        noise: tri(cfg.noise),
        dots: tri(cfg.dots),
        patches: tri(cfg.patches),
        photon_noise_iso: cfg.photon_noise_iso,
        resampling: i64::from(cfg.resampling),
        use_container: cfg.use_container,
        codestream_level: i32::from(cfg.codestream_level),
        extra_int_options: cfg.extra_int_options.clone(),
        extra_float_options: cfg.extra_float_options.clone(),
    };

    let mut encoded = jxl_libjxl::encode(
        &samples,
        image.width(),
        image.height(),
        bits_per_sample,
        &params,
    )
    .map_err(|e| RawError::Encode(EncodeError::Jxl(e)))?;

    // Metadata embedding mirrors the zune `encode_jxl` path exactly.
    let m = &cfg.common.metadata;
    if m.embed_exif {
        match ExifBuilder::new(metadata).append_to_jxl(encoded.clone()) {
            Ok(data) => encoded = data,
            Err(e) => tracing::warn!("Failed to embed EXIF in JXL: {e}"),
        }
    }
    if m.embed_icc {
        match IccProfile::srgb().append_to_jxl(encoded.clone()) {
            Ok(data) => encoded = data,
            Err(e) => tracing::warn!("Failed to embed ICC in JXL: {e}"),
        }
    }
    if m.embed_xmp
        && let Some(xmp_data) = &metadata.xmp
    {
        match append_xmp_to_jxl(xmp_data, encoded.clone()) {
            Ok(data) => encoded = data,
            Err(e) => tracing::warn!("Failed to embed XMP in JXL: {e}"),
        }
    }

    Ok(encoded)
}
