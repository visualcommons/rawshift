//! Integration tests for image export with metadata embedding.
//!
//! Tests verify that EXIF and ICC metadata are correctly embedded in
//! exported images when the corresponding options are enabled.
//! These tests use a tiny synthetic RGB image via `encode_rgb_image` to avoid
//! the expensive full RAW decode pipeline.

use rawshift::core::image::RgbImage;
use rawshift::core::metadata::ImageMetadata;
use rawshift::formats::encode_rgb_image;
use rawshift::formats::export::{EncodeOptions, JpegOptions, PngOptions, WebPOptions};
use std::fs;
use std::path::PathBuf;

/// 4×4 grey synthetic RGB image (16-bit, tone-mapped already).
fn synthetic_image() -> RgbImage {
    // Mid-grey at ~50% sRGB after tone mapping
    RgbImage::new(4, 4, vec![32768u16; 4 * 4 * 3])
}

/// Get a temporary file path for test output
fn temp_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("rawshift_test_{}", name));
    path
}

/// Check if JPEG data contains EXIF APP1 marker
fn jpeg_has_exif(data: &[u8]) -> bool {
    for i in 0..data.len().saturating_sub(10) {
        if data[i] == 0xFF
            && data[i + 1] == 0xE1
            && i + 8 < data.len()
            && &data[i + 4..i + 8] == b"Exif"
        {
            return true;
        }
    }
    false
}

/// Check if JPEG data contains ICC APP2 marker
fn jpeg_has_icc(data: &[u8]) -> bool {
    for i in 0..data.len().saturating_sub(16) {
        if data[i] == 0xFF
            && data[i + 1] == 0xE2
            && i + 15 < data.len()
            && &data[i + 4..i + 15] == b"ICC_PROFILE"
        {
            return true;
        }
    }
    false
}

/// Check if WebP data contains EXIF chunk
fn webp_has_exif(data: &[u8]) -> bool {
    for i in 0..data.len().saturating_sub(4) {
        if &data[i..i + 4] == b"EXIF" {
            return true;
        }
    }
    false
}

// ============================================================================
// JPEG Export Tests
// ============================================================================

mod jpeg_tests {
    use super::*;

