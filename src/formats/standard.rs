//! Standard image format decoders (JPEG, PNG, WebP, TIFF, JXL, AVIF, HEIC, SVG, APV).
//!
//! This module provides decoders for common non-RAW image formats that decode
//! directly to RGB pixel data stored in an [`RgbImage`].

use std::io::Cursor;

use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_core::result::DecodingResult;

use crate::core::image::RgbImage;
use crate::error::{FormatError, RawError, RawResult};

/// Supported standard (non-RAW) image formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// HEIC / HEIF (High Efficiency Image Container using H.265)
    Heic,
    /// SVG (Scalable Vector Graphics)
    Svg,
    /// APV (All-intra Predictive Video codec)
    Apv,
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
            StandardFormat::Heic => "HEIC",
            StandardFormat::Svg => "SVG",
            StandardFormat::Apv => "APV",
        }
    }

    /// Primary file extension (without dot).
    pub fn extension(self) -> &'static str {
        match self {
            StandardFormat::Gif => "gif",
            StandardFormat::Jpeg => "jpg",
            StandardFormat::Png => "png",
            StandardFormat::WebP => "webp",
            StandardFormat::Jxl => "jxl",
            StandardFormat::Tiff => "tiff",
            StandardFormat::Avif => "avif",
            StandardFormat::Heic => "heic",
            StandardFormat::Svg => "svg",
            StandardFormat::Apv => "apv",
        }
    }

    /// Look up a format by file extension (case-insensitive).
    ///
    /// Common aliases are supported (e.g. `jpeg`/`jpe`/`jfif` for JPEG,
    /// `tif` for TIFF, `heif` for HEIC, `svgz` for SVG).
    pub fn from_extension(ext: &str) -> Option<StandardFormat> {
        match ext.to_ascii_lowercase().as_str() {
            "gif" => Some(StandardFormat::Gif),
            "jpg" | "jpeg" | "jpe" | "jfif" => Some(StandardFormat::Jpeg),
            "png" => Some(StandardFormat::Png),
            "webp" => Some(StandardFormat::WebP),
            "jxl" => Some(StandardFormat::Jxl),
            "tiff" | "tif" => Some(StandardFormat::Tiff),
            "avif" => Some(StandardFormat::Avif),
            "heic" | "heif" => Some(StandardFormat::Heic),
            "svg" | "svgz" => Some(StandardFormat::Svg),
            "apv" => Some(StandardFormat::Apv),
            _ => None,
        }
    }

    /// Standard MIME type for this format.
    pub fn mime_type(self) -> &'static str {
        match self {
            StandardFormat::Gif => "image/gif",
            StandardFormat::Jpeg => "image/jpeg",
            StandardFormat::Png => "image/png",
            StandardFormat::WebP => "image/webp",
            StandardFormat::Jxl => "image/jxl",
            StandardFormat::Tiff => "image/tiff",
            StandardFormat::Avif => "image/avif",
            StandardFormat::Heic => "image/heic",
            StandardFormat::Svg => "image/svg+xml",
            StandardFormat::Apv => "video/apv",
        }
    }

    /// Whether this format can be decoded to an [`RgbImage`].
    pub fn supports_decode(self) -> bool {
        match self {
            StandardFormat::Gif
            | StandardFormat::Jpeg
            | StandardFormat::Png
            | StandardFormat::WebP
            | StandardFormat::Jxl
            | StandardFormat::Tiff => true,
            #[cfg(feature = "avif")]
            StandardFormat::Avif => true,
            #[cfg(feature = "svg")]
            StandardFormat::Svg => true,
            _ => false,
        }
    }

    /// Whether this format can be encoded from an [`RgbImage`].
    pub fn supports_encode(self) -> bool {
        match self {
            StandardFormat::Png | StandardFormat::Jpeg | StandardFormat::WebP => true,
            #[cfg(feature = "avif")]
            StandardFormat::Avif => true,
            #[cfg(feature = "jxl-encode")]
            StandardFormat::Jxl => true,
            _ => false,
        }
    }
}

