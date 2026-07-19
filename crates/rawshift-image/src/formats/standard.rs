//! Standard image format decoders (JPEG, PNG, WebP, TIFF, JXL, AVIF, HEIC, SVG, APV, PPM).
//!
//! This module provides decoders for common non-RAW image formats that decode
//! directly to RGB pixel data stored in an [`RgbImage`].

#[cfg(any_standard_decode)]
use std::io::Cursor;

#[cfg(feature = "zune-runtime")]
use zune_core::bytestream::ZCursor;
#[cfg(feature = "zune-runtime")]
use zune_core::colorspace::ColorSpace;
#[cfg(feature = "zune-runtime")]
use zune_core::options::DecoderOptions;
#[cfg(feature = "zune-runtime")]
use zune_core::result::DecodingResult;

use crate::core::CodecId;
use crate::core::{Dimensions, RgbImage};
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
    /// PPM / Netpbm (Portable Pixmap family: P5, P6, P7, PFM)
    Ppm,
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
            StandardFormat::Ppm => "PPM",
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
            StandardFormat::Ppm => "ppm",
        }
    }

    /// Look up a format by file extension (case-insensitive).
    ///
    /// Common aliases are supported (e.g. `jpeg`/`jpe`/`jfif` for JPEG,
    /// `tif` for TIFF, `heif` for HEIC, `svgz` for SVG, `pgm`/`pnm`/`pam`/`pfm`
    /// for PPM).
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
            "ppm" | "pgm" | "pnm" | "pam" | "pfm" => Some(StandardFormat::Ppm),
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
            StandardFormat::Ppm => "image/x-portable-pixmap",
        }
    }

    /// Whether this format can be decoded to an [`RgbImage`].
    pub fn supports_decode(self) -> bool {
        match self {
            #[cfg(feature = "gif-decode")]
            StandardFormat::Gif => true,
            #[cfg(feature = "jpeg-decode")]
            StandardFormat::Jpeg => true,
            #[cfg(feature = "png-decode")]
            StandardFormat::Png => true,
            #[cfg(feature = "webp-decode")]
            StandardFormat::WebP => true,
            #[cfg(feature = "jxl-decode")]
            StandardFormat::Jxl => true,
            #[cfg(feature = "tiff-decode")]
            StandardFormat::Tiff => true,
            #[cfg(feature = "avif-decode")]
            StandardFormat::Avif => true,
            #[cfg(feature = "svg-decode")]
            StandardFormat::Svg => true,
            #[cfg(feature = "heic-decode")]
            StandardFormat::Heic => true,
            #[cfg(feature = "ppm-decode")]
            StandardFormat::Ppm => true,
            _ => false,
        }
    }

    /// Whether this format can be encoded from an [`RgbImage`].
    pub fn supports_encode(self) -> bool {
        match self {
            #[cfg(feature = "png-encode")]
            StandardFormat::Png => true,
            #[cfg(feature = "jpeg-encode")]
            StandardFormat::Jpeg => true,
            #[cfg(feature = "webp-encode")]
            StandardFormat::WebP => true,
            #[cfg(feature = "avif-encode")]
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
        #[cfg(feature = "heic-decode")]
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
        // PPM / Netpbm: 'P' + version (5/6/7 binary, or F/f for PFM) + whitespace.
        // P1-P4 (ASCII bitmaps / binary PBM) are intentionally excluded — the
        // zune-ppm backend does not decode them.
        d if d[0] == b'P'
            && matches!(d[1], b'5' | b'6' | b'7' | b'F' | b'f')
            && d[2].is_ascii_whitespace() =>
        {
            Some(StandardFormat::Ppm)
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
#[cfg_attr(not(any_standard_decode), allow(dead_code))]
fn u8_to_u16(v: u8) -> u16 {
    (v as u16) * 257
}

// ── GIF ──────────────────────────────────────────────────────────────────────

#[cfg(feature = "gif-decode")]
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

    RgbImage::new(canvas_width, canvas_height, out)
}

// ── JPEG ─────────────────────────────────────────────────────────────────────

#[cfg(feature = "jpeg-decode")]
fn decode_jpeg(data: &[u8], cfg: &ZuneJpegDecodeConfig) -> RawResult<RgbImage> {
    let mut opts = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGB)
        .set_strict_mode(cfg.strict);
    if let Some(w) = cfg.max_width {
        opts = opts.set_max_width(w);
    }
    if let Some(h) = cfg.max_height {
        opts = opts.set_max_height(h);
    }
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

    RgbImage::new(w, h, data_u16)
}

// ── PNG ──────────────────────────────────────────────────────────────────────

#[cfg(feature = "png-decode")]
fn decode_png(data: &[u8], cfg: &ZunePngDecodeConfig) -> RawResult<RgbImage> {
    // Decode the PNG in its native color space (RGB, RGBA, Luma, LumaA, …)
    // and then convert to packed RGB u16 manually.
    let mut opts = DecoderOptions::default()
        .png_set_strip_to_8bit(false)
        .set_strict_mode(cfg.strict)
        .png_set_confirm_crc(cfg.confirm_crc);
    if let Some(w) = cfg.max_width {
        opts = opts.set_max_width(w);
    }
    if let Some(h) = cfg.max_height {
        opts = opts.set_max_height(h);
    }
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

    RgbImage::new(w, h, data_u16)
}

// ── WebP ─────────────────────────────────────────────────────────────────────

#[cfg(feature = "webp-decode")]
fn decode_webp(data: &[u8]) -> RawResult<RgbImage> {
    let (w, h, rgb) = crate::codecs::webp::decode_webp_rgb(data).map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "WebP",
            message: e,
        })
    })?;

    let data_u16: Vec<u16> = rgb.iter().map(|&v| u8_to_u16(v)).collect();

    RgbImage::new(w, h, data_u16)
}

// ── JXL ──────────────────────────────────────────────────────────────────────

#[cfg(feature = "jxl-decode")]
fn decode_jxl(data: &[u8]) -> RawResult<RgbImage> {
    use jxl_oxide::JxlImage;

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

    jxl_render_to_rgb(w, h, image.pixel_format(), render)
}

