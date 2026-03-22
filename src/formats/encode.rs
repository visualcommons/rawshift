//! Standard image format encode functions.
//!
//! Extracted from `formats/mod.rs` to separate standard-format encode gates
//! (`png-encode`, `jpeg-encode`, etc.) from RAW format dispatch gates
//! (`arw`, `cr2`, etc.).

use std::path::Path;

use crate::core::image::RgbImage;
use crate::core::metadata::ImageMetadata;
use crate::error::{EncodeError, RawError, RawResult};

use super::export::EncodeOptions;
#[cfg(feature = "dng")]
use super::export_dng;

/// Encode a linear RGB image to a file with optional EXIF/ICC metadata.
///
/// This is a standalone function extracted from `RawFile::export()` so that
/// tests and callers can encode synthetic or pre-decoded images without going
/// through the full RAW decode pipeline.
///
/// `image` must contain 16-bit scene-linear RGB data normalized to [0, 65535].
/// Call `apply_tonemap` first if the image hasn't been tone-mapped yet.
pub fn encode_rgb_image(
    image: &RgbImage,
    metadata: &ImageMetadata,
    path: &Path,
    encode_options: &EncodeOptions,
) -> RawResult<()> {
    match encode_options {
        #[cfg(feature = "png-encode")]
        EncodeOptions::Png(opts) => {
            use zune_core::colorspace::ColorSpace;
            use zune_core::options::EncoderOptions;
            use zune_png::PngEncoder;

            let options = EncoderOptions::default()
                .set_width(image.width() as usize)
                .set_height(image.height() as usize)
                .set_colorspace(ColorSpace::RGB)
                .set_depth(opts.bit_depth);

            let data_bytes = if opts.bit_depth == zune_core::bit_depth::BitDepth::Sixteen {
                let mut bytes = Vec::with_capacity(image.data.len() * 2);
                for &pixel in &image.data {
                    bytes.extend_from_slice(&pixel.to_be_bytes());
                }
                bytes
            } else {
                let mut bytes = Vec::with_capacity(image.data.len());
                for &pixel in &image.data {
                    bytes.push((pixel >> 8) as u8);
                }
                bytes
            };

            let mut encoder = PngEncoder::new(&data_bytes, options);
            let mut output = Vec::new();
            encoder.encode(&mut output).map_err(|e| {
                RawError::Encode(EncodeError::Encoding {
                    format: "PNG",
                    message: format!("PNG encoding error: {:?}", e),
                })
            })?;

            if opts.metadata.embed_exif || opts.metadata.embed_icc || opts.metadata.embed_xmp {
                use crate::metadata::exif::ExifBuilder;
                use crate::metadata::icc::IccProfile;
                use img_parts::png::{Png, PngChunk};
                use img_parts::{Bytes, ImageEXIF, ImageICC};

                match Png::from_bytes(Bytes::from(output.clone())) {
                    Ok(mut png) => {
                        if opts.metadata.embed_icc {
                            let icc = IccProfile::srgb();
                            png.set_icc_profile(Some(Bytes::from(icc.as_bytes().to_vec())));
                        }
                        if opts.metadata.embed_exif {
                            let exif_builder = ExifBuilder::new(metadata);
                            match exif_builder.build_bytes() {
                                Ok(bytes) => png.set_exif(Some(Bytes::from(bytes))),
                                Err(e) => tracing::warn!("Failed to embed EXIF in PNG: {}", e),
                            }
                        }
                        if opts.metadata.embed_xmp {
                            if let Some(xmp_data) = &metadata.xmp {
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
                        }
                        use std::io::Cursor;
                        let mut buf = Cursor::new(Vec::new());
                        match png.encoder().write_to(&mut buf) {
                            Ok(_) => output = buf.into_inner(),
                            Err(e) => tracing::warn!("Failed to write PNG with metadata: {}", e),
                        }
                    }
                    Err(e) => tracing::warn!("Failed to parse PNG for metadata embedding: {}", e),
                }
            }

            let mut file = std::fs::File::create(path)?;
            use std::io::Write;
            file.write_all(&output)?;
        }
        #[cfg(feature = "jpeg-encode")]
        EncodeOptions::Jpeg(opts) => {
            use crate::metadata::exif::ExifBuilder;
            use crate::metadata::icc::IccProfile;
            use jpeg_encoder::{ColorType, Encoder};

            let mut data_8bit = Vec::with_capacity(image.data.len());
            for &pixel in &image.data {
                data_8bit.push((pixel >> 8) as u8);
            }

            let quality = if opts.quality == 0 { 90 } else { opts.quality };
            let encoder = Encoder::new_file(path, quality)?;
            encoder.encode(
                &data_8bit,
                image.width() as u16,
                image.height() as u16,
                ColorType::Rgb,
            )?;

            if opts.metadata.embed_exif || opts.metadata.embed_icc || opts.metadata.embed_xmp {
                let mut jpeg_data = std::fs::read(path)?;

                if opts.metadata.embed_exif {
                    let exif_builder = ExifBuilder::new(metadata);
                    match exif_builder.append_to_jpeg(jpeg_data.clone()) {
                        Ok(data) => jpeg_data = data,
                        Err(e) => tracing::warn!("Failed to embed EXIF: {}", e),
                    }
                }

                if opts.metadata.embed_icc {
                    let icc = IccProfile::srgb();
                    match icc.append_to_jpeg(jpeg_data.clone()) {
                        Ok(data) => jpeg_data = data,
                        Err(e) => tracing::warn!("Failed to embed ICC: {}", e),
                    }
                }

                if opts.metadata.embed_xmp {
                    if let Some(xmp_data) = &metadata.xmp {
                        use crate::metadata::xmp::append_xmp_to_jpeg;
                        match append_xmp_to_jpeg(xmp_data, jpeg_data.clone()) {
                            Ok(data) => jpeg_data = data,
                            Err(e) => tracing::warn!("Failed to embed XMP in JPEG: {}", e),
                        }
                    }
                }

                std::fs::write(path, jpeg_data)?;
            }
        }
        #[cfg(feature = "webp-encode")]
        EncodeOptions::WebP(opts) => {
            use crate::codecs::webp::{build_webp_config, encode_webp_rgb, mux_webp};
            use crate::formats::export::WebPMode;
            use crate::metadata::exif::ExifBuilder;
            use crate::metadata::icc::IccProfile;

            let lossless = opts.mode == WebPMode::Lossless;
            let config = build_webp_config(lossless, opts.quality, opts.method, opts.near_lossless)
                .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            let mut data_8bit = Vec::with_capacity(image.data.len());
            for &pixel in &image.data {
                data_8bit.push((pixel >> 8) as u8);
            }

            let encoded = encode_webp_rgb(&data_8bit, image.width(), image.height(), &config)
                .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            let exif_bytes = if opts.metadata.embed_exif {
                let exif_builder = ExifBuilder::new(metadata);
                match exif_builder.build_bytes() {
                    Ok(bytes) => Some(bytes),
                    Err(e) => {
                        tracing::warn!("Failed to build EXIF for WebP: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            let icc_bytes = if opts.metadata.embed_icc {
                Some(IccProfile::srgb().as_bytes().to_vec())
            } else {
                None
            };

            let xmp_bytes = if opts.metadata.embed_xmp {
                metadata.xmp.as_deref()
            } else {
                None
            };

            let output = mux_webp(
                &encoded,
                exif_bytes.as_deref(),
                icc_bytes.as_deref(),
                xmp_bytes,
            )
            .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            std::fs::write(path, output)?;
        }
        #[cfg(feature = "avif-encode")]
        EncodeOptions::Avif(opts) => {
            use crate::metadata::exif::ExifBuilder;
            use ravif::{Encoder, Img, RGBA8};

            let rgba_data: Vec<RGBA8> = image
                .data
                .chunks(3)
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
                .with_quality(opts.quality as f32)
                .with_speed(opts.speed);

            let result = encoder.encode_rgba(img).expect("Encode AVIF");
            let mut avif_bytes = result.avif_file;

            if opts.metadata.embed_icc {
                use crate::metadata::icc::IccProfile;
                match IccProfile::srgb().append_to_avif(avif_bytes.clone()) {
                    Ok(data) => avif_bytes = data,
                    Err(e) => tracing::warn!("Failed to embed ICC in AVIF: {}", e),
                }
            }

            std::fs::write(path, avif_bytes)?;

            if opts.metadata.embed_exif {
                let exif_builder = ExifBuilder::new(metadata);
                if let Err(e) = exif_builder.append_to_avif_file(path) {
                    tracing::warn!("Failed to embed EXIF in AVIF: {}", e);
                }
            }

            if opts.metadata.embed_xmp {
                if let Some(xmp_data) = &metadata.xmp {
                    use crate::metadata::xmp::append_xmp_to_avif_file;
                    if let Err(e) = append_xmp_to_avif_file(path, xmp_data) {
                        tracing::warn!("Failed to embed XMP in AVIF: {}", e);
                    }
                }
            }
        }
        #[cfg(feature = "jxl-encode")]
        EncodeOptions::Jxl(opts) => {
            use zune_core::colorspace::ColorSpace;
            use zune_core::options::EncoderOptions;
            use zune_jpegxl::JxlSimpleEncoder;

            let data_8bit: Vec<u8> = image.data.iter().map(|&p| (p >> 8) as u8).collect();

            let quality = if opts.quality == 0.0 {
                100
            } else {
                opts.quality as u8
            };
            let enc_options = EncoderOptions::default()
                .set_width(image.width() as usize)
                .set_height(image.height() as usize)
                .set_colorspace(ColorSpace::RGB)
                .set_quality(quality);

            let encoder = JxlSimpleEncoder::new(&data_8bit, enc_options);
            let mut encoded: Vec<u8> = Vec::new();
            encoder.encode(&mut encoded).expect("Encode JXL");
            std::fs::write(path, &encoded)?;

            if opts.metadata.embed_exif {
                use crate::metadata::exif::ExifBuilder;
                let exif_builder = ExifBuilder::new(metadata);
                if let Err(e) = exif_builder.append_to_jxl_file(path) {
                    tracing::warn!("Failed to embed EXIF in JXL: {}", e);
                }
            }

            if opts.metadata.embed_icc {
                use crate::metadata::icc::IccProfile;
                match std::fs::read(path) {
                    Ok(jxl_bytes) => match IccProfile::srgb().append_to_jxl(jxl_bytes) {
                        Ok(data) => {
                            if let Err(e) = std::fs::write(path, data) {
                                tracing::warn!("Failed to write JXL with ICC: {}", e);
                            }
                        }
                        Err(e) => tracing::warn!("Failed to embed ICC in JXL: {}", e),
                    },
                    Err(e) => tracing::warn!("Failed to read JXL for ICC embedding: {}", e),
                }
            }

            if opts.metadata.embed_xmp {
                if let Some(xmp_data) = &metadata.xmp {
                    use crate::metadata::xmp::append_xmp_to_jxl;
                    match std::fs::read(path) {
                        Ok(jxl_bytes) => match append_xmp_to_jxl(xmp_data, jxl_bytes) {
                            Ok(data) => {
                                if let Err(e) = std::fs::write(path, data) {
                                    tracing::warn!("Failed to write JXL with XMP: {}", e);
                                }
                            }
                            Err(e) => tracing::warn!("Failed to embed XMP in JXL: {}", e),
                        },
                        Err(e) => tracing::warn!("Failed to read JXL for XMP embedding: {}", e),
                    }
                }
            }
        }
        #[cfg(feature = "dng")]
        EncodeOptions::Dng(config) => {
            export_dng(path, image, metadata, config)?;
        }
        #[allow(unreachable_patterns)]
        _ => {
            return Err(RawError::Unsupported(
                "This encode format is not available with the current feature flags.".to_string(),
            ));
        }
    }

    Ok(())
}

/// Encode a linear RGB image to a writer with optional EXIF/ICC metadata.
///
/// Like [`encode_rgb_image`] but writes to any `Write` implementor instead of a file path.
/// Currently supports PNG, JPEG, and WebP. For formats requiring post-processing
/// (DNG, AVIF, JXL), use [`encode_rgb_image`] with a file path.
pub fn encode_rgb_image_to_writer<W: std::io::Write>(
    image: &RgbImage,
    metadata: &ImageMetadata,
    writer: &mut W,
    encode_options: &EncodeOptions,
) -> RawResult<()> {
    match encode_options {
        #[cfg(feature = "png-encode")]
        EncodeOptions::Png(opts) => {
            use zune_core::colorspace::ColorSpace;
            use zune_core::options::EncoderOptions;
            use zune_png::PngEncoder;

            let options = EncoderOptions::default()
                .set_width(image.width() as usize)
                .set_height(image.height() as usize)
                .set_colorspace(ColorSpace::RGB)
                .set_depth(opts.bit_depth);

            let data_bytes = if opts.bit_depth == zune_core::bit_depth::BitDepth::Sixteen {
                let mut bytes = Vec::with_capacity(image.data.len() * 2);
                for &pixel in &image.data {
                    bytes.extend_from_slice(&pixel.to_be_bytes());
                }
                bytes
            } else {
                let mut bytes = Vec::with_capacity(image.data.len());
                for &pixel in &image.data {
                    bytes.push((pixel >> 8) as u8);
                }
                bytes
            };

            let mut encoder = PngEncoder::new(&data_bytes, options);
            let mut output = Vec::new();
            encoder.encode(&mut output).map_err(|e| {
                RawError::Encode(EncodeError::Encoding {
                    format: "PNG",
                    message: format!("PNG encoding error: {:?}", e),
                })
            })?;

            if opts.metadata.embed_exif || opts.metadata.embed_icc || opts.metadata.embed_xmp {
                use crate::metadata::exif::ExifBuilder;
                use crate::metadata::icc::IccProfile;
                use img_parts::png::{Png, PngChunk};
                use img_parts::{Bytes, ImageEXIF, ImageICC};

                match Png::from_bytes(Bytes::from(output.clone())) {
                    Ok(mut png) => {
                        if opts.metadata.embed_icc {
                            let icc = IccProfile::srgb();
                            png.set_icc_profile(Some(Bytes::from(icc.as_bytes().to_vec())));
                        }
                        if opts.metadata.embed_exif {
                            let exif_builder = ExifBuilder::new(metadata);
                            match exif_builder.build_bytes() {
                                Ok(bytes) => png.set_exif(Some(Bytes::from(bytes))),
                                Err(e) => tracing::warn!("Failed to embed EXIF in PNG: {}", e),
                            }
                        }
                        if opts.metadata.embed_xmp {
                            if let Some(xmp_data) = &metadata.xmp {
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
                        }
                        use std::io::Cursor;
                        let mut buf = Cursor::new(Vec::new());
                        match png.encoder().write_to(&mut buf) {
                            Ok(_) => output = buf.into_inner(),
                            Err(e) => tracing::warn!("Failed to write PNG with metadata: {}", e),
                        }
                    }
                    Err(e) => tracing::warn!("Failed to parse PNG for metadata embedding: {}", e),
                }
            }

            writer.write_all(&output)?;
        }
        #[cfg(feature = "jpeg-encode")]
        EncodeOptions::Jpeg(opts) => {
            use crate::metadata::exif::ExifBuilder;
            use crate::metadata::icc::IccProfile;
            use jpeg_encoder::{ColorType, Encoder};

            let mut data_8bit = Vec::with_capacity(image.data.len());
            for &pixel in &image.data {
                data_8bit.push((pixel >> 8) as u8);
            }

            let quality = if opts.quality == 0 { 90 } else { opts.quality };
            let mut jpeg_buf = Vec::new();
            let encoder = Encoder::new(&mut jpeg_buf, quality);
            encoder.encode(
                &data_8bit,
                image.width() as u16,
                image.height() as u16,
                ColorType::Rgb,
            )?;

            if opts.metadata.embed_exif {
                let exif_builder = ExifBuilder::new(metadata);
                match exif_builder.append_to_jpeg(jpeg_buf.clone()) {
                    Ok(data) => jpeg_buf = data,
                    Err(e) => tracing::warn!("Failed to embed EXIF: {}", e),
                }
            }

            if opts.metadata.embed_icc {
                let icc = IccProfile::srgb();
                match icc.append_to_jpeg(jpeg_buf.clone()) {
                    Ok(data) => jpeg_buf = data,
                    Err(e) => tracing::warn!("Failed to embed ICC: {}", e),
                }
            }

            if opts.metadata.embed_xmp {
                if let Some(xmp_data) = &metadata.xmp {
                    use crate::metadata::xmp::append_xmp_to_jpeg;
                    match append_xmp_to_jpeg(xmp_data, jpeg_buf.clone()) {
                        Ok(data) => jpeg_buf = data,
                        Err(e) => tracing::warn!("Failed to embed XMP in JPEG: {}", e),
                    }
                }
            }

            writer.write_all(&jpeg_buf)?;
        }
        #[cfg(feature = "webp-encode")]
        EncodeOptions::WebP(opts) => {
            use crate::codecs::webp::{build_webp_config, encode_webp_rgb, mux_webp};
            use crate::formats::export::WebPMode;
            use crate::metadata::exif::ExifBuilder;
            use crate::metadata::icc::IccProfile;

            let lossless = opts.mode == WebPMode::Lossless;
            let config = build_webp_config(lossless, opts.quality, opts.method, opts.near_lossless)
                .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            let mut data_8bit = Vec::with_capacity(image.data.len());
            for &pixel in &image.data {
                data_8bit.push((pixel >> 8) as u8);
            }

            let encoded = encode_webp_rgb(&data_8bit, image.width(), image.height(), &config)
                .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            let exif_bytes = if opts.metadata.embed_exif {
                let exif_builder = ExifBuilder::new(metadata);
                match exif_builder.build_bytes() {
                    Ok(bytes) => Some(bytes),
                    Err(e) => {
                        tracing::warn!("Failed to build EXIF for WebP: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            let icc_bytes = if opts.metadata.embed_icc {
                Some(IccProfile::srgb().as_bytes().to_vec())
            } else {
                None
            };

            let xmp_bytes = if opts.metadata.embed_xmp {
                metadata.xmp.as_deref()
            } else {
                None
            };

            let output = mux_webp(
                &encoded,
                exif_bytes.as_deref(),
                icc_bytes.as_deref(),
                xmp_bytes,
            )
            .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            writer.write_all(&output)?;
        }
        #[allow(unreachable_patterns)]
        _ => {
            return Err(RawError::Unsupported(
                "This format does not support writing to a generic writer. Use encode_rgb_image() with a file path.".to_string(),
            ));
        }
    }

    Ok(())
}