impl std::fmt::Display for StandardFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Detect a standard image format from the first bytes of image data.
///
/// Note: TIFF-based RAW formats (DNG, ARW, NEF, CR2) share the TIFF magic
/// bytes and will be detected as `StandardFormat::Tiff`. Use [`RawFile::open()`]
/// to distinguish RAW formats first.
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
        // HEIC/HEIF: ftyp box with heic/heis/hevc/hevx brand
        d if d.len() >= 12
            && &d[4..8] == b"ftyp"
            && (&d[8..12] == b"heic"
                || &d[8..12] == b"heis"
                || &d[8..12] == b"hevc"
                || &d[8..12] == b"hevx") =>
        {
            Some(StandardFormat::Heic)
        }
        // APV: ftyp box with apv1/apvx brand
        d if d.len() >= 12
            && &d[4..8] == b"ftyp"
            && (&d[8..12] == b"apv1" || &d[8..12] == b"apvx") =>
        {
            Some(StandardFormat::Apv)
        }
        // SVG: XML starting with <?xml, <svg, or <!-- with <svg present in file
        d if d.len() >= 4
            && (&d[0..4] == b"<?xm" || &d[0..4] == b"<svg" || &d[0..4] == b"<!--") =>
        {
            if d.windows(4).any(|w| w == b"<svg") {
                Some(StandardFormat::Svg)
            } else {
                None
            }
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
    let mut decoder = opts.read_info(Cursor::new(data)).map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "GIF",
            message: e.to_string(),
        })
    })?;

    let canvas_width = decoder.width() as u32;
    let canvas_height = decoder.height() as u32;

    let frame = decoder
        .read_next_frame()
        .map_err(|e| {
            RawError::Format(FormatError::ImageDecode {
                format: "GIF",
                message: e.to_string(),
            })
        })?
        .ok_or_else(|| {
            RawError::Format(FormatError::ImageDecode {
                format: "GIF",
                message: "no frames in GIF".to_string(),
            })
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
        return Err(RawError::Format(FormatError::ImageDecode {
            format: "GIF",
            message: format!(
                "frame buffer too small: got {} bytes, expected {} ({}x{}x4)",
                buf.len(),
                expected_rgba,
                frame_width,
                frame_height,
            ),
        }));
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

    let pixels = decoder.decode().map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "JPEG",
            message: format!("{e:?}"),
        })
    })?;

    let (w, h) = decoder
        .dimensions()
        .map(|(w, h)| (w as u32, h as u32))
        .ok_or_else(|| {
            RawError::Format(FormatError::ImageDecode {
                format: "JPEG",
                message: "could not read image dimensions after decode".to_string(),
            })
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

    let result = decoder.decode().map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "PNG",
            message: format!("{e:?}"),
        })
    })?;

    let info = decoder.info().ok_or_else(|| {
        RawError::Format(FormatError::ImageDecode {
            format: "PNG",
            message: "could not read PNG info after decode".to_string(),
        })
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
            return Err(RawError::Format(FormatError::ImageDecode {
                format: "PNG",
                message: "unexpected pixel depth in decoded result".to_string(),
            }));
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
            return Err(RawError::Format(FormatError::ImageDecode {
                format: "PNG",
                message: format!("unsupported PNG colorspace: {colorspace:?}"),
            }));
        }
    };

    Ok(RgbImage::new(w, h, data_u16))
}

// ── WebP ─────────────────────────────────────────────────────────────────────

fn decode_webp(data: &[u8]) -> RawResult<RgbImage> {
    let (w, h, rgb) = crate::codecs::webp::decode_webp_rgb(data).map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "WebP",
            message: e,
        })
    })?;

    let data_u16: Vec<u16> = rgb.iter().map(|&v| u8_to_u16(v)).collect();

    Ok(RgbImage::new(w, h, data_u16))
}

// ── JXL ──────────────────────────────────────────────────────────────────────

fn decode_jxl(data: &[u8]) -> RawResult<RgbImage> {
    use jxl_oxide::{JxlImage, PixelFormat};

    let image = JxlImage::builder().read(Cursor::new(data)).map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "JXL",
            message: format!("{e}"),
        })
    })?;

    let w = image.width();
    let h = image.height();

    if image.num_loaded_keyframes() == 0 {
        return Err(RawError::Format(FormatError::ImageDecode {
            format: "JXL",
            message: "no keyframes decoded".to_string(),
        }));
    }

    let render = image.render_frame(0).map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "JXL",
            message: format!("{e}"),
        })
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
            return Err(RawError::Format(FormatError::ImageDecode {
                format: "JXL",
                message: format!("unsupported pixel format {pixel_format:?}"),
            }));
        }
    };

    Ok(RgbImage::new(w, h, data_u16))
}

// ── TIFF ─────────────────────────────────────────────────────────────────────

