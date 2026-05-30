//! Integration tests for image export with metadata embedding.
//!
//! Tests verify that EXIF/ICC/XMP metadata is correctly embedded in exported
//! images, that the in-memory encode entry points work for every format, and
//! that the decode-side color/probe APIs behave as documented. They use a tiny
//! synthetic RGB image to avoid the expensive full RAW decode pipeline.

use rawshift_image::core::image::RgbImage;
use rawshift_image::core::metadata::ImageMetadata;
use rawshift_image::formats::export::{
    BitDepth, CommonEncodeOptions, EncodeOptions, JpegEncEncodeConfig, LibwebpEncodeConfig,
    MetadataEmbedOptions, WebPMode, ZunePngEncodeConfig,
};
use rawshift_image::formats::{encode_rgb_image, encode_rgb_image_to_vec};
use std::fs;
use std::path::PathBuf;

/// 4×4 grey synthetic RGB image (16-bit, tone-mapped already).
fn synthetic_image() -> RgbImage {
    RgbImage::new(4, 4, vec![32768u16; 4 * 4 * 3])
}

/// Get a temporary file path for test output.
fn temp_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("rawshift_test_{}", name));
    path
}

/// `CommonEncodeOptions` with the given metadata-embed flags and default depth.
fn common(embed_exif: bool, embed_icc: bool, embed_xmp: bool) -> CommonEncodeOptions {
    CommonEncodeOptions {
        metadata: MetadataEmbedOptions {
            embed_exif,
            embed_icc,
            embed_xmp,
        },
        bit_depth: BitDepth::Sixteen,
    }
}

/// Check if JPEG data contains an EXIF APP1 marker.
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

/// Check if JPEG data contains an ICC APP2 marker.
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

fn webp_has_exif(data: &[u8]) -> bool {
    data.windows(4).any(|w| w == b"EXIF")
}

fn webp_has_icc(data: &[u8]) -> bool {
    data.windows(4).any(|w| w == b"ICCP")
}

fn webp_has_xmp(data: &[u8]) -> bool {
    data.windows(4).any(|w| w == b"XMP ")
}

// ============================================================================
// JPEG Export Tests
// ============================================================================

mod jpeg_tests {
    use super::*;

    fn jpeg(quality: u8, exif: bool, icc: bool) -> EncodeOptions {
        EncodeOptions::JpegJpegEnc(JpegEncEncodeConfig {
            quality,
            common: common(exif, icc, true),
        })
    }

    #[test]
    fn test_jpeg_export_with_exif_enabled() {
        let img = synthetic_image();
        let path = temp_path("export_with_exif.jpg");

        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &jpeg(85, true, false),
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

        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &jpeg(85, false, true),
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

        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &jpeg(90, true, true),
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

        encode_rgb_image(
            &img,
            &ImageMetadata::default(),
            &path,
            &jpeg(85, false, false),
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
        let data: Vec<u16> = (0..64 * 64 * 3)
            .map(|i| ((i * 997) % 65536) as u16)
            .collect();
        let img = RgbImage::new(64, 64, data);

        let low = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &jpeg(30, false, false))
            .expect("Export low quality");
        let high =
            encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &jpeg(95, false, false))
                .expect("Export high quality");

        assert!(
            high.len() > low.len(),
            "High quality ({}) should be larger than low quality ({})",
            high.len(),
            low.len()
        );
    }
}

// ============================================================================
// WebP Export Tests
// ============================================================================

mod webp_tests {
    use super::*;

    fn webp(exif: bool, icc: bool, xmp: bool) -> LibwebpEncodeConfig {
        LibwebpEncodeConfig {
            common: common(exif, icc, xmp),
            ..LibwebpEncodeConfig::lossy()
        }
    }

    #[test]
    fn test_webp_export_lossy_with_exif() {
        let img = synthetic_image();
        let path = temp_path("export_lossy_exif.webp");

        let opts = EncodeOptions::WebpLibwebp(webp(true, false, false));
        encode_rgb_image(&img, &ImageMetadata::default(), &path, &opts).expect("Export WebP");

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

        let opts = EncodeOptions::WebpLibwebp(webp(false, false, false));
        encode_rgb_image(&img, &ImageMetadata::default(), &path, &opts).expect("Export WebP");

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

        let opts = EncodeOptions::WebpLibwebp(webp(false, true, false));
        encode_rgb_image(&img, &ImageMetadata::default(), &path, &opts).expect("Export WebP");

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

        let opts = EncodeOptions::WebpLibwebp(webp(false, false, true));
        encode_rgb_image(&img, &meta, &path, &opts).expect("Export WebP");

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

        let mut low = webp(false, false, false);
        low.quality = 10.0;
        let mut high = webp(false, false, false);
        high.quality = 95.0;

        let low = encode_rgb_image_to_vec(
            &img,
            &ImageMetadata::default(),
            &EncodeOptions::WebpLibwebp(low),
        )
        .expect("Export low quality");
        let high = encode_rgb_image_to_vec(
            &img,
            &ImageMetadata::default(),
            &EncodeOptions::WebpLibwebp(high),
        )
        .expect("Export high quality");

        assert!(
            high.len() > low.len(),
            "High quality ({}) should be larger than low quality ({})",
            high.len(),
            low.len()
        );
    }
}