/// Convert a `jxl-oxide` [`Render`](jxl_oxide::Render) into a packed RGB
/// [`RgbImage`]. Shared by [`decode_jxl`] and [`decode_jxl_partial`].
#[cfg(feature = "jxl-decode")]
fn jxl_render_to_rgb(
    w: u32,
    h: u32,
    pixel_format: jxl_oxide::PixelFormat,
    render: jxl_oxide::Render,
) -> RawResult<RgbImage> {
    use jxl_oxide::PixelFormat;

    let total_pixels = (w as usize) * (h as usize);

    // `stream_no_alpha` yields exactly the color channels.
    let mut stream = render.stream_no_alpha();
    let channels = stream.channels() as usize;

    let mut samples_u16 = vec![0u16; total_pixels * channels];
    stream.write_to_buffer(&mut samples_u16);

    let data_u16: Vec<u16> = match pixel_format {
        PixelFormat::Rgb => samples_u16,
        PixelFormat::Gray => samples_u16.iter().flat_map(|&v| [v, v, v]).collect(),
        PixelFormat::Rgba | PixelFormat::Graya => {
            if pixel_format == PixelFormat::Graya {
                samples_u16
                    .chunks_exact(channels)
                    .flat_map(|px| [px[0], px[0], px[0]])
                    .collect()
            } else {
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

    RgbImage::new(w, h, data_u16)
}

/// Decode a JPEG XL stream that may be **truncated**, returning the best
/// available render.
///
/// Unlike [`decode_standard_image`], which errors on a stream with no fully
/// loaded keyframe, this feeds bytes progressively and — if the stream ends
/// mid-frame — renders the partially-decoded frame. The returned `bool` is
/// `true` when a complete keyframe was decoded and `false` for a partial render.
///
/// The returned image is tagged [`ColorDescription::SRGB`](crate::core::ColorDescription).
///
/// # Errors
/// Returns an error only when the stream is too short to even parse the image
/// header, or the pixel format is unsupported.
#[cfg(feature = "jxl-decode")]
pub fn decode_jxl_partial(data: &[u8]) -> RawResult<(RgbImage, bool)> {
    use jxl_oxide::{InitializeResult, JxlImage};

    let jxl_err = |e| {
        RawError::Format(FormatError::ImageDecode {
            format: "JXL",
            message: format!("{e}"),
        })
    };

    // Phase 1: feed bytes until the image header initializes.
    let mut uninit = JxlImage::builder().build_uninit();
    let mut offset = 0usize;
    let mut image = loop {
        let consumed = if offset < data.len() {
            uninit.feed_bytes(&data[offset..]).map_err(jxl_err)?
        } else {
            0
        };
        offset += consumed;
        match uninit.try_init().map_err(jxl_err)? {
            InitializeResult::Initialized(img) => break img,
            InitializeResult::NeedMoreData(next) => {
                uninit = next;
                if consumed == 0 {
                    return Err(RawError::Format(FormatError::ImageDecode {
                        format: "JXL",
                        message: "stream too short to read the image header".to_string(),
                    }));
                }
            }
        }
    };

    // Phase 2: feed the remainder into the initialized image. Feeding stops
    // early — without error — when a truncated stream runs out of bytes.
    while offset < data.len() {
        let consumed = image.feed_bytes(&data[offset..]).map_err(jxl_err)?;
        if consumed == 0 {
            break;
        }
        offset += consumed;
    }

    let (w, h) = (image.width(), image.height());
    let pixel_format = image.pixel_format();

    // A fully-loaded keyframe renders completely; otherwise render whatever the
    // truncated stream has produced so far.
    let (render, complete) = if image.num_loaded_keyframes() > 0 {
        (image.render_frame(0).map_err(jxl_err)?, true)
    } else {
        (image.render_loading_frame().map_err(jxl_err)?, false)
    };

    let rgb = jxl_render_to_rgb(w, h, pixel_format, render)?;
    Ok((tag_srgb(rgb), complete))
}

// ── TIFF ─────────────────────────────────────────────────────────────────────

#[cfg(feature = "tiff-decode")]
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

    RgbImage::new(w, h, data_u16)
}

// ── AVIF ─────────────────────────────────────────────────────────────────────

#[cfg(feature = "avif-decode")]
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
    RgbImage::new(w, h, rgb.into_raw())
}

#[cfg(not(feature = "avif-decode"))]
fn decode_avif(_data: &[u8]) -> RawResult<RgbImage> {
    Err(RawError::Unsupported(
        "AVIF decoding requires the `avif-decode` feature flag.".to_string(),
    ))
}

// ── HEIC ─────────────────────────────────────────────────────────────────────

/// Decode a HEIC/HEIF file (ISOBMFF + HEVC) via libheif.
#[cfg(feature = "heic-decode")]
fn decode_heic(data: &[u8]) -> RawResult<RgbImage> {
    let decoded = crate::codecs::heic::decode_primary(data).map_err(|message| {
        RawError::Format(FormatError::ImageDecode {
            format: "HEIC",
            message,
        })
    })?;
    RgbImage::new(decoded.width, decoded.height, decoded.rgb)
}

#[cfg(not(feature = "heic-decode"))]
fn decode_heic(_data: &[u8]) -> RawResult<RgbImage> {
    Err(RawError::Format(FormatError::ImageDecode {
        format: "HEIC",
        message: "HEIC decoding requires the `heic` feature flag.".to_string(),
    }))
}

// ── SVG ──────────────────────────────────────────────────────────────────────

#[cfg(feature = "svg-decode")]
fn decode_svg(data: &[u8], cfg: &ResvgDecodeConfig) -> RawResult<RgbImage> {
    use resvg::{tiny_skia, usvg};

    let options = usvg::Options {
        dpi: cfg.dpi,
        ..usvg::Options::default()
    };
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

    RgbImage::new(width, height, data_u16)
}

#[cfg(not(feature = "svg-decode"))]
fn decode_svg(_data: &[u8], _cfg: &ResvgDecodeConfig) -> RawResult<RgbImage> {
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

// ── PPM ──────────────────────────────────────────────────────────────────────

#[cfg(feature = "ppm-decode")]
fn decode_ppm(data: &[u8], _cfg: &ZunePpmDecodeConfig) -> RawResult<RgbImage> {
    let cursor = ZCursor::new(data);
    let mut decoder = zune_ppm::PPMDecoder::new_with_options(cursor, DecoderOptions::default());

    let result = decoder.decode().map_err(|e| {
        RawError::Format(FormatError::ImageDecode {
            format: "PPM",
            message: format!("{e:?}"),
        })
    })?;

    let (w, h) = decoder
        .dimensions()
        .map(|(w, h)| (w as u32, h as u32))
        .ok_or_else(|| {
            RawError::Format(FormatError::ImageDecode {
                format: "PPM",
                message: "could not read image dimensions after decode".to_string(),
            })
        })?;

    let colorspace = decoder.colorspace().unwrap_or(ColorSpace::RGB);
    let n = colorspace.num_components();

    // Convert raw samples to Vec<u16> (PFM yields f32 normalised to 0..1).
    let samples_u16: Vec<u16> = match result {
        DecodingResult::U8(px) => px.iter().map(|&v| u8_to_u16(v)).collect(),
        DecodingResult::U16(px) => px,
        DecodingResult::F32(px) => px
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 65535.0) as u16)
            .collect(),
        _ => {
            return Err(RawError::Format(FormatError::ImageDecode {
                format: "PPM",
                message: "unexpected pixel depth in decoded result".to_string(),
            }));
        }
    };

    // Convert any colorspace to packed RGB u16.
    let data_u16: Vec<u16> = match colorspace {
        ColorSpace::RGB => samples_u16,
        ColorSpace::RGBA => samples_u16
            .chunks_exact(n)
            .flat_map(|px| [px[0], px[1], px[2]])
            .collect(),
        ColorSpace::Luma => samples_u16.iter().flat_map(|&v| [v, v, v]).collect(),
        ColorSpace::LumaA => samples_u16
            .chunks_exact(n)
            .flat_map(|px| [px[0], px[0], px[0]])
            .collect(),
        _ => {
            return Err(RawError::Format(FormatError::ImageDecode {
                format: "PPM",
                message: format!("unsupported PPM colorspace: {colorspace:?}"),
            }));
        }
    };

    RgbImage::new(w, h, data_u16)
}

