//! Integration tests for image export with metadata embedding.
//!
//! Tests verify that EXIF/ICC/XMP metadata is correctly embedded in exported
//! images, that the in-memory encode entry points work for every format, and
//! that the decode-side color/probe APIs behave as documented. They use a tiny
//! synthetic RGB image to avoid the expensive full RAW decode pipeline.

use rawshift_image::core::RgbImage;
use rawshift_image::core::metadata::ImageMetadata;
use rawshift_image::formats::export::{
    BitDepth, CommonEncodeOptions, EncodeOptions, JpegEncodeConfig, LibwebpEncodeConfig,
    MetadataEmbedOptions, PngEncodeConfig, WebPMode,
};
use rawshift_image::formats::{encode_rgb_image, encode_rgb_image_to_vec};
use std::fs;
use std::path::PathBuf;

/// 4×4 grey synthetic RGB image (16-bit, tone-mapped already).
fn synthetic_image() -> RgbImage {
    RgbImage::new(4, 4, vec![32768u16; 4 * 4 * 3]).expect("valid RGB buffer")
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
        EncodeOptions::Jpeg(JpegEncodeConfig {
            quality,
            common: common(exif, icc, true),
            ..JpegEncodeConfig::default()
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
        let img = RgbImage::new(64, 64, data).expect("valid RGB buffer");

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

    /// EXIF, ICC, and XMP written by the gamut-jpeg encoder must all read
    /// back through `read_standard_image_metadata` (which extracts them via
    /// `gamut_jpeg::metadata`).
    #[test]
    fn test_jpeg_metadata_roundtrip() {
        use rawshift_image::core::metadata::{CameraInfo, ExifInfo};
        use rawshift_image::formats::{StandardFormat, read_standard_image_metadata};

        let img = synthetic_image();
        let xmp_packet: &[u8] = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
            <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"/>\
            </x:xmpmeta>";
        let md = ImageMetadata {
            camera: CameraInfo {
                make: "TestMake".to_string(),
                model: "TestModel".to_string(),
                ..Default::default()
            },
            exif: ExifInfo {
                iso: Some(400),
                ..Default::default()
            },
            xmp: Some(xmp_packet.to_vec()),
            ..Default::default()
        };

        let bytes = encode_rgb_image_to_vec(&img, &md, &jpeg(90, true, true))
            .expect("encode JPEG with metadata");

        let read_md = read_standard_image_metadata(&bytes, StandardFormat::Jpeg);
        assert_eq!(read_md.camera.make, "TestMake", "make round-trip");
        assert_eq!(read_md.camera.model, "TestModel", "model round-trip");
        assert_eq!(read_md.exif.iso, Some(400), "ISO round-trip");
        assert_eq!(
            read_md.xmp.as_deref(),
            Some(xmp_packet),
            "XMP packet round-trip"
        );
        let icc = read_md.icc_profile.expect("ICC profile round-trip");
        assert_eq!(&icc[36..40], b"acsp", "ICC payload must be a profile");
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

        let opts = EncodeOptions::WebP(webp(true, false, false));
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

        let opts = EncodeOptions::WebP(webp(false, false, false));
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

        let opts = EncodeOptions::WebP(webp(false, true, false));
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

        let opts = EncodeOptions::WebP(webp(false, false, true));
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
        let img = RgbImage::new(64, 64, data).expect("valid RGB buffer");

        let mut low = webp(false, false, false);
        low.quality = 10.0;
        let mut high = webp(false, false, false);
        high.quality = 95.0;

        let low =
            encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &EncodeOptions::WebP(low))
                .expect("Export low quality");
        let high =
            encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &EncodeOptions::WebP(high))
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

        let opts = EncodeOptions::Png(PngEncodeConfig {
            common: CommonEncodeOptions {
                bit_depth: BitDepth::Eight,
                ..CommonEncodeOptions::default()
            },
            ..PngEncodeConfig::default()
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

    /// A chunk type appears in the byte stream (chunk names are unique enough
    /// in a tiny synthetic image for a windowed search).
    fn png_has_chunk(data: &[u8], name: &[u8; 4]) -> bool {
        data.windows(4).any(|w| w == name)
    }

    #[test]
    fn test_png_export_metadata_roundtrip() {
        use rawshift_image::formats::{StandardFormat, decode_standard_image};

        let img = synthetic_image();
        let mut metadata = ImageMetadata::default();
        let xmp = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"></x:xmpmeta>";
        metadata.xmp = Some(xmp.as_bytes().to_vec());

        let opts = EncodeOptions::Png(PngEncodeConfig {
            common: common(true, true, true),
            ..PngEncodeConfig::default()
        });
        let data = encode_rgb_image_to_vec(&img, &metadata, &opts).expect("Export PNG");

        assert_eq!(&data[0..8], &PNG_SIGNATURE);
        assert!(
            png_has_chunk(&data, b"eXIf"),
            "PNG should carry an eXIf chunk"
        );
        assert!(
            png_has_chunk(&data, b"iCCP"),
            "PNG should carry an iCCP chunk"
        );
        assert!(
            png_has_chunk(&data, b"iTXt"),
            "PNG should carry the XMP iTXt chunk"
        );
        assert!(
            data.windows(17).any(|w| w == b"XML:com.adobe.xmp"),
            "the iTXt chunk should use the standard XMP keyword"
        );

        // The metadata-bearing file must still decode losslessly.
        let decoded = decode_standard_image(&data, StandardFormat::Png).expect("decode PNG");
        assert_eq!(decoded.width(), img.width());
        assert_eq!(decoded.height(), img.height());
        assert_eq!(decoded.data(), img.data(), "pixel round-trip must be exact");
    }
}

/// Check if AVIF data contains a `colr rICC`/`colr prof` box (embedded ICC).
#[cfg(feature = "avif-encode")]
fn avif_has_icc(data: &[u8]) -> bool {
    data.windows(8)
        .any(|w| &w[..4] == b"colr" && (&w[4..8] == b"rICC" || &w[4..8] == b"prof"))
}

/// Check if JXL data carries an embedded ICC profile.
///
/// gamut-jxl embeds the ICC profile in the codestream's colour metadata (not a
/// container box), so the check reads the stream headers back through the
/// decoder rather than scanning for a box.
#[cfg(feature = "jxl-encode")]
fn jxl_has_icc(data: &[u8]) -> bool {
    matches!(
        gamut_jxl::JxlDecoder::new().embedded_icc_profile(data),
        Ok(Some(_))
    )
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
    use rawshift_image::formats::export::AvifEncodeConfig;

    fn avif(exif: bool, icc: bool) -> EncodeOptions {
        EncodeOptions::Avif(AvifEncodeConfig {
            common: common(exif, icc, true),
            ..AvifEncodeConfig::default()
        })
    }

    /// The bytes are a valid ISO-BMFF AVIF file (`ftyp` box, AVIF brand).
    fn is_avif(data: &[u8]) -> bool {
        data.len() > 12
            && &data[4..8] == b"ftyp"
            && matches!(&data[8..12], b"avif" | b"avis" | b"mif1")
    }

    #[test]
    fn encodes_lossless_and_lossy() {
        let img = synthetic_image();
        for lossless in [true, false] {
            let opts = EncodeOptions::Avif(AvifEncodeConfig {
                common: common(false, false, false),
                lossless,
                quality: 60,
            });
            let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts)
                .unwrap_or_else(|e| panic!("AVIF encode (lossless={lossless}) failed: {e}"));
            assert!(
                is_avif(&data),
                "lossless={lossless} output must be a valid AVIF"
            );
        }
    }

    /// 10/12-bit AVIF output is temporarily unavailable — gamut-avif is 8-bit
    /// only until justin13888/gamut#251 lands — and must be reported, not
    /// silently degraded.
    #[test]
    fn rejects_10_and_12_bit() {
        let img = synthetic_image();
        for depth in [BitDepth::Ten, BitDepth::Twelve] {
            let opts = EncodeOptions::Avif(AvifEncodeConfig {
                common: CommonEncodeOptions {
                    bit_depth: depth,
                    ..common(false, false, false)
                },
                ..AvifEncodeConfig::default()
            });
            let err = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts)
                .expect_err("10/12-bit AVIF encode must be rejected");
            let msg = err.to_string();
            assert!(
                msg.contains("justin13888/gamut#251"),
                "error must reference the upstream issue, got: {msg}"
            );
        }
    }

    /// Strongest in-scope proof the AV1 bitstream + container are valid:
    /// parse it back through gamut-avif (backend-less container + metadata
    /// surface, which validates the primary item and its av1C), then attempt
    /// pixel decode. rawshift's lossless output is identity 4:4:4 — AV1
    /// **Profile 1** — while today's hardware backends decode Profile 0 only
    /// (docs/SUPPORT.md), so the pixel step is asserted adaptively: real
    /// pixels where the machine's decoder covers Profile 1, and the honest,
    /// matchable scope error (or `HwDecoderUnavailable` with no decoder at
    /// all) elsewhere. The dedicated hardware end-to-end suite lives in
    /// `tests/avif_hw_decode.rs`.
    #[cfg(feature = "avif-decode")]
    #[test]
    fn round_trips_through_decoder() {
        use rawshift_image::core::{MetadataNamespace, MetadataValue};
        use rawshift_image::error::RawError;
        use rawshift_image::formats::{AvifFile, StandardFormat, decode_standard_image};
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &avif(false, false))
            .expect("AVIF encode");

        let file = AvifFile::open(data.clone()).expect("parse back rawshift's own AVIF");
        let md = file.metadata();
        assert_eq!(
            md.get(MetadataNamespace::Avif, "width"),
            Some(&MetadataValue::U64(4))
        );
        assert_eq!(
            md.get(MetadataNamespace::Avif, "height"),
            Some(&MetadataValue::U64(4))
        );
        assert_eq!(md.image.bit_depth, 8);

        match decode_standard_image(&data, StandardFormat::Avif) {
            Ok(decoded) => assert_eq!((decoded.width(), decoded.height()), (4, 4)),
            Err(RawError::HwDecoderUnavailable { codec: "AV1", .. }) => {}
            Err(err @ RawError::Format(_)) => {
                let msg = err.to_string();
                assert!(
                    msg.to_lowercase().contains("profile"),
                    "a hardware decoder rejecting rawshift's Profile 1 output must \
                     name its profile scope, got: {msg}"
                );
            }
            Err(other) => panic!("unexpected AVIF decode error: {other}"),
        }
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
    #[cfg(feature = "jxl-decode")]
    use rawshift_image::formats::decode_standard_image;
    use rawshift_image::formats::export::JxlEncodeConfig;
    use rawshift_image::formats::{StandardFormat, available_encoders};

    fn jxl(exif: bool, icc: bool) -> EncodeOptions {
        EncodeOptions::Jxl(JxlEncodeConfig {
            common: common(exif, icc, true),
            ..JxlEncodeConfig::default()
        })
    }

    /// 16-bit synthetic image with distinct per-sample values (so a lossless
    /// round-trip is a meaningful check). 48 samples, all within `u16`.
    #[cfg(feature = "jxl-decode")]
    fn distinct_16bit() -> RgbImage {
        let data: Vec<u16> = (0..4 * 4 * 3)
            .map(|i| ((i as u32 * 4099) % 65536) as u16)
            .collect();
        RgbImage::new(4, 4, data).expect("valid RGB buffer")
    }

    #[test]
    fn test_jxl_config_default_has_embed_icc() {
        assert!(
            JxlEncodeConfig::default().common.metadata.embed_icc,
            "JxlEncodeConfig default should embed ICC"
        );
        assert!(
            JxlEncodeConfig::default().lossless,
            "JxlEncodeConfig default should be lossless"
        );
    }

    #[test]
    fn jxl_registers_as_encoder() {
        assert!(
            available_encoders().iter().any(|c| c.id.id == "jxl/gamut"),
            "jxl/gamut should be listed when the feature is enabled"
        );
    }

    #[test]
    fn test_jxl_export_with_icc_enabled() {
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &jxl(false, true))
            .expect("Export JXL");
        // The profile lives in the codestream's colour metadata, so no
        // container framing is needed for ICC alone.
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

    #[test]
    fn test_jxl_export_with_exif_uses_container() {
        // EXIF becomes an `Exif` container box, which forces ISO BMFF framing.
        let img = synthetic_image();
        let data = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &jxl(true, false))
            .expect("Export JXL");
        assert!(
            jxl_is_container(&data),
            "JXL with EXIF should be container format"
        );
    }

    #[cfg(feature = "jxl-decode")]
    #[test]
    fn jxl_encodes_and_decodes_roundtrip() {
        let img = synthetic_image();
        let bytes = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &jxl(false, false))
            .expect("encode JXL");
        assert!(!bytes.is_empty());
        assert_eq!(
            rawshift_image::formats::detect_standard_format(&bytes),
            Some(StandardFormat::Jxl),
            "output should be detected as JXL"
        );
        let decoded = decode_standard_image(&bytes, StandardFormat::Jxl).expect("decode JXL");
        assert_eq!(decoded.width(), 4);
        assert_eq!(decoded.height(), 4);
    }

    #[cfg(feature = "jxl-decode")]
    #[test]
    fn jxl_lossless_16bit_is_exact() {
        let img = distinct_16bit();
        let want = img.data().to_vec();
        // The default configuration is lossless 16-bit.
        let bytes = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &jxl(false, false))
            .expect("encode lossless JXL");
        let decoded = decode_standard_image(&bytes, StandardFormat::Jxl).expect("decode JXL");
        assert_eq!(
            decoded.data(),
            want,
            "lossless 16-bit JXL round-trip must be exact"
        );
    }

    #[test]
    fn jxl_lossy_toggles_encode() {
        let img = synthetic_image();
        let opts = EncodeOptions::Jxl(JxlEncodeConfig {
            common: common(false, false, false),
            lossless: false,
            distance: 3.0,
            effort: 4,
            ..JxlEncodeConfig::default()
        });
        let bytes = encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &opts)
            .expect("encode lossy JXL");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn jxl_rejects_invalid_effort_and_distance() {
        let img = synthetic_image();
        let bad_effort = EncodeOptions::Jxl(JxlEncodeConfig {
            common: common(false, false, false),
            effort: 11,
            ..JxlEncodeConfig::default()
        });
        assert!(
            encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &bad_effort).is_err(),
            "effort 11 must be rejected"
        );
        let bad_distance = EncodeOptions::Jxl(JxlEncodeConfig {
            common: common(false, false, false),
            lossless: false,
            distance: 0.0,
            ..JxlEncodeConfig::default()
        });
        assert!(
            encode_rgb_image_to_vec(&img, &ImageMetadata::default(), &bad_distance).is_err(),
            "lossy distance 0.0 must be rejected (lossless is a mode, not a distance)"
        );
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
    fn test_jpeg_config_defaults() {
        let cfg = JpegEncodeConfig::default();
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
        assert_eq!(EncodeOptions::jpeg().codec_id().id, "jpeg/gamut");
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
        use rawshift_image::core::ColorDescription;
        let bytes = encode_rgb_image_to_vec(
            &synthetic_image(),
            &ImageMetadata::default(),
            &EncodeOptions::png(),
        )
        .expect("encode PNG");
        let decoded = decode_standard_image(&bytes, StandardFormat::Png).expect("decode PNG");
        assert_eq!(decoded.color(), ColorDescription::SRGB);
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