// ============================================================================
// PNG Export Tests
// ============================================================================

mod png_tests {
    use super::*;

    const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

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
        assert_eq!(&data[0..8], &PNG_SIGNATURE, "Should have PNG signature");

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_png_export_8bit() {
        let img = synthetic_image();

        let opts = EncodeOptions::PngZune(ZunePngEncodeConfig {
            common: CommonEncodeOptions {
                bit_depth: BitDepth::Eight,
                ..CommonEncodeOptions::default()
            },
        });
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts)
            .expect("Export 8-bit PNG");

        assert_eq!(&data[0..8], &PNG_SIGNATURE);
        // IHDR bit-depth byte sits at offset 24.
        assert_eq!(data[24], 8, "PNG should be 8-bit");
    }

    #[test]
    fn test_png_export_16bit() {
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &EncodeOptions::png())
            .expect("Export 16-bit PNG");
        assert_eq!(data[24], 16, "default PNG should be 16-bit");
    }
}

/// Check if AVIF data contains a `colr rICC`/`colr prof` box (embedded ICC).
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
    use rawshift_image::formats::export::RavifEncodeConfig;

    fn avif(exif: bool, icc: bool) -> EncodeOptions {
        EncodeOptions::AvifRavif(RavifEncodeConfig {
            common: common(exif, icc, true),
            ..RavifEncodeConfig::default()
        })
    }

    #[test]
    fn test_avif_export_with_icc_enabled() {
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &avif(false, true))
            .expect("Export AVIF");
        assert!(avif_has_icc(&data), "AVIF should contain ICC profile");
    }

    #[test]
    fn test_avif_export_with_icc_disabled() {
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &avif(false, false))
            .expect("Export AVIF");
        assert!(!avif_has_icc(&data), "AVIF should NOT contain ICC profile");
    }

    #[test]
    fn test_avif_export_with_icc_and_exif() {
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &avif(true, true))
            .expect("Export AVIF");
        assert!(
            avif_has_icc(&data),
            "AVIF should still contain ICC after EXIF embedding"
        );
    }

    #[test]
    fn test_avif_default_options_embed_icc() {
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &EncodeOptions::avif())
            .expect("Export AVIF");
        assert!(avif_has_icc(&data), "Default AVIF should embed ICC");
    }
}

// ============================================================================
// JXL Export Tests
// ============================================================================

#[cfg(feature = "jxl-encode")]
mod jxl_tests {
    use super::*;
    use rawshift_image::formats::export::ZuneJxlEncodeConfig;

    fn jxl(exif: bool, icc: bool) -> EncodeOptions {
        EncodeOptions::JxlZune(ZuneJxlEncodeConfig {
            common: common(exif, icc, true),
            ..ZuneJxlEncodeConfig::default()
        })
    }

    #[test]
    fn test_jxl_config_default_has_embed_icc() {
        assert!(
            ZuneJxlEncodeConfig::default().common.metadata.embed_icc,
            "ZuneJxlEncodeConfig default should embed ICC"
        );
    }

    #[test]
    fn test_jxl_export_with_icc_enabled() {
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &jxl(false, true))
            .expect("Export JXL");
        assert!(jxl_is_container(&data), "JXL should be container format");
        assert!(jxl_has_icc(&data), "JXL should contain ICC profile");
    }

    #[test]
    fn test_jxl_export_with_icc_disabled() {
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &jxl(false, false))
            .expect("Export JXL");
        assert!(!jxl_has_icc(&data), "JXL should NOT contain ICC profile");
    }

    #[test]
    fn test_jxl_default_options_embed_icc() {
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &EncodeOptions::jxl())
            .expect("Export JXL");
        assert!(jxl_has_icc(&data), "Default JXL should embed ICC");
    }
}

// ============================================================================
// JXL Export Tests — libjxl backend (opt-in `jxl-encode-libjxl`)
// ============================================================================