// ── Decoder implementation selection ──────────────────────────────────────────

/// Per-implementation configuration for the `zune-jpeg` JPEG decoder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ZuneJpegDecodeConfig {
    /// Reject images wider than this, in pixels. `None` keeps the decoder's
    /// built-in limit.
    pub max_width: Option<usize>,
    /// Reject images taller than this, in pixels. `None` keeps the decoder's
    /// built-in limit.
    pub max_height: Option<usize>,
    /// Reject streams that deviate from the JPEG specification instead of
    /// attempting recovery. Default: `false`.
    pub strict: bool,
}

/// Per-implementation configuration for the `zune-png` PNG decoder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ZunePngDecodeConfig {
    /// Reject images wider than this, in pixels. `None` keeps the decoder's
    /// built-in limit.
    pub max_width: Option<usize>,
    /// Reject images taller than this, in pixels. `None` keeps the decoder's
    /// built-in limit.
    pub max_height: Option<usize>,
    /// Verify per-chunk CRCs while decoding. Default: `false`.
    pub confirm_crc: bool,
    /// Reject streams that deviate from the PNG specification. Default: `false`.
    pub strict: bool,
}

/// Per-implementation configuration for the `resvg` SVG renderer.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ResvgDecodeConfig {
    /// Dots-per-inch used to resolve physical units (`mm`, `cm`, `in`) in the
    /// SVG. Default: `96.0`.
    pub dpi: f32,
}

impl Default for ResvgDecodeConfig {
    fn default() -> Self {
        Self { dpi: 96.0 }
    }
}

/// Macro to define an implementation config type that currently exposes no
/// tunable parameters. The type is a stable home for future backend-specific
/// options — adding fields later is a non-breaking change.
macro_rules! empty_decode_config {
    ($(#[$m:meta])* $name:ident, $lib:literal) => {
        #[doc = concat!("Per-implementation configuration for the `", $lib, "` decoder.")]
        ///
        /// This backend currently exposes no tunable parameters that affect the
        /// decoded output; the type exists so backend-specific options can be
        /// added without breaking the API.
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, Default)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        pub struct $name {}
    };
}

empty_decode_config!(LibwebpDecodeConfig, "libwebp");
empty_decode_config!(JxlOxideDecodeConfig, "jxl-oxide");
empty_decode_config!(GifDecodeConfig, "gif");
empty_decode_config!(TiffDecodeConfig, "tiff");
empty_decode_config!(ImageAvifDecodeConfig, "image (avif-native)");
empty_decode_config!(LibheifDecodeConfig, "libheif");
empty_decode_config!(ZunePpmDecodeConfig, "zune-ppm");

/// Selects which decoder implementation handles a standard image, and carries
/// that implementation's configuration.
///
/// Each variant pairs a compressed format with one backend library. rawshift
/// can be built with multiple implementations of the same format enabled (see
/// the implementation feature flags in the crate documentation); this enum is
/// how a caller pins exactly which one [`decode_standard_image_with`] uses.
///
/// Use [`DecodeOptions::default_for`] to obtain the default backend for a
/// format. RAW formats are intentionally absent — they have a single in-repo
/// implementation and are decoded through [`RawFile`](crate::formats::RawFile).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DecodeOptions {
    /// JPEG via `zune-jpeg`.
    #[cfg(feature = "jpeg-decode")]
    JpegZune(ZuneJpegDecodeConfig),
    /// PNG via `zune-png`.
    #[cfg(feature = "png-decode")]
    PngZune(ZunePngDecodeConfig),
    /// WebP via `libwebp`.
    #[cfg(feature = "webp-decode")]
    WebpLibwebp(LibwebpDecodeConfig),
    /// JPEG XL via `jxl-oxide`.
    #[cfg(feature = "jxl-decode")]
    JxlOxide(JxlOxideDecodeConfig),
    /// GIF via the `gif` crate.
    #[cfg(feature = "gif-decode")]
    Gif(GifDecodeConfig),
    /// TIFF via the `tiff` crate.
    #[cfg(feature = "tiff-decode")]
    Tiff(TiffDecodeConfig),
    /// AVIF via `image` (`avif-native`).
    #[cfg(feature = "avif-decode")]
    AvifImage(ImageAvifDecodeConfig),
    /// HEIC/HEIF via `libheif`.
    #[cfg(feature = "heic-decode")]
    HeicLibheif(LibheifDecodeConfig),
    /// SVG via `resvg`.
    #[cfg(feature = "svg-decode")]
    SvgResvg(ResvgDecodeConfig),
    /// PPM / Netpbm via `zune-ppm`.
    #[cfg(feature = "ppm-decode")]
    PpmZune(ZunePpmDecodeConfig),
}

