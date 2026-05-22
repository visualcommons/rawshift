//! Integration tests for image export with metadata embedding.
//!
//! Tests verify that EXIF and ICC metadata are correctly embedded in
//! exported images when the corresponding options are enabled.
//! These tests use a tiny synthetic RGB image via `encode_rgb_image` to avoid
//! the expensive full RAW decode pipeline.

use rawshift_image::core::image::RgbImage;
use rawshift_image::core::metadata::ImageMetadata;
use rawshift_image::formats::encode_rgb_image;
#[cfg(feature = "avif-encode")]
use rawshift_image::formats::export::AvifOptions;
#[cfg(feature = "jxl-encode")]
use rawshift_image::formats::export::JxlOptions;
use rawshift_image::formats::export::{
    EncodeOptions, JpegOptions, MetadataEmbedOptions, PngOptions, WebPMode, WebPOptions,
};
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
    data.windows(4).any(|w| w == b"EXIF")
}

/// Check if WebP data contains ICC profile chunk
fn webp_has_icc(data: &[u8]) -> bool {
    data.windows(4).any(|w| w == b"ICCP")
}

/// Check if WebP data contains XMP chunk
fn webp_has_xmp(data: &[u8]) -> bool {
    data.windows(4).any(|w| w == b"XMP ")
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
            metadata: MetadataEmbedOptions {
                embed_icc: false,
                ..MetadataEmbedOptions::default()
            },
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
            metadata: MetadataEmbedOptions {
                embed_exif: false,
                ..MetadataEmbedOptions::default()
            },
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
            metadata: MetadataEmbedOptions::default(),
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
            metadata: MetadataEmbedOptions {
                embed_exif: false,
                embed_icc: false,
                ..MetadataEmbedOptions::default()
            },
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
            metadata: MetadataEmbedOptions {
                embed_exif: false,
                embed_icc: false,
                ..MetadataEmbedOptions::default()
            },
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
            metadata: MetadataEmbedOptions {
                embed_exif: false,
                embed_icc: false,
                ..MetadataEmbedOptions::default()
            },
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
    fn test_webp_export_lossy_with_exif() {
        let img = synthetic_image();
        let path = temp_path("export_lossy_exif.webp");

        let mut opts = WebPOptions::lossy();
        opts.metadata.embed_icc = false;
        opts.metadata.embed_xmp = false;
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
    fn test_webp_export_lossless() {
        let img = synthetic_image();
        let path = temp_path("export_lossless.webp");

        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::webp_lossless(),
        )
        .expect("Export lossless WebP");

        let data = fs::read(&path).expect("Read WebP");
        assert_eq!(&data[0..4], b"RIFF", "Should start with RIFF");
        assert_eq!(&data[8..12], b"WEBP", "Should have WEBP FourCC");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_webp_export_without_metadata() {
        let img = synthetic_image();
        let path = temp_path("export_no_meta.webp");

        let mut opts = WebPOptions::lossy();
        opts.metadata.embed_exif = false;
        opts.metadata.embed_icc = false;
        opts.metadata.embed_xmp = false;
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::WebP(opts),
        )
        .expect("Export WebP");

        let data = fs::read(&path).expect("Read WebP");
        assert!(!webp_has_exif(&data), "WebP should NOT contain EXIF");
        assert!(!webp_has_icc(&data), "WebP should NOT contain ICC");
        assert!(!webp_has_xmp(&data), "WebP should NOT contain XMP");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_webp_export_with_icc() {
        let img = synthetic_image();
        let path = temp_path("export_icc.webp");

        let mut opts = WebPOptions::lossy();
        opts.metadata.embed_exif = false;
        opts.metadata.embed_xmp = false;
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::WebP(opts),
        )
        .expect("Export WebP");

        let data = fs::read(&path).expect("Read WebP");
        assert!(webp_has_icc(&data), "WebP should contain ICC profile");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_webp_export_with_xmp() {
        let img = synthetic_image();
        let path = temp_path("export_xmp.webp");

        let meta = ImageMetadata {
            xmp: Some(b"<x:xmpmeta>test</x:xmpmeta>".to_vec()),
            ..Default::default()
        };

        let mut opts = WebPOptions::lossy();
        opts.metadata.embed_exif = false;
        opts.metadata.embed_icc = false;
        encode_rgb_image(&img, &meta, &path, &EncodeOptions::WebP(opts)).expect("Export WebP");

        let data = fs::read(&path).expect("Read WebP");
        assert!(webp_has_xmp(&data), "WebP should contain XMP metadata");

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
            &EncodeOptions::webp_lossy(),
        )
        .expect("Export WebP");

        let data = fs::read(&path).expect("Read WebP");
        assert!(webp_has_exif(&data), "Default WebP should contain EXIF");
        assert!(webp_has_icc(&data), "Default WebP should contain ICC");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_webp_quality_affects_file_size() {
        let data: Vec<u16> = (0..64 * 64 * 3)
            .map(|i| ((i * 997) % 65536) as u16)
            .collect();
        let img = RgbImage::new(64, 64, data);

        let path_low = temp_path("webp_quality_low.webp");
        let path_high = temp_path("webp_quality_high.webp");

        let mut opts_low = WebPOptions::lossy();
        opts_low.quality = 10.0;
        opts_low.metadata.embed_exif = false;
        opts_low.metadata.embed_icc = false;
        opts_low.metadata.embed_xmp = false;
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path_low,
            &EncodeOptions::WebP(opts_low),
        )
        .expect("Export low quality");

        let mut opts_high = WebPOptions::lossy();
        opts_high.quality = 95.0;
        opts_high.metadata.embed_exif = false;
        opts_high.metadata.embed_icc = false;
        opts_high.metadata.embed_xmp = false;
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path_high,
            &EncodeOptions::WebP(opts_high),
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
            bit_depth: rawshift_image::formats::export::BitDepth::Eight,
            metadata: MetadataEmbedOptions::default(),
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

/// Check if AVIF data contains a `colr rICC` or `colr prof` box (embedded ICC profile).
#[cfg(feature = "avif-encode")]
fn avif_has_icc(data: &[u8]) -> bool {
    data.windows(8)
        .any(|w| &w[..4] == b"colr" && (&w[4..8] == b"rICC" || &w[4..8] == b"prof"))
}

/// Check if JXL container data contains an `iccp` box.
#[cfg(feature = "jxl-encode")]
fn jxl_has_icc(data: &[u8]) -> bool {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let sz = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        if &data[pos + 4..pos + 8] == b"iccp" {
            return true;
        }
        if sz < 8 {
            break;
        }
        pos += sz;
    }
    false
}

/// Check if data is a JXL container (has "JXL " signature at offset 4).
#[cfg(feature = "jxl-encode")]
fn jxl_is_container(data: &[u8]) -> bool {
    data.get(4..8) == Some(b"JXL ")
}

// ============================================================================
// AVIF Export Tests
// ============================================================================

#[cfg(feature = "avif-encode")]
mod avif_tests {
    use super::*;

    #[test]
    fn test_avif_export_with_icc_enabled() {
        let img = synthetic_image();
        let path = temp_path("avif_icc_on.avif");
        let opts = AvifOptions {
            metadata: MetadataEmbedOptions {
                embed_exif: false,
                ..MetadataEmbedOptions::default()
            },
            ..AvifOptions::default()
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Avif(opts),
        )
        .expect("Export AVIF");
        let data = fs::read(&path).expect("Read AVIF");
        assert!(avif_has_icc(&data), "AVIF should contain ICC profile");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_avif_export_with_icc_disabled() {
        let img = synthetic_image();
        let path = temp_path("avif_icc_off.avif");
        let opts = AvifOptions {
            metadata: MetadataEmbedOptions {
                embed_exif: false,
                embed_icc: false,
                ..MetadataEmbedOptions::default()
            },
            ..AvifOptions::default()
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Avif(opts),
        )
        .expect("Export AVIF");
        let data = fs::read(&path).expect("Read AVIF");
        assert!(!avif_has_icc(&data), "AVIF should NOT contain ICC profile");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_avif_export_with_icc_and_exif() {
        let img = synthetic_image();
        let path = temp_path("avif_icc_exif.avif");
        let opts = AvifOptions {
            metadata: MetadataEmbedOptions::default(),
            ..AvifOptions::default()
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Avif(opts),
        )
        .expect("Export AVIF");
        let data = fs::read(&path).expect("Read AVIF");
        assert!(
            avif_has_icc(&data),
            "AVIF should still contain ICC after EXIF embedding"
        );
        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_avif_default_options_embed_icc() {
        let img = synthetic_image();
        let path = temp_path("avif_default.avif");
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::avif(),
        )
        .expect("Export AVIF");
        let data = fs::read(&path).expect("Read AVIF");
        assert!(avif_has_icc(&data), "Default AVIF should embed ICC");
        fs::remove_file(&path).ok();
    }
}

// ============================================================================
// JXL Export Tests
// ============================================================================

#[cfg(feature = "jxl-encode")]
mod jxl_tests {
    use super::*;

    #[test]
    fn test_jxl_options_default_has_embed_icc() {
        assert!(
            JxlOptions::default().metadata.embed_icc,
            "JxlOptions default should have embed_icc=true"
        );
    }

    #[test]
    fn test_jxl_export_with_icc_enabled() {
        let img = synthetic_image();
        let path = temp_path("jxl_icc_on.jxl");
        let opts = JxlOptions {
            metadata: MetadataEmbedOptions {
                embed_exif: false,
                ..MetadataEmbedOptions::default()
            },
            ..JxlOptions::default()
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Jxl(opts),
        )
        .expect("Export JXL");
        let data = fs::read(&path).expect("Read JXL");
        assert!(jxl_is_container(&data), "JXL should be container format");
        assert!(jxl_has_icc(&data), "JXL should contain ICC profile");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_jxl_export_with_icc_disabled() {
        let img = synthetic_image();
        let path = temp_path("jxl_icc_off.jxl");
        let opts = JxlOptions {
            metadata: MetadataEmbedOptions {
                embed_exif: false,
                embed_icc: false,
                ..MetadataEmbedOptions::default()
            },
            ..JxlOptions::default()
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Jxl(opts),
        )
        .expect("Export JXL");
        let data = fs::read(&path).expect("Read JXL");
        assert!(!jxl_has_icc(&data), "JXL should NOT contain ICC profile");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_jxl_export_with_icc_and_exif() {
        let img = synthetic_image();
        let path = temp_path("jxl_icc_exif.jxl");
        let opts = JxlOptions {
            metadata: MetadataEmbedOptions::default(),
            ..JxlOptions::default()
        };
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::Jxl(opts),
        )
        .expect("Export JXL");
        let data = fs::read(&path).expect("Read JXL");
        assert!(jxl_has_icc(&data), "JXL should contain ICC");
        assert!(
            data.windows(4).any(|w| w == b"Exif"),
            "JXL should contain Exif box"
        );
        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_jxl_default_options_embed_icc() {
        let img = synthetic_image();
        let path = temp_path("jxl_default.jxl");
        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &EncodeOptions::jxl(),
        )
        .expect("Export JXL");
        let data = fs::read(&path).expect("Read JXL");
        assert!(jxl_has_icc(&data), "Default JXL should embed ICC");
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
        let _ = EncodeOptions::webp_lossy();
        let _ = EncodeOptions::webp_lossless();
        #[cfg(feature = "avif-encode")]
        let _ = EncodeOptions::avif();
        #[cfg(feature = "jxl-encode")]
        let _ = EncodeOptions::jxl();
        #[cfg(feature = "dng-encode")]
        let _ = EncodeOptions::dng();
    }

    #[test]
    fn test_jpeg_options_defaults() {
        let opts = JpegOptions::default();
        assert_eq!(opts.quality, 90, "JPEG default quality should be 90");
        assert!(
            opts.metadata.embed_exif,
            "JPEG should embed EXIF by default"
        );
        assert!(opts.metadata.embed_icc, "JPEG should embed ICC by default");
    }

    #[test]
    fn test_webp_options_defaults() {
        let opts = WebPOptions::default();
        assert_eq!(
            opts.mode,
            WebPMode::Lossy,
            "WebP default mode should be Lossy"
        );
        assert!(
            (opts.quality - 75.0).abs() < f32::EPSILON,
            "WebP default quality should be 75"
        );
        assert_eq!(opts.method, 4, "WebP default method should be 4");
        assert_eq!(
            opts.near_lossless, 100,
            "WebP default near_lossless should be 100"
        );
        assert!(
            opts.metadata.embed_exif,
            "WebP should embed EXIF by default"
        );
        assert!(opts.metadata.embed_icc, "WebP should embed ICC by default");
        assert!(opts.metadata.embed_xmp, "WebP should embed XMP by default");
    }

    #[test]
    fn test_webp_named_constructors() {
        let lossy = WebPOptions::lossy();
        assert_eq!(lossy.mode, WebPMode::Lossy);

        let lossless = WebPOptions::lossless();
        assert_eq!(lossless.mode, WebPMode::Lossless);
    }
}