fn decode_tiff(data: &[u8]) -> RawResult<RgbImage> {
    use tiff::ColorType;
    use tiff::decoder::{Decoder, DecodingResult};

    let cursor = Cursor::new(data);
    let mut decoder = Decoder::new(cursor).map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "TIFF",
            message: format!("{e}"),
        })
    })?;

    let (w, h) = decoder.dimensions().map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "TIFF",
            message: format!("{e}"),
        })
    })?;

    let color_type = decoder.colortype().map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "TIFF",
            message: format!("{e}"),
        })
    })?;

    let result = decoder.read_image().map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "TIFF",
            message: format!("{e}"),
        })
    })?;

    // Extract raw samples as u16 values.
    let samples_u16: Vec<u16> = match result {
        DecodingResult::U8(px) => px.iter().map(|&v| u8_to_u16(v)).collect(),
        DecodingResult::U16(px) => px,
        DecodingResult::U32(px) => px.iter().map(|&v| (v >> 16) as u16).collect(),
        DecodingResult::F32(px) => px
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 65535.0) as u16)
            .collect(),
        _ => {
            return Err(RawError::Format(FormatError::ImageDecode {
                format: "TIFF",
                message: format!("unsupported TIFF sample type for color type {color_type:?}"),
            }));
        }
    };

    // Convert to interleaved RGB u16 based on the color type.
    let data_u16: Vec<u16> = match color_type {
        ColorType::RGB(_) => samples_u16,
        ColorType::RGBA(_) => samples_u16
            .chunks_exact(4)
            .flat_map(|px| [px[0], px[1], px[2]])
            .collect(),
        ColorType::Gray(_) => samples_u16.iter().flat_map(|&v| [v, v, v]).collect(),
        ColorType::GrayA(_) => samples_u16
            .chunks_exact(2)
            .flat_map(|px| [px[0], px[0], px[0]])
            .collect(),
        ColorType::CMYK(_) => {
            // Simple CMYK→RGB: R = (1-C)*(1-K), G = (1-M)*(1-K), B = (1-Y)*(1-K)
            samples_u16
                .chunks_exact(4)
                .flat_map(|px| {
                    let c = px[0] as f64 / 65535.0;
                    let m = px[1] as f64 / 65535.0;
                    let y = px[2] as f64 / 65535.0;
                    let k = px[3] as f64 / 65535.0;
                    let r = ((1.0 - c) * (1.0 - k) * 65535.0) as u16;
                    let g = ((1.0 - m) * (1.0 - k) * 65535.0) as u16;
                    let b = ((1.0 - y) * (1.0 - k) * 65535.0) as u16;
                    [r, g, b]
                })
                .collect()
        }
        _ => {
            return Err(RawError::Format(FormatError::ImageDecode {
                format: "TIFF",
                message: format!("unsupported TIFF color type: {color_type:?}"),
            }));
        }
    };

    Ok(RgbImage::new(w, h, data_u16))
}

// ── AVIF ─────────────────────────────────────────────────────────────────────

#[cfg(feature = "avif")]
fn decode_avif(data: &[u8]) -> RawResult<RgbImage> {
    use image::DynamicImage;
    use image::codecs::avif::AvifDecoder;

    let decoder = AvifDecoder::new(Cursor::new(data)).map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "AVIF",
            message: format!("{e}"),
        })
    })?;

    let img = DynamicImage::from_decoder(decoder).map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "AVIF",
            message: format!("{e}"),
        })
    })?;

    let rgb = img.into_rgb16();
    let w = rgb.width();
    let h = rgb.height();
    Ok(RgbImage::new(w, h, rgb.into_raw()))
}

#[cfg(not(feature = "avif"))]
fn decode_avif(_data: &[u8]) -> RawResult<RgbImage> {
    Err(RawError::Unsupported(
        "AVIF decoding requires the `avif` feature flag.".to_string(),
    ))
}

// ── HEIC ─────────────────────────────────────────────────────────────────────

fn decode_heic(_data: &[u8]) -> RawResult<RgbImage> {
    // HEIC uses the ISOBMFF container with H.265 (HEVC) compression.
    // H.265 decoding requires a licensed library and is not implemented here.
    Err(RawError::Format(FormatError::ImageDecode {
        format: "HEIC",
        message: "HEIC decoding requires a licensed H.265 decoder. \
                  Set the 'heic' feature flag and provide a compatible library."
            .to_string(),
    }))
}

// ── SVG ──────────────────────────────────────────────────────────────────────