impl DecodeOptions {
    /// The standard format this backend decodes.
    pub fn format(&self) -> StandardFormat {
        match self {
            #[cfg(feature = "jpeg-decode")]
            DecodeOptions::JpegZune(_) => StandardFormat::Jpeg,
            #[cfg(feature = "png-decode")]
            DecodeOptions::PngZune(_) => StandardFormat::Png,
            #[cfg(feature = "webp-decode")]
            DecodeOptions::WebpLibwebp(_) => StandardFormat::WebP,
            #[cfg(feature = "jxl-decode")]
            DecodeOptions::JxlOxide(_) => StandardFormat::Jxl,
            #[cfg(feature = "gif-decode")]
            DecodeOptions::Gif(_) => StandardFormat::Gif,
            #[cfg(feature = "tiff-decode")]
            DecodeOptions::Tiff(_) => StandardFormat::Tiff,
            #[cfg(feature = "avif-decode")]
            DecodeOptions::AvifImage(_) => StandardFormat::Avif,
            #[cfg(feature = "heic-decode")]
            DecodeOptions::HeicLibheif(_) => StandardFormat::Heic,
            #[cfg(feature = "svg-decode")]
            DecodeOptions::SvgResvg(_) => StandardFormat::Svg,
            #[cfg(feature = "ppm-decode")]
            DecodeOptions::PpmZune(_) => StandardFormat::Ppm,
            // Unreachable: with no decode feature enabled `DecodeOptions` has
            // no variants and no value of it can be constructed.
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    /// The stable identifier of the selected decoder implementation.
    pub fn codec_id(&self) -> CodecId {
        match self {
            #[cfg(feature = "jpeg-decode")]
            DecodeOptions::JpegZune(_) => CodecId::new("jpeg/zune"),
            #[cfg(feature = "png-decode")]
            DecodeOptions::PngZune(_) => CodecId::new("png/zune"),
            #[cfg(feature = "webp-decode")]
            DecodeOptions::WebpLibwebp(_) => CodecId::new("webp/libwebp"),
            #[cfg(feature = "jxl-decode")]
            DecodeOptions::JxlOxide(_) => CodecId::new("jxl/jxl-oxide"),
            #[cfg(feature = "gif-decode")]
            DecodeOptions::Gif(_) => CodecId::new("gif/gif"),
            #[cfg(feature = "tiff-decode")]
            DecodeOptions::Tiff(_) => CodecId::new("tiff/tiff"),
            #[cfg(feature = "avif-decode")]
            DecodeOptions::AvifImage(_) => CodecId::new("avif/image"),
            #[cfg(feature = "heic-decode")]
            DecodeOptions::HeicLibheif(_) => CodecId::new("heic/libheif"),
            #[cfg(feature = "svg-decode")]
            DecodeOptions::SvgResvg(_) => CodecId::new("svg/resvg"),
            #[cfg(feature = "ppm-decode")]
            DecodeOptions::PpmZune(_) => CodecId::new("ppm/zune"),
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    /// The default decoder backend for `format`, with default configuration.
    ///
    /// Returns `None` when no decoder for `format` is compiled in (the relevant
    /// feature flag is disabled, or the format has no decoder — e.g. APV).
    pub fn default_for(format: StandardFormat) -> Option<DecodeOptions> {
        match format {
            #[cfg(feature = "jpeg-decode")]
            StandardFormat::Jpeg => Some(DecodeOptions::JpegZune(ZuneJpegDecodeConfig::default())),
            #[cfg(feature = "png-decode")]
            StandardFormat::Png => Some(DecodeOptions::PngZune(ZunePngDecodeConfig::default())),
            #[cfg(feature = "webp-decode")]
            StandardFormat::WebP => {
                Some(DecodeOptions::WebpLibwebp(LibwebpDecodeConfig::default()))
            }
            #[cfg(feature = "jxl-decode")]
            StandardFormat::Jxl => Some(DecodeOptions::JxlOxide(JxlOxideDecodeConfig::default())),
            #[cfg(feature = "gif-decode")]
            StandardFormat::Gif => Some(DecodeOptions::Gif(GifDecodeConfig::default())),
            #[cfg(feature = "tiff-decode")]
            StandardFormat::Tiff => Some(DecodeOptions::Tiff(TiffDecodeConfig::default())),
            #[cfg(feature = "avif-decode")]
            StandardFormat::Avif => {
                Some(DecodeOptions::AvifImage(ImageAvifDecodeConfig::default()))
            }
            #[cfg(feature = "heic-decode")]
            StandardFormat::Heic => {
                Some(DecodeOptions::HeicLibheif(LibheifDecodeConfig::default()))
            }
            #[cfg(feature = "svg-decode")]
            StandardFormat::Svg => Some(DecodeOptions::SvgResvg(ResvgDecodeConfig::default())),
            #[cfg(feature = "ppm-decode")]
            StandardFormat::Ppm => Some(DecodeOptions::PpmZune(ZunePpmDecodeConfig::default())),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Decode a standard (non-RAW) image to an [`RgbImage`] using each format's
/// default decoder implementation.
///
/// The caller must supply the [`StandardFormat`] explicitly. Use
/// [`detect_standard_format`] to infer it from magic bytes when the format is
/// not otherwise known. To pin a specific decoder implementation or pass
/// implementation-specific configuration, use [`decode_standard_image_with`].
///
/// The returned [`RgbImage`] contains 16-bit interleaved RGB data in row-major
/// order. 8-bit source images are scaled to 16-bit by multiplying by 257.
///
/// # Errors
/// Returns [`RawError::ImageDecodeError`] on decode failure, or
/// [`RawError::UnsupportedFormat`] for formats without a decoder.
pub fn decode_standard_image(data: &[u8], format: StandardFormat) -> RawResult<RgbImage> {
    let decoded: RawResult<RgbImage> = match format {
        #[cfg(feature = "gif-decode")]
        StandardFormat::Gif => decode_gif(data),
        #[cfg(feature = "jpeg-decode")]
        StandardFormat::Jpeg => decode_jpeg(data, &ZuneJpegDecodeConfig::default()),
        #[cfg(feature = "png-decode")]
        StandardFormat::Png => decode_png(data, &ZunePngDecodeConfig::default()),
        #[cfg(feature = "webp-decode")]
        StandardFormat::WebP => decode_webp(data),
        #[cfg(feature = "jxl-decode")]
        StandardFormat::Jxl => decode_jxl(data),
        #[cfg(feature = "tiff-decode")]
        StandardFormat::Tiff => decode_tiff(data),
        StandardFormat::Avif => decode_avif(data),
        StandardFormat::Heic => decode_heic(data),
        StandardFormat::Svg => decode_svg(data, &ResvgDecodeConfig::default()),
        #[cfg(feature = "ppm-decode")]
        StandardFormat::Ppm => decode_ppm(data, &ZunePpmDecodeConfig::default()),
        StandardFormat::Apv => decode_apv(data),
        #[allow(unreachable_patterns)]
        _ => Err(RawError::Unsupported(format!(
            "Decoding {:?} requires a feature flag that is not enabled.",
            format.name()
        ))),
    };
    decoded.map(tag_srgb)
}

/// Decode a standard (non-RAW) image with an explicitly selected decoder
/// implementation.
///
/// Unlike [`decode_standard_image`], which always uses each format's default
/// implementation, this lets the caller pin a specific backend library and
/// pass that library's configuration via [`DecodeOptions`].
///
/// # Errors
/// Returns a [`RawError`] if the selected backend fails to decode `data`.
#[cfg_attr(not(any_standard_decode), allow(unused_variables, unreachable_code))]
pub fn decode_standard_image_with(data: &[u8], options: &DecodeOptions) -> RawResult<RgbImage> {
    let decoded: RawResult<RgbImage> = match options {
        #[cfg(feature = "jpeg-decode")]
        DecodeOptions::JpegZune(cfg) => decode_jpeg(data, cfg),
        #[cfg(feature = "png-decode")]
        DecodeOptions::PngZune(cfg) => decode_png(data, cfg),
        #[cfg(feature = "webp-decode")]
        DecodeOptions::WebpLibwebp(_cfg) => decode_webp(data),
        #[cfg(feature = "jxl-decode")]
        DecodeOptions::JxlOxide(_cfg) => decode_jxl(data),
        #[cfg(feature = "gif-decode")]
        DecodeOptions::Gif(_cfg) => decode_gif(data),
        #[cfg(feature = "tiff-decode")]
        DecodeOptions::Tiff(_cfg) => decode_tiff(data),
        #[cfg(feature = "avif-decode")]
        DecodeOptions::AvifImage(_cfg) => decode_avif(data),
        #[cfg(feature = "heic-decode")]
        DecodeOptions::HeicLibheif(_cfg) => decode_heic(data),
        #[cfg(feature = "svg-decode")]
        DecodeOptions::SvgResvg(cfg) => decode_svg(data, cfg),
        #[cfg(feature = "ppm-decode")]
        DecodeOptions::PpmZune(cfg) => decode_ppm(data, cfg),
        // Unreachable: with no decode feature enabled `DecodeOptions` has no
        // variants and no value of it can be constructed.
        #[allow(unreachable_patterns)]
        _ => unreachable!(),
    };
    decoded.map(tag_srgb)
}

/// Tag a freshly-decoded standard image with its color description.
///
/// Every standard decoder produces display-referred, sRGB-encoded RGB, so the
/// result is tagged [`ColorDescription::SRGB`](crate::core::ColorDescription::SRGB).
/// When the source carried a non-sRGB ICC profile the pixels are *not*
/// converted — the precise profile is preserved in
/// [`ImageMetadata::icc_profile`](crate::core::ImageMetadata) by
/// [`read_standard_image_metadata`], and a caller wanting true sRGB pixels can
/// apply [`convert_to_srgb`](crate::transforms::convert_to_srgb).
fn tag_srgb(mut image: RgbImage) -> RgbImage {
    image.set_color(crate::core::ColorDescription::SRGB);
    image
}

// ── Header-only probe ─────────────────────────────────────────────────────────

/// A cheap, header-only summary of a standard image.
///
/// Produced by [`probe_standard_image`] without decoding pixel data — useful
/// when ingest only needs dimensions and format up front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct ImageProbe {
    /// The detected image format.
    pub format: StandardFormat,
    /// Pixel dimensions read from the format header.
    #[cfg_attr(
        feature = "serde",
        serde(with = "rawshift_core::image::dimensions_serde")
    )]
    pub size: Dimensions,
    /// Bits per channel, when the header exposes it cheaply (`None` otherwise).
    pub bit_depth: Option<u8>,
    /// Best-effort color description — see [`decode_standard_image`] for the caveats.
    pub color_space: crate::core::ColorDescription,
}

fn probe_err(format: &'static str, msg: impl Into<String>) -> RawError {
    RawError::Format(FormatError::ImageDecode {
        format,
        message: format!("probe: {}", msg.into()),
    })
}

/// Read an image's format and dimensions from its header, without decoding.
///
/// Parses only the container/codec header, so it is far cheaper than a full
/// [`decode_standard_image`]. Works for every raster format regardless of which
/// decoder features are compiled in — except JXL, whose header parser needs the
/// `jxl-decode` feature.
///
/// # Errors
/// Returns an error when the format is unrecognised, the header is truncated,
/// or (for SVG/APV) the format has no intrinsic raster dimensions.
pub fn probe_standard_image(data: &[u8]) -> RawResult<ImageProbe> {
    let format = detect_standard_format(data)
        .ok_or_else(|| RawError::Unsupported("unrecognized image format".to_string()))?;

    let (size, bit_depth) = match format {
        StandardFormat::Png => probe_png(data)?,
        StandardFormat::Jpeg => probe_jpeg(data)?,
        StandardFormat::Gif => probe_gif(data)?,
        StandardFormat::WebP => probe_webp(data)?,
        StandardFormat::Tiff => probe_tiff(data)?,
        StandardFormat::Avif => probe_isobmff(data, "AVIF")?,
        StandardFormat::Heic => probe_isobmff(data, "HEIC")?,
        StandardFormat::Ppm => probe_ppm(data)?,
        StandardFormat::Jxl => {
            #[cfg(feature = "jxl-decode")]
            {
                probe_jxl(data)?
            }
            #[cfg(not(feature = "jxl-decode"))]
            {
                return Err(RawError::Unsupported(
                    "probing JXL requires the `jxl-decode` feature".to_string(),
                ));
            }
        }
        StandardFormat::Svg | StandardFormat::Apv => {
            return Err(probe_err(
                format.name(),
                "format has no intrinsic raster dimensions",
            ));
        }
    };

    Ok(ImageProbe {
        format,
        size,
        bit_depth,
        color_space: crate::core::ColorDescription::SRGB,
    })
}

/// PNG: dimensions and bit depth live in the fixed-offset IHDR chunk.
fn probe_png(data: &[u8]) -> RawResult<(Dimensions, Option<u8>)> {
    if data.len() < 26 || &data[12..16] != b"IHDR" {
        return Err(probe_err("PNG", "missing IHDR chunk"));
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    Ok((Dimensions { width, height }, Some(data[24])))
}

/// JPEG: scan marker segments for a Start-Of-Frame (SOFn) marker.
fn probe_jpeg(data: &[u8]) -> RawResult<(Dimensions, Option<u8>)> {
    let mut i = 2; // skip the SOI marker
    while i + 1 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        // 0xFF padding and 0xFF00 stuffed bytes carry no segment.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0x00 || marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            i += 2;
            continue;
        }
        // SOF0..SOF15, excluding DHT(C4), JPG(C8) and DAC(CC).
        if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
            if i + 9 > data.len() {
                return Err(probe_err("JPEG", "truncated SOF segment"));
            }
            let precision = data[i + 4];
            let height = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            let width = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
            return Ok((Dimensions { width, height }, Some(precision)));
        }
        // Any other marker carries a big-endian u16 length (incl. the 2 length
        // bytes) — skip past it.
        if i + 4 > data.len() {
            break;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        i += 2 + len.max(2);
    }
    Err(probe_err("JPEG", "no SOF marker found"))
}

/// GIF: the Logical Screen Descriptor follows the 6-byte signature.
fn probe_gif(data: &[u8]) -> RawResult<(Dimensions, Option<u8>)> {
    if data.len() < 10 {
        return Err(probe_err("GIF", "truncated header"));
    }
    let width = u16::from_le_bytes([data[6], data[7]]) as u32;
    let height = u16::from_le_bytes([data[8], data[9]]) as u32;
    Ok((Dimensions { width, height }, Some(8)))
}

/// WebP: dimensions live in the first RIFF chunk (`VP8X`, `VP8 ` or `VP8L`).
fn probe_webp(data: &[u8]) -> RawResult<(Dimensions, Option<u8>)> {
    if data.len() < 30 || &data[8..12] != b"WEBP" {
        return Err(probe_err("WebP", "not a RIFF/WEBP container"));
    }
    // The first chunk's payload starts at offset 20 (12 RIFF + 8 chunk header).
    match &data[12..16] {
        b"VP8X" => {
            let width = 1 + u32::from_le_bytes([data[24], data[25], data[26], 0]);
            let height = 1 + u32::from_le_bytes([data[27], data[28], data[29], 0]);
            Ok((Dimensions { width, height }, Some(8)))
        }
        b"VP8 " => {
            // Lossy: 3-byte frame tag, 3-byte start code, then 14-bit w/h.
            let width = u16::from_le_bytes([data[26], data[27]]) as u32 & 0x3FFF;
            let height = u16::from_le_bytes([data[28], data[29]]) as u32 & 0x3FFF;
            Ok((Dimensions { width, height }, Some(8)))
        }
        b"VP8L" => {
            // Lossless: 1 signature byte, then 14-bit (w-1) and 14-bit (h-1).
            let bits = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);
            let width = (bits & 0x3FFF) + 1;
            let height = ((bits >> 14) & 0x3FFF) + 1;
            Ok((Dimensions { width, height }, Some(8)))
        }
        _ => Err(probe_err("WebP", "unrecognized WebP chunk")),
    }
}

/// TIFF: read `ImageWidth`/`ImageLength` from the first IFD.
fn probe_tiff(data: &[u8]) -> RawResult<(Dimensions, Option<u8>)> {
    if data.len() < 8 {
        return Err(probe_err("TIFF", "truncated header"));
    }
    let le = match &data[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(probe_err("TIFF", "bad byte-order mark")),
    };
    let rd16 = |b: &[u8]| {
        if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        }
    };
    let rd32 = |b: &[u8]| {
        if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        }
    };

