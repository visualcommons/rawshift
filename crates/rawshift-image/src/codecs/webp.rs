//! Safe wrappers around libwebp FFI for WebP encoding, decoding, and muxing.

use std::ffi::c_int;
use std::os::raw::c_void;
use std::ptr;
use std::slice;

use libwebp_sys::*;

/// Decode a WebP bitstream into RGB pixels.
///
/// Returns `(width, height, rgb_pixels)`. Alpha is always stripped.
pub fn decode_webp_rgb(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut width: c_int = 0;
    let mut height: c_int = 0;

    // Validate and get dimensions
    let ok = unsafe { WebPGetInfo(data.as_ptr(), data.len(), &mut width, &mut height) };
    if ok == 0 {
        return Err("WebPGetInfo failed: invalid WebP data".into());
    }
    if width <= 0 || height <= 0 {
        return Err(format!("Invalid dimensions: {width}x{height}"));
    }

    let w = width as u32;
    let h = height as u32;

    // Decode to RGB (alpha stripped by libwebp)
    let ptr = unsafe { WebPDecodeRGB(data.as_ptr(), data.len(), &mut width, &mut height) };
    if ptr.is_null() {
        return Err("WebPDecodeRGB failed".into());
    }

    let len = (w as usize) * (h as usize) * 3;
    let pixels = unsafe { slice::from_raw_parts(ptr, len) }.to_vec();
    unsafe { WebPFree(ptr as *mut c_void) };

    Ok((w, h, pixels))
}

/// Build a `WebPConfig` from our high-level options.
pub fn build_webp_config(
    lossless: bool,
    quality: f32,
    method: u32,
    near_lossless: u32,
) -> Result<WebPConfig, String> {
    let mut config = WebPConfig::new_with_preset(WebPPreset::WEBP_PRESET_DEFAULT, quality)
        .map_err(|()| "WebPConfigInit failed")?;

    config.lossless = if lossless { 1 } else { 0 };
    config.quality = quality;
    config.method = method as c_int;
    if lossless {
        config.near_lossless = near_lossless as c_int;
    }

    let valid = unsafe { WebPValidateConfig(&config) };
    if valid == 0 {
        return Err("WebPValidateConfig failed: invalid config".into());
    }

    Ok(config)
}

/// Encode RGB pixel data to a WebP bitstream.
///
/// `pixels` must be `width * height * 3` bytes of packed RGB.
pub fn encode_webp_rgb(
    pixels: &[u8],
    width: u32,
    height: u32,
    config: &WebPConfig,
) -> Result<Vec<u8>, String> {
    let expected_len = (width as usize) * (height as usize) * 3;
    if pixels.len() != expected_len {
        return Err(format!(
            "pixel buffer length mismatch: expected {expected_len}, got {}",
            pixels.len()
        ));
    }

    unsafe {
        let mut picture: WebPPicture = std::mem::zeroed();
        if WebPPictureInitInternal(&mut picture, WEBP_ENCODER_ABI_VERSION as c_int) == 0 {
            return Err("WebPPictureInit failed".into());
        }

        picture.width = width as c_int;
        picture.height = height as c_int;
        picture.use_argb = if config.lossless != 0 { 1 } else { 0 };

        // Set up memory writer
        let mut writer: WebPMemoryWriter = std::mem::zeroed();
        WebPMemoryWriterInit(&mut writer);
        picture.writer = Some(WebPMemoryWrite);
        picture.custom_ptr = &mut writer as *mut WebPMemoryWriter as *mut c_void;

        // Import RGB data
        let stride = (width * 3) as c_int;
        if WebPPictureImportRGB(&mut picture, pixels.as_ptr(), stride) == 0 {
            WebPPictureFree(&mut picture);
            WebPMemoryWriterClear(&mut writer);
            return Err("WebPPictureImportRGB failed".into());
        }

        // Encode
        let ok = WebPEncode(config, &mut picture);
        if ok == 0 {
            let err_code = picture.error_code;
            WebPPictureFree(&mut picture);
            WebPMemoryWriterClear(&mut writer);
            return Err(format!("WebPEncode failed: error code {:?}", err_code));
        }

        // Copy output
        let output = slice::from_raw_parts(writer.mem, writer.size).to_vec();

        WebPPictureFree(&mut picture);
        WebPMemoryWriterClear(&mut writer);

        Ok(output)
    }
}