#[cfg(feature = "svg")]
fn decode_svg(data: &[u8]) -> RawResult<RgbImage> {
    use resvg::{tiny_skia, usvg};

    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(data, &options).map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "SVG",
            message: e.to_string(),
        })
    })?;

    let pixmap_size = tree.size().to_int_size();
    let width = pixmap_size.width();
    let height = pixmap_size.height();

    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
        RawError::Format(FormatError::ImageDecode {
            format: "SVG",
            message: "Failed to create pixmap".to_string(),
        })
    })?;

    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    // pixmap contains RGBA u8 data; convert to RGB u16
    let rgba = pixmap.data();
    let data_u16: Vec<u16> = rgba
        .chunks_exact(4)
        .flat_map(|chunk| {
            [
                chunk[0] as u16 * 257, // R
                chunk[1] as u16 * 257, // G
                chunk[2] as u16 * 257, // B
            ]
        })
        .collect();

    Ok(RgbImage::new(width, height, data_u16))
}

#[cfg(not(feature = "svg"))]
fn decode_svg(_data: &[u8]) -> RawResult<RgbImage> {
    Err(RawError::Format(FormatError::ImageDecode {
        format: "SVG",
        message: "SVG support requires the 'svg' feature flag".to_string(),
    }))
}

// ── APV ──────────────────────────────────────────────────────────────────────

fn decode_apv(_data: &[u8]) -> RawResult<RgbImage> {
    // APV (All-intra Predictive Video codec) is an open format developed by Samsung.
    // No Rust decoder exists yet.
    Err(RawError::Format(FormatError::ImageDecode {
        format: "APV",
        message: "APV codec decoding is not yet implemented. \
                  The APV codec is an open format but no Rust decoder exists yet."
            .to_string(),
    }))
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
        StandardFormat::Heic => decode_heic(data),
        StandardFormat::Svg => decode_svg(data),
        StandardFormat::Apv => decode_apv(data),
    }
}

