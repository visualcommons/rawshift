//! Standard image format decoders (JPEG, PNG, WebP, TIFF, JXL, AVIF).
//!
//! This module provides decoders for common non-RAW image formats that decode
//! directly to RGB pixel data stored in an [`RgbImage`].

use std::io::{BufReader, Cursor};

use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_core::result::DecodingResult;

use crate::core::image::RgbImage;
use crate::error::{RawError, RawResult};

/// Supported standard (non-RAW) image formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardFormat {
    /// GIF (Graphics Interchange Format)
    Gif,
    /// JPEG / JFIF
    Jpeg,
    /// PNG (Portable Network Graphics)
    Png,
    /// WebP
    WebP,
    /// JPEG XL
    Jxl,
    /// TIFF
    Tiff,
    /// AVIF
    Avif,
}

impl StandardFormat {
    /// Human-readable name of the format.
    pub fn name(self) -> &'static str {
        match self {
            StandardFormat::Gif => "GIF",
            StandardFormat::Jpeg => "JPEG",
            StandardFormat::Png => "PNG",
            StandardFormat::WebP => "WebP",
            StandardFormat::Jxl => "JXL",
            StandardFormat::Tiff => "TIFF",
            StandardFormat::Avif => "AVIF",
        }
    }
}

/// Detect a standard image format from the first bytes of image data.
///
/// Returns `None` if the format is not recognised or if `data` is too short.
pub fn detect_standard_format(data: &[u8]) -> Option<StandardFormat> {
    if data.len() < 8 {
        return None;
    }
    match data {
        // GIF87a / GIF89a
        d if d.len() >= 6 && (&d[0..6] == b"GIF87a" || &d[0..6] == b"GIF89a") => {
            Some(StandardFormat::Gif)
        }
        // JPEG: FF D8 FF
        d if d[0] == 0xFF && d[1] == 0xD8 && d[2] == 0xFF => Some(StandardFormat::Jpeg),
        // PNG: 89 50 4E 47 0D 0A 1A 0A
        d if &d[0..8] == b"\x89PNG\r\n\x1a\n" => Some(StandardFormat::Png),
        // WebP: RIFF????WEBP
        d if d.len() >= 12 && &d[0..4] == b"RIFF" && &d[8..12] == b"WEBP" => {
            Some(StandardFormat::WebP)
        }
        // JXL bare codestream: FF 0A
        d if d[0] == 0xFF && d[1] == 0x0A => Some(StandardFormat::Jxl),
        // JXL ISO BMFF container: box size (4 bytes) + "JXL " (4 bytes)
        d if d.len() >= 12 && &d[4..8] == b"JXL " => Some(StandardFormat::Jxl),
        // TIFF little-endian (II) or big-endian (MM)
        d if (d[0] == 0x49 && d[1] == 0x49 && d[2] == 0x2A && d[3] == 0x00)
            || (d[0] == 0x4D && d[1] == 0x4D && d[2] == 0x00 && d[3] == 0x2A) =>
        {
            Some(StandardFormat::Tiff)
        }
        // AVIF / HEIF: ftyp box with avif/avis/mif1 brand
        d if d.len() >= 12
            && &d[4..8] == b"ftyp"
            && (&d[8..12] == b"avif" || &d[8..12] == b"avis" || &d[8..12] == b"mif1") =>
        {
            Some(StandardFormat::Avif)
        }
        _ => None,
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Scale an 8-bit sample to 16-bit by duplicating the byte in both halves.
/// This is equivalent to `v * 257` and ensures that 255 maps to 65535.
#[inline(always)]
fn u8_to_u16(v: u8) -> u16 {
    (v as u16) * 257
}

// ── GIF ──────────────────────────────────────────────────────────────────────

fn decode_gif(data: &[u8]) -> RawResult<RgbImage> {
    use gif::{ColorOutput, DecodeOptions};

    let mut opts = DecodeOptions::new();
    opts.set_color_output(ColorOutput::RGBA);
    let mut decoder =
        opts.read_info(Cursor::new(data))
            .map_err(|e| RawError::ImageDecodeError {
                format: "GIF",
                message: e.to_string(),
            })?;

    let canvas_width = decoder.width() as u32;
    let canvas_height = decoder.height() as u32;

    let frame = decoder
        .read_next_frame()
        .map_err(|e| RawError::ImageDecodeError {
            format: "GIF",
            message: e.to_string(),
        })?
        .ok_or_else(|| RawError::ImageDecodeError {
            format: "GIF",
            message: "no frames in GIF".to_string(),
        })?;

    let frame_width = frame.width as usize;
    let frame_height = frame.height as usize;
    let frame_left = frame.left as usize;
    let frame_top = frame.top as usize;

    // Allocate a black canvas and composite the frame (RGBA → RGB, drop alpha).
    let mut out = vec![0u16; (canvas_width as usize) * (canvas_height as usize) * 3];

    let buf = &frame.buffer[..];
    // With ColorOutput::RGBA the buffer contains 4 bytes per pixel.
    let expected_rgba = frame_width * frame_height * 4;
    if buf.len() < expected_rgba {
        return Err(RawError::ImageDecodeError {
            format: "GIF",
            message: format!(
                "frame buffer too small: got {} bytes, expected {} ({}x{}x4)",
                buf.len(),
                expected_rgba,
                frame_width,
                frame_height,
            ),
        });
    }

    for row in 0..frame_height {
        let canvas_y = frame_top + row;
        if canvas_y >= canvas_height as usize {
            break;
        }
        for col in 0..frame_width {
            let canvas_x = frame_left + col;
            if canvas_x >= canvas_width as usize {
                continue;
            }
            let src = (row * frame_width + col) * 4;
            let dst = (canvas_y * canvas_width as usize + canvas_x) * 3;
            out[dst] = u8_to_u16(buf[src]);
            out[dst + 1] = u8_to_u16(buf[src + 1]);
            out[dst + 2] = u8_to_u16(buf[src + 2]);
            // Alpha channel (buf[src + 3]) is intentionally dropped.
        }
    }

    Ok(RgbImage::new(canvas_width, canvas_height, out))
}

// ── JPEG ─────────────────────────────────────────────────────────────────────

fn decode_jpeg(data: &[u8]) -> RawResult<RgbImage> {
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let cursor = ZCursor::new(data);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(cursor, opts);

    let pixels = decoder.decode().map_err(|e| RawError::ImageDecodeError {
        format: "JPEG",
        message: format!("{e:?}"),
    })?;

    let (w, h) = decoder
        .dimensions()
        .map(|(w, h)| (w as u32, h as u32))
        .ok_or_else(|| RawError::ImageDecodeError {
            format: "JPEG",
            message: "could not read image dimensions after decode".to_string(),
        })?;

    // pixels is Vec<u8>, RGB interleaved — scale to u16
    let data_u16: Vec<u16> = pixels.iter().map(|&v| u8_to_u16(v)).collect();

    Ok(RgbImage::new(w, h, data_u16))
}

// ── PNG ──────────────────────────────────────────────────────────────────────

fn decode_png(data: &[u8]) -> RawResult<RgbImage> {
    // Decode the PNG in its native color space (RGB, RGBA, Luma, LumaA, …)
    // and then convert to packed RGB u16 manually.
    let opts = DecoderOptions::default().png_set_strip_to_8bit(false);
    let cursor = ZCursor::new(data);
    let mut decoder = zune_png::PngDecoder::new_with_options(cursor, opts);

    let result = decoder.decode().map_err(|e| RawError::ImageDecodeError {
        format: "PNG",
        message: format!("{e:?}"),
    })?;

    let info = decoder.info().ok_or_else(|| RawError::ImageDecodeError {
        format: "PNG",
        message: "could not read PNG info after decode".to_string(),
    })?;

    let w = info.width as u32;
    let h = info.height as u32;

    // Determine the output colorspace from the decoder
    let colorspace = decoder.colorspace().unwrap_or(ColorSpace::RGB);
    let n = colorspace.num_components();

    // Convert raw samples to Vec<u16>
    let samples_u16: Vec<u16> = match result {
        DecodingResult::U8(px) => px.iter().map(|&v| u8_to_u16(v)).collect(),
        DecodingResult::U16(px) => px,
        _ => {
            return Err(RawError::ImageDecodeError {
                format: "PNG",
                message: "unexpected pixel depth in decoded result".to_string(),
            });
        }
    };

    // Convert any colorspace to packed RGB u16
    let data_u16: Vec<u16> = match colorspace {
        ColorSpace::RGB => samples_u16,
        ColorSpace::RGBA => {
            // Drop alpha channel (index 3)
            samples_u16
                .chunks_exact(n)
                .flat_map(|px| [px[0], px[1], px[2]])
                .collect()
        }
        ColorSpace::Luma => {
            // Expand grayscale to RGB
            samples_u16.iter().flat_map(|&v| [v, v, v]).collect()
        }
        ColorSpace::LumaA => {
            // Expand grayscale+alpha to RGB (drop alpha)
            samples_u16
                .chunks_exact(n)
                .flat_map(|px| [px[0], px[0], px[0]])
                .collect()
        }
        _ => {
            return Err(RawError::ImageDecodeError {
                format: "PNG",
                message: format!("unsupported PNG colorspace: {colorspace:?}"),
            });
        }
    };

    Ok(RgbImage::new(w, h, data_u16))
}

// ── WebP ─────────────────────────────────────────────────────────────────────

fn decode_webp(data: &[u8]) -> RawResult<RgbImage> {
    let cursor = BufReader::new(Cursor::new(data));
    let mut decoder =
        image_webp::WebPDecoder::new(cursor).map_err(|e| RawError::ImageDecodeError {
            format: "WebP",
            message: format!("{e}"),
        })?;

    let (w, h) = decoder.dimensions();
    let has_alpha = decoder.has_alpha();
    let bytes_per_pixel: usize = if has_alpha { 4 } else { 3 };

    let buf_size = decoder
        .output_buffer_size()
        .ok_or_else(|| RawError::ImageDecodeError {
            format: "WebP",
            message: "image too large to fit in memory".to_string(),
        })?;

    let mut raw_pixels = vec![0u8; buf_size];
    decoder
        .read_image(&mut raw_pixels)
        .map_err(|e| RawError::ImageDecodeError {
            format: "WebP",
            message: format!("{e}"),
        })?;

    // Strip alpha channel if present, converting to plain RGB
    let data_u16: Vec<u16> = if has_alpha {
        raw_pixels
            .chunks_exact(bytes_per_pixel)
            .flat_map(|px| [u8_to_u16(px[0]), u8_to_u16(px[1]), u8_to_u16(px[2])])
            .collect()
    } else {
        raw_pixels.iter().map(|&v| u8_to_u16(v)).collect()
    };

    Ok(RgbImage::new(w, h, data_u16))
}

// ── JXL ──────────────────────────────────────────────────────────────────────

fn decode_jxl(data: &[u8]) -> RawResult<RgbImage> {
    use jxl_oxide::{JxlImage, PixelFormat};

    let image =
        JxlImage::builder()
            .read(Cursor::new(data))
            .map_err(|e| RawError::ImageDecodeError {
                format: "JXL",
                message: format!("{e}"),
            })?;

    let w = image.width();
    let h = image.height();

    if image.num_loaded_keyframes() == 0 {
        return Err(RawError::ImageDecodeError {
            format: "JXL",
            message: "no keyframes decoded".to_string(),
        });
    }

    let render = image
        .render_frame(0)
        .map_err(|e| RawError::ImageDecodeError {
            format: "JXL",
            message: format!("{e}"),
        })?;

    let pixel_format = image.pixel_format();
    let total_pixels = (w as usize) * (h as usize);

    // Use stream_no_alpha so we get exactly the color channels
    let mut stream = render.stream_no_alpha();
    let channels = stream.channels() as usize;

    let total_samples = total_pixels * channels;
    let mut samples_u16 = vec![0u16; total_samples];
    stream.write_to_buffer(&mut samples_u16);

    let data_u16: Vec<u16> = match pixel_format {
        PixelFormat::Rgb => samples_u16,
        PixelFormat::Gray => {
            // Expand grayscale to RGB
            samples_u16.iter().flat_map(|&v| [v, v, v]).collect()
        }
        PixelFormat::Rgba | PixelFormat::Graya => {
            // Drop alpha; if grayscale expand to RGB
            if pixel_format == PixelFormat::Graya {
                samples_u16
                    .chunks_exact(channels)
                    .flat_map(|px| [px[0], px[0], px[0]])
                    .collect()
            } else {
                // RGBA — keep only RGB
                samples_u16
                    .chunks_exact(channels)
                    .flat_map(|px| [px[0], px[1], px[2]])
                    .collect()
            }
        }
        _ => {
            return Err(RawError::ImageDecodeError {
                format: "JXL",
                message: format!("unsupported pixel format {pixel_format:?}"),
            });
        }
    };

    Ok(RgbImage::new(w, h, data_u16))
}

// ── TIFF ─────────────────────────────────────────────────────────────────────

fn decode_tiff(_data: &[u8]) -> RawResult<RgbImage> {
    // No general TIFF decode crate is available in the dependency tree.
    // (zune-image does not include a TIFF decoder; tiff/image crates are not
    // listed as dependencies.)
    // The project's own TIFF parser (`src/tiff/`) is focused on metadata and
    // TIFF-based RAW formats, not generic RGB TIFF images.
    Err(RawError::UnsupportedFormat(
        "Standard TIFF decoding is not yet implemented. \
         Add the `tiff` crate as a dependency to enable this."
            .to_string(),
    ))
}

// ── AVIF ─────────────────────────────────────────────────────────────────────

fn decode_avif(_data: &[u8]) -> RawResult<RgbImage> {
    // `ravif` (the only AVIF crate in Cargo.toml) is an *encoder* only.
    // There is no AVIF *decoder* dependency available.
    Err(RawError::UnsupportedFormat(
        "AVIF decoding is not yet implemented. \
         Add the `libavif` or `avif-decode` crate as a dependency to enable this."
            .to_string(),
    ))
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Decode a standard (non-RAW) image to an [`RgbImage`].
///
/// The caller must supply the [`StandardFormat`] explicitly. Use
/// [`detect_standard_format`] to infer it from magic bytes when the format is
/// not otherwise known.
///
/// The returned [`RgbImage`] contains 16-bit interleaved RGB data in row-major
/// order. 8-bit source images are scaled to 16-bit by multiplying by 257.
///
/// # Errors
/// Returns [`RawError::ImageDecodeError`] on decode failure, or
/// [`RawError::UnsupportedFormat`] for formats without a decoder.
pub fn decode_standard_image(data: &[u8], format: StandardFormat) -> RawResult<RgbImage> {
    match format {
        StandardFormat::Gif => decode_gif(data),
        StandardFormat::Jpeg => decode_jpeg(data),
        StandardFormat::Png => decode_png(data),
        StandardFormat::WebP => decode_webp(data),
        StandardFormat::Jxl => decode_jxl(data),
        StandardFormat::Tiff => decode_tiff(data),
        StandardFormat::Avif => decode_avif(data),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_standard_format ────────────────────────────────────────────

    #[test]
    fn detect_gif89a() {
        let magic = *b"GIF89a\x01\x00";
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Gif));
    }

    #[test]
    fn detect_gif87a() {
        let magic = *b"GIF87a\x01\x00";
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Gif));
    }

    #[test]
    fn detect_gif_non_gif_returns_none() {
        let magic = *b"GIF99z\x01\x00";
        assert_ne!(detect_standard_format(&magic), Some(StandardFormat::Gif));
    }

    #[test]
    fn detect_jpeg() {
        let magic = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46];
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Jpeg));
    }

    #[test]
    fn detect_png() {
        let magic = *b"\x89PNG\r\n\x1a\n";
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Png));
    }

    #[test]
    fn detect_webp() {
        let mut magic = [0u8; 12];
        magic[0..4].copy_from_slice(b"RIFF");
        magic[8..12].copy_from_slice(b"WEBP");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::WebP));
    }

    #[test]
    fn detect_jxl_bare() {
        let magic = [0xFF, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Jxl));
    }

    #[test]
    fn detect_jxl_container() {
        let mut magic = [0u8; 12];
        // bytes 4..8 must be "JXL "
        magic[4..8].copy_from_slice(b"JXL ");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Jxl));
    }

    #[test]
    fn detect_tiff_le() {
        let magic = [0x49, 0x49, 0x2A, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Tiff));
    }

    #[test]
    fn detect_tiff_be() {
        let magic = [0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08];
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Tiff));
    }

    #[test]
    fn detect_avif() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"avif");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Avif));
    }

    #[test]
    fn detect_avif_avis() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"avis");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Avif));
    }

    #[test]
    fn detect_unknown_returns_none() {
        let magic = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
        assert_eq!(detect_standard_format(&magic), None);
    }

    #[test]
    fn detect_too_short_returns_none() {
        assert_eq!(detect_standard_format(&[0xFF, 0xD8]), None);
    }

    // ── StandardFormat ────────────────────────────────────────────────────

    #[test]
    fn standard_format_name() {
        assert_eq!(StandardFormat::Gif.name(), "GIF");
        assert_eq!(StandardFormat::Jpeg.name(), "JPEG");
        assert_eq!(StandardFormat::Png.name(), "PNG");
        assert_eq!(StandardFormat::WebP.name(), "WebP");
        assert_eq!(StandardFormat::Jxl.name(), "JXL");
        assert_eq!(StandardFormat::Tiff.name(), "TIFF");
        assert_eq!(StandardFormat::Avif.name(), "AVIF");
    }

    #[test]
    fn standard_format_variants_exist() {
        let _variants = [
            StandardFormat::Gif,
            StandardFormat::Jpeg,
            StandardFormat::Png,
            StandardFormat::WebP,
            StandardFormat::Jxl,
            StandardFormat::Tiff,
            StandardFormat::Avif,
        ];
    }

    // ── u8_to_u16 ────────────────────────────────────────────────────────

    #[test]
    fn u8_to_u16_min_max() {
        assert_eq!(u8_to_u16(0), 0);
        assert_eq!(u8_to_u16(255), 65535);
        assert_eq!(u8_to_u16(1), 257);
        assert_eq!(u8_to_u16(128), 32896);
    }

    // ── JPEG roundtrip ────────────────────────────────────────────────────

    #[test]
    fn jpeg_roundtrip_dimensions() {
        // Encode a 4x4 RGB image to JPEG, then decode it and check dimensions.
        const W: u16 = 4;
        const H: u16 = 4;
        let pixels: Vec<u8> = (0..(W as usize * H as usize * 3))
            .map(|i| (i * 17 % 256) as u8)
            .collect();

        let mut encoded = Vec::new();
        let encoder = jpeg_encoder::Encoder::new(&mut encoded, 95);
        encoder
            .encode(&pixels, W, H, jpeg_encoder::ColorType::Rgb)
            .expect("JPEG encode failed");

        let decoded =
            decode_standard_image(&encoded, StandardFormat::Jpeg).expect("JPEG decode failed");

        assert_eq!(decoded.width, W as u32);
        assert_eq!(decoded.height, H as u32);
        assert_eq!(decoded.data.len(), W as usize * H as usize * 3);
    }

    // ── PNG roundtrip ─────────────────────────────────────────────────────

    #[test]
    fn png_roundtrip_dimensions() {
        // Encode a 4x4 8-bit RGB image to PNG, then decode it and check
        // dimensions and that data contains 16-bit samples.
        const W: usize = 4;
        const H: usize = 4;
        let pixels_u8: Vec<u8> = (0..(W * H * 3)).map(|i| (i * 13 % 256) as u8).collect();

        let opts = zune_core::options::EncoderOptions::default()
            .set_width(W)
            .set_height(H)
            .set_colorspace(ColorSpace::RGB);
        let mut encoded = Vec::new();
        let mut encoder = zune_png::PngEncoder::new(&pixels_u8, opts);
        encoder.encode(&mut encoded).expect("PNG encode failed");

        let decoded =
            decode_standard_image(&encoded, StandardFormat::Png).expect("PNG decode failed");

        assert_eq!(decoded.width, W as u32);
        assert_eq!(decoded.height, H as u32);
        assert_eq!(decoded.data.len(), W * H * 3);
        // Each u8 value should have been scaled to u16
        assert_eq!(decoded.data[0], u8_to_u16(pixels_u8[0]));
    }

    // ── detect + decode consistency ───────────────────────────────────────

    #[test]
    fn detect_then_decode_jpeg() {
        const W: u16 = 2;
        const H: u16 = 2;
        let pixels = vec![
            100u8, 150u8, 200u8, 50u8, 75u8, 100u8, 200u8, 220u8, 240u8, 10u8, 20u8, 30u8,
        ];
        let mut encoded = Vec::new();
        jpeg_encoder::Encoder::new(&mut encoded, 90)
            .encode(&pixels, W, H, jpeg_encoder::ColorType::Rgb)
            .unwrap();

        let fmt = detect_standard_format(&encoded);
        assert_eq!(fmt, Some(StandardFormat::Jpeg));
        let img = decode_standard_image(&encoded, fmt.unwrap()).unwrap();
        assert_eq!(img.width, W as u32);
        assert_eq!(img.height, H as u32);
    }

    // ── GIF decode ────────────────────────────────────────────────────────

    /// Build a minimal but valid GIF89a file in memory.
    ///
    /// Creates a 2×2 image with 4 palette entries and distinct pixel colours:
    ///   index 0 → red  (255, 0, 0)
    ///   index 1 → green (0, 255, 0)
    ///   index 2 → blue  (0, 0, 255)
    ///   index 3 → white (255, 255, 255)
    fn make_minimal_gif() -> Vec<u8> {
        // We hand-craft the GIF binary so we don't need a separate encoder crate.
        // The LZW-compressed pixel data for indices [0, 1, 2, 3] with min_code_size=2
        // was pre-computed. This is a known-good 2×2 GIF.
        use gif::{Encoder, Frame, Repeat};
        use std::borrow::Cow;

        let palette: &[u8] = &[
            255, 0, 0, // 0: red
            0, 255, 0, // 1: green
            0, 0, 255, // 2: blue
            255, 255, 255, // 3: white
            0, 0, 0, // 4: black (padding to a power of 2)
            0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];

        let mut out: Vec<u8> = Vec::new();
        let mut encoder = Encoder::new(&mut out, 2, 2, palette).expect("gif encoder init");
        encoder.set_repeat(Repeat::Finite(0)).expect("set repeat");

        let mut frame = Frame::<'static>::default();
        frame.width = 2;
        frame.height = 2;
        // Pixel indices row-major: top-left=0, top-right=1, bottom-left=2, bottom-right=3
        frame.buffer = Cow::Owned(vec![0u8, 1, 2, 3]);
        encoder.write_frame(&frame).expect("write gif frame");
        drop(encoder);
        out
    }

    #[test]
    fn gif_decode_dimensions() {
        let gif_data = make_minimal_gif();
        let img =
            decode_standard_image(&gif_data, StandardFormat::Gif).expect("GIF decode must succeed");
        assert_eq!(img.width, 2, "decoded width must be 2");
        assert_eq!(img.height, 2, "decoded height must be 2");
        assert_eq!(
            img.data.len(),
            2 * 2 * 3,
            "must have 12 u16 samples (2×2×3)"
        );
    }

    #[test]
    fn gif_decode_detect_then_decode() {
        let gif_data = make_minimal_gif();
        let fmt = detect_standard_format(&gif_data);
        assert_eq!(
            fmt,
            Some(StandardFormat::Gif),
            "format detection must return GIF"
        );
        let img = decode_standard_image(&gif_data, fmt.unwrap()).unwrap();
        assert_eq!(img.width, 2);
        assert_eq!(img.height, 2);
    }

    #[test]
    fn gif_decode_first_pixel_is_red() {
        let gif_data = make_minimal_gif();
        let img = decode_standard_image(&gif_data, StandardFormat::Gif).unwrap();
        // Index 0 → red (255, 0, 0) → scaled to u16: (255*257, 0, 0)
        assert_eq!(img.data[0], u8_to_u16(255), "R of top-left pixel");
        assert_eq!(img.data[1], u8_to_u16(0), "G of top-left pixel");
        assert_eq!(img.data[2], u8_to_u16(0), "B of top-left pixel");
    }

    #[test]
    fn gif_decode_invalid_data_returns_error() {
        let junk = vec![0u8; 32];
        let result = decode_standard_image(&junk, StandardFormat::Gif);
        assert!(result.is_err(), "junk data must return an error");
    }

    // ── stub error paths ──────────────────────────────────────────────────

    #[test]
    fn tiff_returns_unsupported() {
        // Feed dummy TIFF magic bytes
        let magic = [0x49u8, 0x49, 0x2A, 0x00, 0, 0, 0, 8];
        let result = decode_standard_image(&magic, StandardFormat::Tiff);
        assert!(result.is_err());
        assert!(matches!(result, Err(RawError::UnsupportedFormat(_))));
    }

    #[test]
    fn avif_returns_unsupported() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"avif");
        let result = decode_standard_image(&magic, StandardFormat::Avif);
        assert!(result.is_err());
        assert!(matches!(result, Err(RawError::UnsupportedFormat(_))));
    }
}