/// Mux a WebP bitstream with optional EXIF, ICC, and XMP metadata chunks.
///
/// Returns the assembled WebP file bytes including all metadata.
/// If no metadata is provided, returns the original bitstream unchanged.
pub fn mux_webp(
    bitstream: &[u8],
    exif: Option<&[u8]>,
    icc: Option<&[u8]>,
    xmp: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    if exif.is_none() && icc.is_none() && xmp.is_none() {
        return Ok(bitstream.to_vec());
    }

    unsafe {
        let mux = WebPMuxNew();
        if mux.is_null() {
            return Err("WebPMuxNew failed".into());
        }

        // Set the image bitstream
        let image_data = WebPData {
            bytes: bitstream.as_ptr(),
            size: bitstream.len(),
        };
        let rc = WebPMuxSetImage(mux, &image_data, 1);
        if rc != WebPMuxError::WEBP_MUX_OK {
            WebPMuxDelete(mux);
            return Err(format!("WebPMuxSetImage failed: {:?}", rc));
        }

        // Set EXIF chunk
        if let Some(data) = exif {
            let chunk_data = WebPData {
                bytes: data.as_ptr(),
                size: data.len(),
            };
            let rc = WebPMuxSetChunk(mux, b"EXIF".as_ptr() as *const i8, &chunk_data, 1);
            if rc != WebPMuxError::WEBP_MUX_OK {
                WebPMuxDelete(mux);
                return Err(format!("WebPMuxSetChunk(EXIF) failed: {:?}", rc));
            }
        }

        // Set ICC profile chunk
        if let Some(data) = icc {
            let chunk_data = WebPData {
                bytes: data.as_ptr(),
                size: data.len(),
            };
            let rc = WebPMuxSetChunk(mux, b"ICCP".as_ptr() as *const i8, &chunk_data, 1);
            if rc != WebPMuxError::WEBP_MUX_OK {
                WebPMuxDelete(mux);
                return Err(format!("WebPMuxSetChunk(ICCP) failed: {:?}", rc));
            }
        }

        // Set XMP chunk
        if let Some(data) = xmp {
            let chunk_data = WebPData {
                bytes: data.as_ptr(),
                size: data.len(),
            };
            let rc = WebPMuxSetChunk(mux, b"XMP ".as_ptr() as *const i8, &chunk_data, 1);
            if rc != WebPMuxError::WEBP_MUX_OK {
                WebPMuxDelete(mux);
                return Err(format!("WebPMuxSetChunk(XMP) failed: {:?}", rc));
            }
        }

        // Assemble
        let mut output_data: WebPData = WebPData {
            bytes: ptr::null(),
            size: 0,
        };
        let rc = WebPMuxAssemble(mux, &mut output_data);
        if rc != WebPMuxError::WEBP_MUX_OK {
            WebPMuxDelete(mux);
            return Err(format!("WebPMuxAssemble failed: {:?}", rc));
        }

        let result = slice::from_raw_parts(output_data.bytes, output_data.size).to_vec();

        WebPDataClear(&mut output_data);
        WebPMuxDelete(mux);

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_config_lossy() {
        let config = build_webp_config(false, 75.0, 4, 100).expect("config should be valid");
        assert_eq!(config.lossless, 0);
        assert!((config.quality - 75.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_build_config_lossless() {
        let config = build_webp_config(true, 75.0, 4, 80).expect("config should be valid");
        assert_eq!(config.lossless, 1);
        assert_eq!(config.near_lossless, 80);
    }

    #[test]
    fn test_lossless_roundtrip() {
        // 2x2 red/green/blue/white image
        let pixels: Vec<u8> = vec![
            255, 0, 0, 0, 255, 0, // row 0
            0, 0, 255, 255, 255, 255, // row 1
        ];
        let config = build_webp_config(true, 75.0, 4, 100).unwrap();
        let encoded = encode_webp_rgb(&pixels, 2, 2, &config).expect("encode");
        assert!(!encoded.is_empty());

        let (w, h, decoded) = decode_webp_rgb(&encoded).expect("decode");
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(decoded, pixels, "lossless round-trip should be exact");
    }

    #[test]
    fn test_lossy_encode_decode() {
        let pixels: Vec<u8> = vec![128; 4 * 4 * 3];
        let config = build_webp_config(false, 50.0, 4, 100).unwrap();
        let encoded = encode_webp_rgb(&pixels, 4, 4, &config).expect("encode");
        assert!(!encoded.is_empty());

        let (w, h, decoded) = decode_webp_rgb(&encoded).expect("decode");
        assert_eq!(w, 4);
        assert_eq!(h, 4);
        assert_eq!(decoded.len(), 4 * 4 * 3);
    }

    #[test]
    fn test_mux_with_exif() {
        // Encode a minimal image first
        let pixels: Vec<u8> = vec![100; 2 * 2 * 3];
        let config = build_webp_config(true, 75.0, 0, 100).unwrap();
        let bitstream = encode_webp_rgb(&pixels, 2, 2, &config).unwrap();

        let fake_exif = b"Exif\x00\x00test_exif_data";
        let muxed = mux_webp(&bitstream, Some(fake_exif), None, None).expect("mux");

        // Should be a valid RIFF/WEBP container
        assert_eq!(&muxed[0..4], b"RIFF");
        assert_eq!(&muxed[8..12], b"WEBP");
        // Should contain EXIF fourcc
        let has_exif = muxed.windows(4).any(|w| w == b"EXIF");
        assert!(has_exif, "muxed output should contain EXIF chunk");
    }

    #[test]
    fn test_mux_with_icc() {
        let pixels: Vec<u8> = vec![100; 2 * 2 * 3];
        let config = build_webp_config(true, 75.0, 0, 100).unwrap();
        let bitstream = encode_webp_rgb(&pixels, 2, 2, &config).unwrap();

        let fake_icc = b"fake_icc_profile_data";
        let muxed = mux_webp(&bitstream, None, Some(fake_icc), None).expect("mux");

        let has_iccp = muxed.windows(4).any(|w| w == b"ICCP");
        assert!(has_iccp, "muxed output should contain ICCP chunk");
    }

    #[test]
    fn test_mux_with_xmp() {
        let pixels: Vec<u8> = vec![100; 2 * 2 * 3];
        let config = build_webp_config(true, 75.0, 0, 100).unwrap();
        let bitstream = encode_webp_rgb(&pixels, 2, 2, &config).unwrap();

        let fake_xmp = b"<x:xmpmeta>test</x:xmpmeta>";
        let muxed = mux_webp(&bitstream, None, None, Some(fake_xmp)).expect("mux");

        let has_xmp = muxed.windows(4).any(|w| w == b"XMP ");
        assert!(has_xmp, "muxed output should contain XMP chunk");
    }

    #[test]
    fn test_mux_no_metadata_returns_original() {
        let data = b"some_data";
        let result = mux_webp(data, None, None, None).unwrap();
        assert_eq!(result, data);
    }

    #[test]
    fn test_decode_invalid_data() {
        let result = decode_webp_rgb(b"not a webp");
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_buffer_mismatch() {
        let pixels = vec![0u8; 10]; // wrong size for any image
        let config = build_webp_config(false, 75.0, 4, 100).unwrap();
        let result = encode_webp_rgb(&pixels, 2, 2, &config);
        assert!(result.is_err());
    }
}