    let ifd_off = rd32(&data[4..8]) as usize;
    if ifd_off + 2 > data.len() {
        return Err(probe_err("TIFF", "IFD offset out of bounds"));
    }
    let count = rd16(&data[ifd_off..]) as usize;
    let (mut width, mut height) = (None, None);
    for entry in 0..count {
        let base = ifd_off + 2 + entry * 12;
        if base + 12 > data.len() {
            break;
        }
        let tag = rd16(&data[base..]);
        let ty = rd16(&data[base + 2..]);
        let value = &data[base + 8..base + 12];
        let scalar = match ty {
            3 => rd16(value) as u32, // SHORT
            4 => rd32(value),        // LONG
            _ => continue,
        };
        match tag {
            0x0100 => width = Some(scalar),
            0x0101 => height = Some(scalar),
            _ => {}
        }
    }
    match (width, height) {
        (Some(w), Some(h)) => Ok((
            Dimensions {
                width: w,
                height: h,
            },
            None,
        )),
        _ => Err(probe_err("TIFF", "ImageWidth/ImageLength not found")),
    }
}

/// AVIF / HEIC: locate the ISOBMFF `ispe` (image spatial extents) box. Several
/// may exist (thumbnails, alpha planes) — the largest is taken as the primary.
fn probe_isobmff(data: &[u8], format: &'static str) -> RawResult<(Dimensions, Option<u8>)> {
    let mut best: Option<(u32, u32)> = None;
    let mut i = 0;
    while i + 16 <= data.len() {
        if &data[i..i + 4] == b"ispe" {
            // ispe payload: 4-byte version+flags, then BE width and height.
            let w = u32::from_be_bytes([data[i + 8], data[i + 9], data[i + 10], data[i + 11]]);
            let h = u32::from_be_bytes([data[i + 12], data[i + 13], data[i + 14], data[i + 15]]);
            let larger =
                best.is_none_or(|(bw, bh)| (bw as u64 * bh as u64) < (w as u64 * h as u64));
            if larger {
                best = Some((w, h));
            }
        }
        i += 1;
    }
    match best {
        Some((w, h)) => Ok((
            Dimensions {
                width: w,
                height: h,
            },
            None,
        )),
        None => Err(probe_err(format, "no `ispe` box found")),
    }
}