#[cfg(feature = "jxl-encode-libjxl")]
mod libjxl_tests {
    use super::*;
    use rawshift_image::formats::export::LibjxlEncodeConfig;
    use rawshift_image::formats::{StandardFormat, available_encoders, decode_standard_image};

    /// 16-bit synthetic image with distinct per-sample values (so a lossless
    /// round-trip is a meaningful check). 48 samples, all within `u16`.
    fn distinct_16bit() -> RgbImage {
        let data: Vec<u16> = (0..4 * 4 * 3)
            .map(|i| ((i as u32 * 4099) % 65536) as u16)
            .collect();
        RgbImage::new(4, 4, data)
    }

    #[test]
    fn libjxl_registers_as_encoder() {
        assert!(
            available_encoders().iter().any(|c| c.id.id == "jxl/libjxl"),
            "jxl/libjxl should be listed when the feature is enabled"
        );
    }

    #[test]
    fn libjxl_encodes_and_decodes_roundtrip() {
        let img = synthetic_image();
        let opts = EncodeOptions::JxlLibjxl(LibjxlEncodeConfig {
            common: common(false, false, false),
            ..LibjxlEncodeConfig::default()
        });
        let bytes = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts)
            .expect("encode JXL via libjxl");
        assert!(!bytes.is_empty());
        assert_eq!(
            rawshift_image::formats::detect_standard_format(&bytes),
            Some(StandardFormat::Jxl),
            "libjxl output should be detected as JXL"
        );
        let decoded = decode_standard_image(&bytes, StandardFormat::Jxl).expect("decode JXL");
        assert_eq!(decoded.width(), 4);
        assert_eq!(decoded.height(), 4);
    }

    #[test]
    fn libjxl_lossless_16bit_is_exact() {
        let img = distinct_16bit();
        let want = img.data.clone();
        let opts = EncodeOptions::JxlLibjxl(LibjxlEncodeConfig {
            common: common(false, false, false),
            distance: 0.0,
            lossless: true,
            ..LibjxlEncodeConfig::default()
        });
        let bytes = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts)
            .expect("encode lossless JXL");
        let decoded = decode_standard_image(&bytes, StandardFormat::Jxl).expect("decode JXL");
        assert_eq!(
            decoded.data, want,
            "lossless 16-bit libjxl round-trip must be exact"
        );
    }

    #[test]
    fn libjxl_embeds_icc_when_requested() {
        let img = synthetic_image();
        let opts = EncodeOptions::JxlLibjxl(LibjxlEncodeConfig {
            common: common(false, true, false),
            ..LibjxlEncodeConfig::default()
        });
        let data =
            encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts).expect("encode JXL");
        assert!(
            jxl_has_icc(&data),
            "libjxl JXL should contain an ICC profile box"
        );
    }

    #[test]
    fn libjxl_toggles_encode() {
        // Exercise a spread of typed toggles. (The raw `extra_*_options` escape
        // hatch is covered by the unit test in `codecs::jxl_libjxl`, which has the
        // real `JxlEncoderFrameSettingId` constants in scope.)
        let img = synthetic_image();
        let opts = EncodeOptions::JxlLibjxl(LibjxlEncodeConfig {
            common: common(false, false, false),
            distance: 3.0,
            effort: 4,
            modular: rawshift_image::formats::export::LibjxlModular::Modular,
            progressive: true,
            decoding_speed: 2,
            ..LibjxlEncodeConfig::default()
        });
        let bytes = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts)
            .expect("encode JXL with toggles");
        assert!(!bytes.is_empty());
    }
}

// ============================================================================
// JPEG Export Tests — jpegli backend (opt-in `jpeg-encode-jpegli`)
// ============================================================================

#[cfg(feature = "jpeg-encode-jpegli")]
mod jpegli_tests {
    use super::*;
    use rawshift_image::formats::export::{JpegSubsampling, JpegliEncodeConfig};
    use rawshift_image::formats::{StandardFormat, available_encoders, decode_standard_image};

    /// `CommonEncodeOptions` at 8-bit depth with no metadata.
    fn common_8bit() -> CommonEncodeOptions {
        CommonEncodeOptions {
            metadata: MetadataEmbedOptions {
                embed_exif: false,
                embed_icc: false,
                embed_xmp: false,
            },
            bit_depth: BitDepth::Eight,
        }
    }

    #[test]
    fn jpegli_registers_as_encoder() {
        assert!(
            available_encoders()
                .iter()
                .any(|c| c.id.id == "jpeg/jpegli"),
            "jpeg/jpegli should be listed when the feature is enabled"
        );
    }