    #[test]
    fn test_jpeg_export_with_exif_enabled() {
        let img = synthetic_image();
        let path = temp_path("export_with_exif.jpg");

        let opts = JpegOptions {
            quality: 85,
            embed_exif: true,
            embed_icc: false,
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Jpeg(opts),
        )
        .expect("Export JPEG");

        let data = fs::read(&path).expect("Read JPEG");
        assert_eq!(&data[0..2], &[0xFF, 0xD8], "Should be valid JPEG");
        assert!(jpeg_has_exif(&data), "JPEG should contain EXIF metadata");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_jpeg_export_with_icc_enabled() {
        let img = synthetic_image();
        let path = temp_path("export_with_icc.jpg");

        let opts = JpegOptions {
            quality: 85,
            embed_exif: false,
            embed_icc: true,
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Jpeg(opts),
        )
        .expect("Export JPEG");

        let data = fs::read(&path).expect("Read JPEG");
        assert!(jpeg_has_icc(&data), "JPEG should contain ICC profile");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_jpeg_export_with_both_exif_and_icc() {
        let img = synthetic_image();
        let path = temp_path("export_with_both.jpg");

        let opts = JpegOptions {
            quality: 90,
            embed_exif: true,
            embed_icc: true,
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Jpeg(opts),
        )
        .expect("Export JPEG");

        let data = fs::read(&path).expect("Read JPEG");
        assert!(jpeg_has_exif(&data), "JPEG should contain EXIF");
        assert!(jpeg_has_icc(&data), "JPEG should contain ICC");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_jpeg_export_without_metadata() {
        let img = synthetic_image();
        let path = temp_path("export_no_meta.jpg");

        let opts = JpegOptions {
            quality: 85,
            embed_exif: false,
            embed_icc: false,
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Jpeg(opts),
        )
        .expect("Export JPEG");

        let data = fs::read(&path).expect("Read JPEG");
        assert!(!jpeg_has_exif(&data), "JPEG should NOT contain EXIF");
        assert!(!jpeg_has_icc(&data), "JPEG should NOT contain ICC");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_jpeg_export_default_options() {
        let img = synthetic_image();
        let path = temp_path("export_default.jpg");

        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::jpeg(),
        )
        .expect("Export JPEG");

        let data = fs::read(&path).expect("Read JPEG");
        assert!(jpeg_has_exif(&data), "Default JPEG should contain EXIF");
        assert!(jpeg_has_icc(&data), "Default JPEG should contain ICC");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_jpeg_quality_affects_file_size() {
        // Use a 64×64 image with varied content so quality differences are visible.
        let data: Vec<u16> = (0..64 * 64 * 3)
            .map(|i| ((i * 997) % 65536) as u16)
            .collect();
        let img = RgbImage::new(64, 64, data);

        let path_low = temp_path("quality_low.jpg");
        let path_high = temp_path("quality_high.jpg");

        let opts_low = JpegOptions {
            quality: 30,
            embed_exif: false,
            embed_icc: false,
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path_low,
            &EncodeOptions::Jpeg(opts_low),
        )
        .expect("Export low quality");

        let opts_high = JpegOptions {
            quality: 95,
            embed_exif: false,
            embed_icc: false,
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path_high,
            &EncodeOptions::Jpeg(opts_high),
        )
        .expect("Export high quality");

        let size_low = fs::metadata(&path_low).expect("Get size").len();
        let size_high = fs::metadata(&path_high).expect("Get size").len();

        assert!(
            size_high > size_low,
            "High quality ({}) should be larger than low quality ({})",
            size_high,
            size_low
        );

        fs::remove_file(&path_low).ok();
        fs::remove_file(&path_high).ok();
    }
}

// ============================================================================
// WebP Export Tests
// ============================================================================

mod webp_tests {
    use super::*;

    #[test]
    fn test_webp_export_with_exif_enabled() {
        let img = synthetic_image();
        let path = temp_path("export_with_exif.webp");

        let opts = WebPOptions {
            quality: 80.0,
            lossless: true,
            embed_exif: true,
            embed_icc: false,
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::WebP(opts),
        )
        .expect("Export WebP");

        let data = fs::read(&path).expect("Read WebP");
        assert_eq!(&data[0..4], b"RIFF", "Should start with RIFF");
        assert_eq!(&data[8..12], b"WEBP", "Should have WEBP FourCC");
        assert!(webp_has_exif(&data), "WebP should contain EXIF metadata");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_webp_export_without_exif() {
        let img = synthetic_image();
        let path = temp_path("export_no_exif.webp");

        let opts = WebPOptions {
            quality: 80.0,
            lossless: true,
            embed_exif: false,
            embed_icc: false,
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::WebP(opts),
        )
        .expect("Export WebP");

        let data = fs::read(&path).expect("Read WebP");
        assert!(!webp_has_exif(&data), "WebP should NOT contain EXIF");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_webp_export_default_options() {
        let img = synthetic_image();
        let path = temp_path("export_default.webp");

        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::webp(),
        )
        .expect("Export WebP");

        let data = fs::read(&path).expect("Read WebP");
        assert!(webp_has_exif(&data), "Default WebP should contain EXIF");

        fs::remove_file(&path).ok();
    }
}

// ============================================================================
// PNG Export Tests
// ============================================================================

mod png_tests {
    use super::*;

    #[test]
    fn test_png_export_basic() {
        let img = synthetic_image();
        let path = temp_path("export_basic.png");

        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::png(),
        )
        .expect("Export PNG");

        let data = fs::read(&path).expect("Read PNG");
        assert_eq!(
            &data[0..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
            "Should have PNG signature"
        );

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_png_export_8bit() {
        let img = synthetic_image();
        let path = temp_path("export_8bit.png");

        let opts = PngOptions {
            bit_depth: zune_core::bit_depth::BitDepth::Eight,
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Png(opts),
        )
        .expect("Export 8-bit PNG");

        assert!(path.exists());
        fs::remove_file(&path).ok();
    }
}

// ============================================================================
// EncodeOptions API Tests
// ============================================================================

mod encode_options_tests {
    use super::*;

    #[test]
    fn test_encode_options_constructors() {
        let _ = EncodeOptions::png();
        let _ = EncodeOptions::jpeg();
        let _ = EncodeOptions::webp();
        #[cfg(feature = "avif")]
        let _ = EncodeOptions::avif();
        #[cfg(feature = "jxl-encode")]
        let _ = EncodeOptions::jxl();
        let _ = EncodeOptions::dng();
    }

    #[test]
    fn test_jpeg_options_defaults() {
        let opts = JpegOptions::default();
        assert_eq!(opts.quality, 90, "JPEG default quality should be 90");
        assert!(opts.embed_exif, "JPEG should embed EXIF by default");
        assert!(opts.embed_icc, "JPEG should embed ICC by default");
    }

    #[test]
    fn test_webp_options_defaults() {
        let opts = WebPOptions::default();
        assert_eq!(opts.quality, 80.0, "WebP default quality should be 80");
        assert!(opts.lossless, "WebP should be lossless by default");
        assert!(opts.embed_exif, "WebP should embed EXIF by default");
        assert!(opts.embed_icc, "WebP should embed ICC by default");
    }
}