/// PPM / PGM / PBM (Netpbm): a whitespace-separated ASCII header.
fn probe_ppm(data: &[u8]) -> RawResult<(Dimensions, Option<u8>)> {
    // After the 2-byte magic ("P1".."P6"), read ASCII tokens separated by
    // whitespace: width, height, and (except for bitmaps) maxval. A '#' starts
    // a comment that runs to end-of-line.
    let mut tokens: Vec<&[u8]> = Vec::new();
    let mut i = 2;
    while i < data.len() && tokens.len() < 3 {
        match data[i] {
            b'#' => {
                while i < data.len() && data[i] != b'\n' {
                    i += 1;
                }
            }
            b if b.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < data.len() && !data[i].is_ascii_whitespace() && data[i] != b'#' {
                    i += 1;
                }
                tokens.push(&data[start..i]);
            }
        }
    }
    let parse =
        |t: &[u8]| -> Option<u32> { std::str::from_utf8(t).ok().and_then(|s| s.parse().ok()) };
    let width = tokens.first().and_then(|&t| parse(t));
    let height = tokens.get(1).and_then(|&t| parse(t));
    match (width, height) {
        (Some(w), Some(h)) => {
            // A maxval above 255 means 16-bit samples.
            let bits = tokens
                .get(2)
                .and_then(|&t| parse(t))
                .map(|maxval| if maxval > 255 { 16u8 } else { 8 });
            Ok((
                Dimensions {
                    width: w,
                    height: h,
                },
                bits,
            ))
        }
        _ => Err(probe_err("PPM", "could not read width/height")),
    }
}