    #[test]
    fn jpegli_encodes_and_decodes_roundtrip_8bit() {
        let img = synthetic_image();
        let opts = EncodeOptions::JpegJpegli(JpegliEncodeConfig {
            common: common_8bit(),
            ..JpegliEncodeConfig::default()
        });
        let bytes = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts)
            .expect("encode JPEG via jpegli");
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "jpegli output should be a JPEG");
        assert_eq!(
            rawshift_image::formats::detect_standard_format(&bytes),
            Some(StandardFormat::Jpeg),
            "jpegli output should be detected as JPEG"
        );
        let decoded = decode_standard_image(&bytes, StandardFormat::Jpeg).expect("decode JPEG");
        assert_eq!((decoded.width(), decoded.height()), (4, 4));
    }

    #[test]
    fn jpegli_encodes_from_16bit_input() {
        // The default depth is `Sixteen`, so jpegli is fed full-precision input;
        // the output is still an 8-bit JPEG that decodes at the right size.
        let img = synthetic_image();
        let opts = EncodeOptions::JpegJpegli(JpegliEncodeConfig {
            common: common(false, false, false),
            ..JpegliEncodeConfig::default()
        });
        let bytes = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts)
            .expect("encode 16-bit input via jpegli");
        let decoded = decode_standard_image(&bytes, StandardFormat::Jpeg).expect("decode JPEG");
        assert_eq!((decoded.width(), decoded.height()), (4, 4));
    }

    #[test]
    fn jpegli_xyb_and_quality_encode() {
        let img = synthetic_image();
        let opts = EncodeOptions::JpegJpegli(JpegliEncodeConfig {
            common: common_8bit(),
            quality: Some(85),
            xyb: true,
            progressive: false,
            subsampling: JpegSubsampling::Yuv444,
            ..JpegliEncodeConfig::default()
        });
        let bytes = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts)
            .expect("encode XYB jpegli");
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn jpegli_embeds_exif_and_icc_when_requested() {
        let img = synthetic_image();
        let opts = EncodeOptions::JpegJpegli(JpegliEncodeConfig {
            common: common(true, true, false),
            ..JpegliEncodeConfig::default()
        });
        let data =
            encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts).expect("encode jpegli");
        assert!(jpeg_has_exif(&data), "jpegli JPEG should contain EXIF");
        assert!(jpeg_has_icc(&data), "jpegli JPEG should contain ICC");
    }
}

// ============================================================================
// EncodeOptions API Tests
// ============================================================================

mod encode_options_tests {
    use super::*;
    use rawshift_image::formats::export::OutputFormat;

    #[test]
    fn test_encode_options_constructors() {
        let _ = EncodeOptions::png();
        let _ = EncodeOptions::jpeg();
        #[cfg(feature = "jpeg-encode-jpegli")]
        let _ = EncodeOptions::jpeg_jpegli();
        let _ = EncodeOptions::webp_lossy();
        let _ = EncodeOptions::webp_lossless();
        #[cfg(feature = "avif-encode")]
        let _ = EncodeOptions::avif();
        #[cfg(feature = "jxl-encode")]
        let _ = EncodeOptions::jxl();
        #[cfg(feature = "jxl-encode-libjxl")]
        let _ = EncodeOptions::jxl_libjxl();
        #[cfg(feature = "dng-encode")]
        let _ = EncodeOptions::dng();
    }

    #[test]
    fn test_jpeg_config_defaults() {
        let cfg = JpegEncEncodeConfig::default();
        assert_eq!(cfg.quality, 90, "JPEG default quality should be 90");
        assert!(
            cfg.common.metadata.embed_exif,
            "JPEG embeds EXIF by default"
        );
        assert!(cfg.common.metadata.embed_icc, "JPEG embeds ICC by default");
    }

    #[test]
    fn test_webp_config_defaults() {
        let cfg = LibwebpEncodeConfig::default();
        assert_eq!(cfg.mode, WebPMode::Lossy, "WebP default mode is Lossy");
        assert!((cfg.quality - 75.0).abs() < f32::EPSILON);
        assert_eq!(cfg.method, 4);
        assert_eq!(cfg.near_lossless, 100);
    }

    #[test]
    fn test_webp_named_constructors() {
        assert_eq!(LibwebpEncodeConfig::lossy().mode, WebPMode::Lossy);
        assert_eq!(LibwebpEncodeConfig::lossless().mode, WebPMode::Lossless);
    }