/// Extract EXIF metadata from a standard image without decoding pixel data.
///
/// Reads embedded EXIF from image file bytes and maps the tags to the unified
/// [`ImageMetadata`] type.  Returns a default (empty) [`ImageMetadata`] when
/// the format carries no EXIF or when the format is not supported for metadata
/// extraction (e.g. GIF, SVG, APV).
///
/// # Supported formats
/// | Format | Metadata source |
/// |--------|----------------|
/// | JPEG   | APP1 EXIF segment |
/// | TIFF   | IFD0 EXIF tags |
/// | WebP   | EXIF chunk |
/// | AVIF   | HEIF/ISOBMFF Exif item |
/// | PNG    | eXIf chunk |
/// | GIF / JXL / SVG / APV / HEIC | returns empty metadata |
pub fn read_standard_image_metadata(
    data: &[u8],
    format: StandardFormat,
) -> crate::core::metadata::ImageMetadata {
    use crate::metadata::exif::ExifParser;
    use little_exif::filetype::FileExtension;

    let file_type = match format {
        StandardFormat::Jpeg => FileExtension::JPEG,
        StandardFormat::Tiff => FileExtension::TIFF,
        StandardFormat::WebP => FileExtension::WEBP,
        StandardFormat::Avif => FileExtension::HEIF,
        StandardFormat::Png => FileExtension::PNG {
            as_zTXt_chunk: false,
        },
        // Formats with no EXIF support in little_exif
        _ => return crate::core::metadata::ImageMetadata::default(),
    };

    ExifParser::parse_from_bytes(data, file_type)
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
            StandardFormat::Heic,
            StandardFormat::Svg,
            StandardFormat::Apv,
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

        assert_eq!(decoded.width(), W as u32);
        assert_eq!(decoded.height(), H as u32);
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

        assert_eq!(decoded.width(), W as u32);
        assert_eq!(decoded.height(), H as u32);
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
        assert_eq!(img.width(), W as u32);
        assert_eq!(img.height(), H as u32);
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

        let frame = Frame {
            width: 2,
            height: 2,
            // Pixel indices row-major: top-left=0, top-right=1, bottom-left=2, bottom-right=3
            buffer: Cow::Owned(vec![0u8, 1, 2, 3]),
            ..Frame::default()
        };
        encoder.write_frame(&frame).expect("write gif frame");
        drop(encoder);
        out
    }

    #[test]
    fn gif_decode_dimensions() {
        let gif_data = make_minimal_gif();
        let img =
            decode_standard_image(&gif_data, StandardFormat::Gif).expect("GIF decode must succeed");
        assert_eq!(img.width(), 2, "decoded width must be 2");
        assert_eq!(img.height(), 2, "decoded height must be 2");
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
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
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
    fn tiff_invalid_data_returns_error() {
        // Truncated TIFF magic bytes should return a decode error, not a panic.
        let magic = [0x49u8, 0x49, 0x2A, 0x00, 0, 0, 0, 8];
        let result = decode_standard_image(&magic, StandardFormat::Tiff);
        assert!(result.is_err());
    }

    /// Build a minimal valid TIFF file (2×2 RGB, 8-bit) in memory using the tiff crate encoder.
    fn make_minimal_tiff_rgb8() -> Vec<u8> {
        use std::io::Cursor;
        use tiff::encoder::{TiffEncoder, colortype::RGB8};
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut enc = TiffEncoder::new(&mut cursor).unwrap();
            let pixels: Vec<u8> = vec![
                255, 0, 0, // red
                0, 255, 0, // green
                0, 0, 255, // blue
                255, 255, 255, // white
            ];
            enc.write_image::<RGB8>(2, 2, &pixels).unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn tiff_decode_dimensions() {
        let tiff_data = make_minimal_tiff_rgb8();
        let img = decode_standard_image(&tiff_data, StandardFormat::Tiff)
            .expect("TIFF decode must succeed");
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
        assert_eq!(img.data.len(), 2 * 2 * 3);
    }

    #[test]
    fn tiff_decode_first_pixel_is_red() {
        let tiff_data = make_minimal_tiff_rgb8();
        let img = decode_standard_image(&tiff_data, StandardFormat::Tiff).unwrap();
        assert_eq!(img.data[0], u8_to_u16(255), "R of top-left pixel");
        assert_eq!(img.data[1], u8_to_u16(0), "G of top-left pixel");
        assert_eq!(img.data[2], u8_to_u16(0), "B of top-left pixel");
    }

    #[test]
    fn tiff_detect_then_decode() {
        let tiff_data = make_minimal_tiff_rgb8();
        let fmt = detect_standard_format(&tiff_data);
        assert_eq!(fmt, Some(StandardFormat::Tiff));
        let img = decode_standard_image(&tiff_data, fmt.unwrap()).unwrap();
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
    }

    /// Build a grayscale TIFF (4×4, 8-bit) in memory.
    fn make_tiff_gray8() -> Vec<u8> {
        use std::io::Cursor;
        use tiff::encoder::{TiffEncoder, colortype::Gray8};
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut enc = TiffEncoder::new(&mut cursor).unwrap();
            let pixels: Vec<u8> = (0..16).map(|i| (i * 17) as u8).collect();
            enc.write_image::<Gray8>(4, 4, &pixels).unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn tiff_decode_grayscale_expands_to_rgb() {
        let tiff_data = make_tiff_gray8();
        let img = decode_standard_image(&tiff_data, StandardFormat::Tiff).unwrap();
        assert_eq!(img.width(), 4);
        assert_eq!(img.height(), 4);
        assert_eq!(img.data.len(), 4 * 4 * 3);
        // Grayscale: R == G == B for each pixel
        for px in img.data.chunks_exact(3) {
            assert_eq!(px[0], px[1]);
            assert_eq!(px[1], px[2]);
        }
    }

    /// Build an RGBA TIFF (2×2, 8-bit) in memory.
    fn make_tiff_rgba8() -> Vec<u8> {
        use std::io::Cursor;
        use tiff::encoder::{TiffEncoder, colortype::RGBA8};
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut enc = TiffEncoder::new(&mut cursor).unwrap();
            let pixels: Vec<u8> = vec![
                255, 0, 0, 128, // red, half alpha
                0, 255, 0, 255, // green, full alpha
                0, 0, 255, 0, // blue, zero alpha
                255, 255, 255, 255, // white, full alpha
            ];
            enc.write_image::<RGBA8>(2, 2, &pixels).unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn tiff_decode_rgba_drops_alpha() {
        let tiff_data = make_tiff_rgba8();
        let img = decode_standard_image(&tiff_data, StandardFormat::Tiff).unwrap();
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
        // Should be RGB only (alpha dropped)
        assert_eq!(img.data.len(), 2 * 2 * 3);
        // First pixel should be red
        assert_eq!(img.data[0], u8_to_u16(255));
        assert_eq!(img.data[1], u8_to_u16(0));
        assert_eq!(img.data[2], u8_to_u16(0));
    }

    /// Build a 16-bit RGB TIFF (2×2) in memory.
    fn make_tiff_rgb16() -> Vec<u8> {
        use std::io::Cursor;
        use tiff::encoder::{TiffEncoder, colortype::RGB16};
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut enc = TiffEncoder::new(&mut cursor).unwrap();
            let pixels: Vec<u16> = vec![
                65535, 0, 0, // red
                0, 65535, 0, // green
                0, 0, 65535, // blue
                32768, 32768, 32768, // gray
            ];
            enc.write_image::<RGB16>(2, 2, &pixels).unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn tiff_decode_16bit_preserves_values() {
        let tiff_data = make_tiff_rgb16();
        let img = decode_standard_image(&tiff_data, StandardFormat::Tiff).unwrap();
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
        // 16-bit values should be preserved exactly
        assert_eq!(img.data[0], 65535); // R of red pixel
        assert_eq!(img.data[1], 0); // G of red pixel
        assert_eq!(img.data[2], 0); // B of red pixel
    }

    #[cfg(not(feature = "avif"))]
    #[test]
    fn avif_returns_unsupported_without_feature() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"avif");
        let result = decode_standard_image(&magic, StandardFormat::Avif);
        assert!(result.is_err());
        assert!(matches!(result, Err(RawError::Unsupported(_))));
    }

    // ── StandardFormat name (new variants) ───────────────────────────────

    #[test]
    fn standard_format_name_new_variants() {
        assert_eq!(StandardFormat::Heic.name(), "HEIC");
        assert_eq!(StandardFormat::Svg.name(), "SVG");
        assert_eq!(StandardFormat::Apv.name(), "APV");
    }

    // ── Display ──────────────────────────────────────────────────────────

    #[test]
    fn standard_format_display() {
        let all = [
            (StandardFormat::Gif, "GIF"),
            (StandardFormat::Jpeg, "JPEG"),
            (StandardFormat::Png, "PNG"),
            (StandardFormat::WebP, "WebP"),
            (StandardFormat::Jxl, "JXL"),
            (StandardFormat::Tiff, "TIFF"),
            (StandardFormat::Avif, "AVIF"),
            (StandardFormat::Heic, "HEIC"),
            (StandardFormat::Svg, "SVG"),
            (StandardFormat::Apv, "APV"),
        ];
        for (fmt, expected) in all {
            assert_eq!(format!("{}", fmt), expected);
            assert_eq!(fmt.to_string(), expected);
        }
    }

    // ── extension / from_extension / mime_type ──────────────────────────

    #[test]
    fn extension_roundtrip() {
        let all = [
            StandardFormat::Gif,
            StandardFormat::Jpeg,
            StandardFormat::Png,
            StandardFormat::WebP,
            StandardFormat::Jxl,
            StandardFormat::Tiff,
            StandardFormat::Avif,
            StandardFormat::Heic,
            StandardFormat::Svg,
            StandardFormat::Apv,
        ];
        for fmt in all {
            let ext = fmt.extension();
            assert_eq!(
                StandardFormat::from_extension(ext),
                Some(fmt),
                "roundtrip failed for {:?} (ext={ext})",
                fmt
            );
        }
    }

    #[test]
    fn from_extension_case_insensitive() {
        assert_eq!(
            StandardFormat::from_extension("JPG"),
            Some(StandardFormat::Jpeg)
        );
        assert_eq!(
            StandardFormat::from_extension("Png"),
            Some(StandardFormat::Png)
        );
        assert_eq!(
            StandardFormat::from_extension("WEBP"),
            Some(StandardFormat::WebP)
        );
    }

    #[test]
    fn from_extension_aliases() {
        // JPEG aliases
        for ext in ["jpg", "jpeg", "jpe", "jfif"] {
            assert_eq!(
                StandardFormat::from_extension(ext),
                Some(StandardFormat::Jpeg),
                "alias {ext}"
            );
        }
        // TIFF alias
        assert_eq!(
            StandardFormat::from_extension("tif"),
            Some(StandardFormat::Tiff)
        );
        // HEIC alias
        assert_eq!(
            StandardFormat::from_extension("heif"),
            Some(StandardFormat::Heic)
        );
        // SVG alias
        assert_eq!(
            StandardFormat::from_extension("svgz"),
            Some(StandardFormat::Svg)
        );
    }

    #[test]
    fn from_extension_unknown_returns_none() {
        assert_eq!(StandardFormat::from_extension("bmp"), None);
        assert_eq!(StandardFormat::from_extension(""), None);
        assert_eq!(StandardFormat::from_extension("raw"), None);
    }

    #[test]
    fn mime_types() {
        assert_eq!(StandardFormat::Gif.mime_type(), "image/gif");
        assert_eq!(StandardFormat::Jpeg.mime_type(), "image/jpeg");
        assert_eq!(StandardFormat::Png.mime_type(), "image/png");
        assert_eq!(StandardFormat::WebP.mime_type(), "image/webp");
        assert_eq!(StandardFormat::Jxl.mime_type(), "image/jxl");
        assert_eq!(StandardFormat::Tiff.mime_type(), "image/tiff");
        assert_eq!(StandardFormat::Avif.mime_type(), "image/avif");
        assert_eq!(StandardFormat::Heic.mime_type(), "image/heic");
        assert_eq!(StandardFormat::Svg.mime_type(), "image/svg+xml");
        assert_eq!(StandardFormat::Apv.mime_type(), "video/apv");
    }

    // ── supports_decode / supports_encode ───────────────────────────────

    #[test]
    fn supports_decode_standard_formats() {
        assert!(StandardFormat::Gif.supports_decode());
        assert!(StandardFormat::Jpeg.supports_decode());
        assert!(StandardFormat::Png.supports_decode());
        assert!(StandardFormat::WebP.supports_decode());
        assert!(StandardFormat::Jxl.supports_decode());
        assert!(StandardFormat::Tiff.supports_decode());
        // AVIF decode requires the `avif` feature
        #[cfg(feature = "avif")]
        assert!(StandardFormat::Avif.supports_decode());
        #[cfg(not(feature = "avif"))]
        assert!(!StandardFormat::Avif.supports_decode());
        // Stubbed formats
        assert!(!StandardFormat::Heic.supports_decode());
        assert!(!StandardFormat::Apv.supports_decode());
    }

    #[test]
    fn supports_encode_standard_formats() {
        assert!(StandardFormat::Png.supports_encode());
        assert!(StandardFormat::Jpeg.supports_encode());
        assert!(StandardFormat::WebP.supports_encode());
        // Formats without encoding support
        assert!(!StandardFormat::Gif.supports_encode());
        assert!(!StandardFormat::Tiff.supports_encode());
        assert!(!StandardFormat::Heic.supports_encode());
        assert!(!StandardFormat::Apv.supports_encode());
    }

    // ── HEIC detection and decode ─────────────────────────────────────────

    #[test]
    fn detect_heic_heic_brand() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"heic");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Heic));
    }

    #[test]
    fn detect_heic_heis_brand() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"heis");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Heic));
    }

    #[test]
    fn detect_heic_hevc_brand() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"hevc");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Heic));
    }

    #[test]
    fn detect_heic_hevx_brand() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"hevx");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Heic));
    }

    #[test]
    fn detect_non_heic_ftyp_cr3_returns_none_for_heic() {
        // CR3 uses ftyp with 'crx ' brand — must NOT be detected as HEIC
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"crx ");
        assert_ne!(detect_standard_format(&magic), Some(StandardFormat::Heic));
    }

    #[test]
    fn heic_decode_returns_error() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"heic");
        let result = decode_standard_image(&magic, StandardFormat::Heic);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(RawError::Format(FormatError::ImageDecode {
                format: "HEIC",
                ..
            }))
        ));
    }

    // ── APV detection and decode ──────────────────────────────────────────

    #[test]
    fn detect_apv_apv1_brand() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"apv1");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Apv));
    }

    #[test]
    fn detect_apv_apvx_brand() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"apvx");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Apv));
    }

    #[test]
    fn apv_decode_returns_error() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"apv1");
        let result = decode_standard_image(&magic, StandardFormat::Apv);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(RawError::Format(FormatError::ImageDecode {
                format: "APV",
                ..
            }))
        ));
    }

    // ── SVG detection ─────────────────────────────────────────────────────

    #[test]
    fn detect_svg_xml_prefix() {
        let data = b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
        assert_eq!(detect_standard_format(data), Some(StandardFormat::Svg));
    }

    #[test]
    fn detect_svg_bare_svg_tag() {
        let data = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
        assert_eq!(detect_standard_format(data), Some(StandardFormat::Svg));
    }

    #[test]
    fn detect_svg_xml_without_svg_tag_returns_none() {
        // XML that contains no <svg element
        let data = b"<?xml version=\"1.0\"?><root></root>";
        assert_eq!(detect_standard_format(data), None);
    }

    #[test]
    fn svg_decode_returns_error_without_feature() {
        // Without the 'svg' feature, decode should return an error.
        #[cfg(not(feature = "svg"))]
        {
            let data = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"></svg>";
            let result = decode_standard_image(data, StandardFormat::Svg);
            assert!(result.is_err());
            assert!(matches!(
                result,
                Err(RawError::Format(FormatError::ImageDecode {
                    format: "SVG",
                    ..
                }))
            ));
        }
        // With the svg feature enabled, the test is skipped here (covered by feature-gated tests).
        #[cfg(feature = "svg")]
        {
            // Just verify the variant exists and the name is correct.
            assert_eq!(StandardFormat::Svg.name(), "SVG");
        }
    }

    #[cfg(feature = "svg")]
    #[test]
    fn svg_decode_simple_rect() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <rect width="4" height="4" fill="red"/>
        </svg>"#;
        let result = decode_standard_image(svg, StandardFormat::Svg);
        assert!(result.is_ok());
        let img = result.unwrap();
        assert_eq!(img.width(), 4);
        assert_eq!(img.height(), 4);
        assert_eq!(img.data.len(), 4 * 4 * 3);
    }

    // ── read_standard_image_metadata ─────────────────────────────────────

    #[test]
    fn read_metadata_unsupported_format_returns_default() {
        // GIF has no EXIF support → returns empty metadata (no panic)
        let gif_header = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\x00\x00\x00\x00\x00\x3b";
        let md = read_standard_image_metadata(gif_header, StandardFormat::Gif);
        assert!(md.camera.make.is_empty());
        assert!(md.exif.iso.is_none());
    }

    #[test]
    fn read_metadata_invalid_data_returns_default() {
        // Garbage data → little_exif returns error → we return default
        let junk = b"\x00\x01\x02\x03\x04\x05\x06\x07";
        let md = read_standard_image_metadata(junk, StandardFormat::Jpeg);
        assert!(md.camera.make.is_empty());
    }

    #[cfg(feature = "avif")]
    #[test]
    fn avif_exif_round_trip() {
        use crate::core::metadata::*;
        use crate::formats::encode_rgb_image;
        use crate::formats::export::{AvifOptions, EncodeOptions};

        // Build a 2×2 synthetic image (solid red).
        let data: Vec<u16> = vec![65535, 0, 0, 65535, 0, 0, 65535, 0, 0, 65535, 0, 0];
        let rgb = RgbImage::new(2, 2, data);

        // Build metadata with known EXIF values.
        let md = ImageMetadata {
            camera: CameraInfo {
                make: "TestMake".to_string(),
                model: "TestModel".to_string(),
                ..Default::default()
            },
            exif: ExifInfo {
                iso: Some(400),
                focal_length: Some(URational::new(50, 1)),
                ..Default::default()
            },
            datetime: DateTimeInfo {
                datetime_original: Some("2025:06:15 12:00:00".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let tmp = std::env::temp_dir().join("rawshift_avif_exif_test.avif");
        let opts = EncodeOptions::Avif(AvifOptions {
            quality: 60,
            speed: 10,
            embed_exif: true,
            embed_icc: false,
        });
        encode_rgb_image(&rgb, &md, &tmp, &opts).expect("encode AVIF");

        // Read back the AVIF file and extract metadata.
        let avif_bytes = std::fs::read(&tmp).expect("read back AVIF");
        let read_md = read_standard_image_metadata(&avif_bytes, StandardFormat::Avif);

        // Verify core EXIF tags survived the round-trip.
        assert_eq!(read_md.camera.make, "TestMake", "make round-trip");
        assert_eq!(read_md.camera.model, "TestModel", "model round-trip");
        assert_eq!(read_md.exif.iso, Some(400), "ISO round-trip");
        assert_eq!(
            read_md.exif.focal_length.map(|r| r.numerator),
            Some(50),
            "focal_length round-trip"
        );
        assert_eq!(
            read_md.datetime.datetime_original,
            Some("2025:06:15 12:00:00".to_string()),
            "datetime_original round-trip"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[cfg(feature = "avif")]
    #[test]
    fn avif_supports_decode_with_feature() {
        assert!(StandardFormat::Avif.supports_decode());
    }

    #[cfg(feature = "avif")]
    #[test]
    fn avif_supports_encode_with_feature() {
        assert!(StandardFormat::Avif.supports_encode());
    }
}