/// JXL: parse just enough of the codestream to read the image header.
#[cfg(feature = "jxl-decode")]
fn probe_jxl(data: &[u8]) -> RawResult<(Dimensions, Option<u8>)> {
    use jxl_oxide::{InitializeResult, JxlImage};

    let mut uninit = JxlImage::builder().build_uninit();
    uninit
        .feed_bytes(data)
        .map_err(|e| probe_err("JXL", e.to_string()))?;
    match uninit
        .try_init()
        .map_err(|e| probe_err("JXL", e.to_string()))?
    {
        InitializeResult::Initialized(img) => Ok((
            Dimensions {
                width: img.width(),
                height: img.height(),
            },
            None,
        )),
        InitializeResult::NeedMoreData(_) => Err(probe_err(
            "JXL",
            "stream too short to read the image header",
        )),
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;

    #[test]
    fn probe_png_reads_ihdr() {
        // 8-byte signature + IHDR chunk header + 13-byte IHDR data.
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]); // bit depth 8, color type 2 (RGB)
        let probe = probe_standard_image(&png).expect("probe PNG");
        assert_eq!(probe.format, StandardFormat::Png);
        assert_eq!(
            probe.size,
            Dimensions {
                width: 640,
                height: 480
            }
        );
        assert_eq!(probe.bit_depth, Some(8));
    }

    #[test]
    fn probe_gif_reads_screen_descriptor() {
        let mut gif = Vec::from(*b"GIF89a");
        gif.extend_from_slice(&320u16.to_le_bytes());
        gif.extend_from_slice(&200u16.to_le_bytes());
        gif.extend_from_slice(&[0, 0, 0]);
        let probe = probe_standard_image(&gif).expect("probe GIF");
        assert_eq!(
            probe.size,
            Dimensions {
                width: 320,
                height: 200
            }
        );
    }

    #[test]
    fn probe_rejects_garbage() {
        assert!(probe_standard_image(b"not an image at all").is_err());
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
/// | HEIC   | HEIF Exif + ICC + XMP items (requires the `heic` feature) |
/// | GIF / JXL / SVG / APV | returns empty metadata |
/// When the `exif` feature is disabled the crate has no EXIF parser, so this
/// degrades to returning empty metadata for every format.
#[cfg(feature = "exif")]
pub fn read_standard_image_metadata(
    data: &[u8],
    format: StandardFormat,
) -> crate::core::metadata::ImageMetadata {
    use crate::metadata::exif::ExifParser;
    use little_exif::filetype::FileExtension;

    // HEIC goes through libheif so that ICC and XMP are extracted alongside EXIF.
    #[cfg(feature = "heic-decode")]
    if format == StandardFormat::Heic {
        return crate::formats::heic::read_heic_metadata(data);
    }

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

/// Extract EXIF metadata from a standard image without decoding pixel data.
///
/// This is the `exif`-feature-disabled build: the crate carries no EXIF parser,
/// so empty metadata is always returned. See the `exif`-enabled variant for the
/// documented behaviour.
#[cfg(not(feature = "exif"))]
pub fn read_standard_image_metadata(
    _data: &[u8],
    _format: StandardFormat,
) -> crate::core::metadata::ImageMetadata {
    crate::core::metadata::ImageMetadata::default()
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
            StandardFormat::Ppm,
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
        assert_eq!(decoded.data().len(), W as usize * H as usize * 3);
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
        assert_eq!(decoded.data().len(), W * H * 3);
        // Each u8 value should have been scaled to u16
        assert_eq!(decoded.data()[0], u8_to_u16(pixels_u8[0]));
    }

    // ── DecodeOptions / decode_standard_image_with ────────────────────────

    #[test]
    fn decode_options_default_for_apv_is_none() {
        // APV has no decoder, so there is no default backend.
        assert!(DecodeOptions::default_for(StandardFormat::Apv).is_none());
    }

    #[cfg(feature = "png-decode")]
    #[test]
    fn decode_options_default_for_roundtrips_format() {
        let opts = DecodeOptions::default_for(StandardFormat::Png).expect("png decoder");
        assert_eq!(opts.format(), StandardFormat::Png);
        assert!(matches!(opts, DecodeOptions::PngZune(_)));
    }

    #[cfg(feature = "png-decode")]
    #[test]
    fn decode_standard_image_with_selects_png_backend() {
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

        // Decode through the explicit-backend API with a non-default config.
        let cfg = ZunePngDecodeConfig {
            confirm_crc: true,
            ..ZunePngDecodeConfig::default()
        };
        let via_with = decode_standard_image_with(&encoded, &DecodeOptions::PngZune(cfg))
            .expect("decode_standard_image_with failed");
        // The default-path API must produce an identical result.
        let via_default =
            decode_standard_image(&encoded, StandardFormat::Png).expect("PNG decode failed");

        assert_eq!(via_with.width(), W as u32);
        assert_eq!(via_with.height(), H as u32);
        assert_eq!(via_with.data(), via_default.data());
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

    // ── PPM detect + decode ───────────────────────────────────────────────

    #[test]
    fn detect_ppm_p6() {
        let magic = *b"P6\n2 2\n255\n";
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Ppm));
    }

    #[test]
    fn detect_ppm_rejects_ascii_pbm() {
        // P1-P4 are not handled by the zune-ppm backend, so must not be detected.
        let magic = *b"P1\n2 2\n0 0\n";
        assert_eq!(detect_standard_format(&magic), None);
    }

    #[cfg(feature = "ppm-decode")]
    #[test]
    fn ppm_p6_decode_dimensions_and_samples() {
        // Hand-crafted 2×2 binary P6 (RGB, maxval 255) — no encoder needed.
        // Sample values deliberately avoid ASCII-whitespace bytes (9-13, 32) so
        // the first pixel byte is not mistaken for header padding.
        let pixels: [u8; 12] = [
            100, 120, 130, // (0,0)
            140, 150, 160, // (1,0)
            170, 180, 190, // (0,1)
            200, 210, 220, // (1,1)
        ];
        let mut file = b"P6\n2 2\n255\n".to_vec();
        file.extend_from_slice(&pixels);

        let fmt = detect_standard_format(&file);
        assert_eq!(fmt, Some(StandardFormat::Ppm));

        let decoded = decode_standard_image(&file, StandardFormat::Ppm).expect("PPM decode failed");
        assert_eq!(decoded.width(), 2);
        assert_eq!(decoded.height(), 2);
        assert_eq!(decoded.data().len(), 2 * 2 * 3);
        // 8-bit samples must have been scaled to 16-bit.
        assert_eq!(decoded.data()[0], u8_to_u16(pixels[0]));
        assert_eq!(decoded.data()[11], u8_to_u16(pixels[11]));
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
            img.data().len(),
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
        assert_eq!(img.data()[0], u8_to_u16(255), "R of top-left pixel");
        assert_eq!(img.data()[1], u8_to_u16(0), "G of top-left pixel");
        assert_eq!(img.data()[2], u8_to_u16(0), "B of top-left pixel");
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
        assert_eq!(img.data().len(), 2 * 2 * 3);
    }

    #[test]
    fn tiff_decode_first_pixel_is_red() {
        let tiff_data = make_minimal_tiff_rgb8();
        let img = decode_standard_image(&tiff_data, StandardFormat::Tiff).unwrap();
        assert_eq!(img.data()[0], u8_to_u16(255), "R of top-left pixel");
        assert_eq!(img.data()[1], u8_to_u16(0), "G of top-left pixel");
        assert_eq!(img.data()[2], u8_to_u16(0), "B of top-left pixel");
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
        assert_eq!(img.data().len(), 4 * 4 * 3);
        // Grayscale: R == G == B for each pixel
        for px in img.data().chunks_exact(3) {
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
        assert_eq!(img.data().len(), 2 * 2 * 3);
        // First pixel should be red
        assert_eq!(img.data()[0], u8_to_u16(255));
        assert_eq!(img.data()[1], u8_to_u16(0));
        assert_eq!(img.data()[2], u8_to_u16(0));
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
        assert_eq!(img.data()[0], 65535); // R of red pixel
        assert_eq!(img.data()[1], 0); // G of red pixel
        assert_eq!(img.data()[2], 0); // B of red pixel
    }

    #[cfg(not(feature = "avif-decode"))]
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
        // AVIF decode requires the `avif-decode` feature
        #[cfg(feature = "avif-decode")]
        assert!(StandardFormat::Avif.supports_decode());
        #[cfg(not(feature = "avif-decode"))]
        assert!(!StandardFormat::Avif.supports_decode());
        // HEIC decode requires the `heic` feature
        #[cfg(feature = "heic-decode")]
        assert!(StandardFormat::Heic.supports_decode());
        #[cfg(not(feature = "heic-decode"))]
        assert!(!StandardFormat::Heic.supports_decode());
        // Stubbed formats
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

    #[cfg(feature = "heic-decode")]
    #[test]
    fn detect_heic_heic_brand() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"heic");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Heic));
    }

    #[cfg(feature = "heic-decode")]
    #[test]
    fn detect_heic_heis_brand() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"heis");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Heic));
    }

    #[cfg(feature = "heic-decode")]
    #[test]
    fn detect_heic_hevc_brand() {
        let mut magic = [0u8; 12];
        magic[4..8].copy_from_slice(b"ftyp");
        magic[8..12].copy_from_slice(b"hevc");
        assert_eq!(detect_standard_format(&magic), Some(StandardFormat::Heic));
    }

    #[cfg(feature = "heic-decode")]
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

    #[cfg(feature = "heic-decode")]
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
        #[cfg(not(feature = "svg-decode"))]
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
        #[cfg(feature = "svg-decode")]
        {
            // Just verify the variant exists and the name is correct.
            assert_eq!(StandardFormat::Svg.name(), "SVG");
        }
    }

    #[cfg(feature = "svg-decode")]
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
        assert_eq!(img.data().len(), 4 * 4 * 3);
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
        use crate::formats::export::{
            CommonEncodeOptions, EncodeOptions, MetadataEmbedOptions, RavifEncodeConfig,
        };

        // Build a 2×2 synthetic image (solid red).
        let data: Vec<u16> = vec![65535, 0, 0, 65535, 0, 0, 65535, 0, 0, 65535, 0, 0];
        let rgb = RgbImage::new(2, 2, data).expect("valid RGB buffer");

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
        let opts = EncodeOptions::AvifRavif(RavifEncodeConfig {
            quality: 60,
            speed: 10,
            common: CommonEncodeOptions {
                metadata: MetadataEmbedOptions {
                    embed_icc: false,
                    ..MetadataEmbedOptions::default()
                },
                ..Default::default()
            },
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

    #[cfg(feature = "avif-decode")]
    #[test]
    fn avif_supports_decode_with_feature() {
        assert!(StandardFormat::Avif.supports_decode());
    }

    #[cfg(feature = "avif-encode")]
    #[test]
    fn avif_supports_encode_with_feature() {
        assert!(StandardFormat::Avif.supports_encode());
    }
}