    #[test]
    fn test_format_and_codec_id() {
        assert_eq!(EncodeOptions::png().format(), OutputFormat::Png);
        assert_eq!(EncodeOptions::jpeg().codec_id().id, "jpeg/jpeg-encoder");
    }
}

// ============================================================================
// In-memory encode + decode-side API tests (the overhaul)
// ============================================================================

mod in_memory_tests {
    use super::*;
    use rawshift_image::formats::{
        StandardFormat, available_decoders, available_encoders, decode_standard_image,
        probe_standard_image,
    };

    #[test]
    fn encode_to_vec_matches_path_for_png() {
        let img = synthetic_image();
        let meta = ImageMetadata::default();
        let path = temp_path("vec_vs_path.png");

        let vec_bytes =
            encode_rgb_image_to_vec(&img, &meta, &EncodeOptions::png()).expect("encode to vec");
        encode_rgb_image(&img, &meta, &path, &EncodeOptions::png()).expect("encode to path");
        let path_bytes = fs::read(&path).expect("read back");

        assert_eq!(
            vec_bytes, path_bytes,
            "vec and path output must be identical"
        );
        fs::remove_file(&path).ok();
    }

    #[cfg(feature = "avif-encode")]
    #[test]
    fn avif_encodes_in_memory_without_a_path() {
        // The headline fix: AVIF no longer needs a file path for metadata.
        let img = synthetic_image();
        let bytes =
            encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &EncodeOptions::avif())
                .expect("AVIF to vec");
        assert!(!bytes.is_empty());
        assert_eq!(
            rawshift_image::formats::detect_standard_format(&bytes),
            Some(StandardFormat::Avif)
        );

        // ...and writing to a generic writer works too (previously Unsupported).
        let mut sink = Vec::new();
        rawshift_image::formats::encode_rgb_image_to_writer(
            &img,
            &ImageMetadata::default(),
            &mut sink,
            &EncodeOptions::avif(),
        )
        .expect("AVIF to writer");
        assert_eq!(sink, bytes, "writer output must match vec output");
    }

    #[cfg(feature = "jxl-encode")]
    #[test]
    fn jxl_encodes_in_memory_without_a_path() {
        let img = synthetic_image();
        let bytes = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &EncodeOptions::jxl())
            .expect("JXL to vec");
        assert!(!bytes.is_empty());

        let mut sink = Vec::new();
        rawshift_image::formats::encode_rgb_image_to_writer(
            &img,
            &ImageMetadata::default(),
            &mut sink,
            &EncodeOptions::jxl(),
        )
        .expect("JXL to writer");
        assert_eq!(sink, bytes);
    }

    #[test]
    fn registry_lists_compiled_codecs() {
        let encoders = available_encoders();
        assert!(!encoders.is_empty(), "default build has encoders");
        assert!(encoders.iter().all(|c| c.id.id.contains('/')));

        let decoders = available_decoders();
        assert!(!decoders.is_empty(), "default build has decoders");
    }

    #[test]
    fn decoded_png_is_tagged_srgb() {
        use rawshift_image::core::ColorSpace;
        let bytes = encode_rgb_image_to_vec(
            &synthetic_image(),
            &ImageMetadata::default(),
            &EncodeOptions::png(),
        )
        .expect("encode PNG");
        let decoded = decode_standard_image(&bytes, StandardFormat::Png).expect("decode PNG");
        assert_eq!(decoded.color_space(), ColorSpace::Srgb);
    }

    #[test]
    fn probe_reports_dimensions_without_decoding() {
        let bytes = encode_rgb_image_to_vec(
            &synthetic_image(),
            &ImageMetadata::default(),
            &EncodeOptions::png(),
        )
        .expect("encode PNG");
        let probe = probe_standard_image(&bytes).expect("probe");
        assert_eq!(probe.format, StandardFormat::Png);
        assert_eq!(probe.size.width, 4);
        assert_eq!(probe.size.height, 4);
    }

    #[test]
    fn entry_point_types_are_send_and_sync() {
        // The decode/encode entry points are stateless free functions; the
        // values that cross thread boundaries on a rayon worker pool must be
        // `Send + Sync`. This is a compile-time assertion.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RgbImage>();
        assert_send_sync::<ImageMetadata>();
        assert_send_sync::<EncodeOptions>();
        assert_send_sync::<rawshift_image::formats::DecodeOptions>();
        assert_send_sync::<rawshift_image::formats::ImageProbe>();
        assert_send_sync::<rawshift_image::error::RawError>();
        assert_send_sync::<Vec<u8>>();
    }
}
