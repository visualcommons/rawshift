//! Integration tests for standard format decoding from on-disk fixture files.
//!
//! These tests load real image files from `test_data/standard/<format>/` and verify
//! decoding produces correct dimensions and pixel data. Tests skip gracefully when
//! fixture files are not present.
//!
//! Generate fixtures with:
//!   cargo run --example generate_test_fixtures

use rawshift_image::formats::{
    StandardFormat, decode_standard_image, detect_standard_format, read_standard_image_metadata,
};
use serde::Deserialize;
use std::path::PathBuf;

/// Ground truth for a standard format fixture.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct StandardGroundTruth {
    format: String,
    file_name: String,
    width: u32,
    height: u32,
    channels: u32,
    bit_depth_output: u32,
    #[serde(default)]
    metadata: Option<MetadataGroundTruth>,
}

/// Expected metadata values for formats that embed EXIF/ICC/XMP.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MetadataGroundTruth {
    make: Option<String>,
    model: Option<String>,
    iso: Option<u32>,
    focal_length_num: Option<u32>,
    datetime_original: Option<String>,
    has_icc: Option<bool>,
    has_xmp: Option<bool>,
}

fn test_data_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_data")
        .join("standard")
        .join(rel)
}

fn test_fixture_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_fixtures")
        .join("standard")
        .join(rel)
}

fn load_standard_ground_truth(format_dir: &str) -> Option<StandardGroundTruth> {
    let path = test_fixture_path(&format!("{}/expected.json", format_dir));
    let contents = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Skip test if fixture file doesn't exist.
macro_rules! skip_if_missing {
    ($path:expr) => {
        if !$path.exists() {
            eprintln!(
                "Skipping test: fixture not found: {:?}\n  Run: cargo run --example generate_test_fixtures",
                $path
            );
            return;
        }
    };
}

// ============================================================================
// Format Detection from File
// ============================================================================

#[test]
fn detect_jpeg_from_file() {
    let gt = match load_standard_ground_truth("jpeg") {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("jpeg/{}", gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let fmt = detect_standard_format(&data);
    assert_eq!(fmt, Some(StandardFormat::Jpeg), "JPEG detection from file");
}

#[test]
fn detect_png_from_file() {
    let gt = match load_standard_ground_truth("png") {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("png/{}", gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let fmt = detect_standard_format(&data);
    assert_eq!(fmt, Some(StandardFormat::Png), "PNG detection from file");
}

#[test]
fn detect_gif_from_file() {
    let gt = match load_standard_ground_truth("gif") {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("gif/{}", gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let fmt = detect_standard_format(&data);
    assert_eq!(fmt, Some(StandardFormat::Gif), "GIF detection from file");
}

#[test]
fn detect_tiff_from_file() {
    let gt = match load_standard_ground_truth("tiff") {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("tiff/{}", gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let fmt = detect_standard_format(&data);
    assert_eq!(fmt, Some(StandardFormat::Tiff), "TIFF detection from file");
}

#[test]
fn detect_webp_from_file() {
    let gt = match load_standard_ground_truth("webp") {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("webp/{}", gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let fmt = detect_standard_format(&data);
    assert_eq!(fmt, Some(StandardFormat::WebP), "WebP detection from file");
}

#[test]
fn detect_svg_from_file() {
    let gt = match load_standard_ground_truth("svg") {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("svg/{}", gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let fmt = detect_standard_format(&data);
    assert_eq!(fmt, Some(StandardFormat::Svg), "SVG detection from file");
}

#[cfg(feature = "heic")]
#[test]
fn detect_heic_from_file() {
    let gt = match load_standard_ground_truth("heic") {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("heic/{}", gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let fmt = detect_standard_format(&data);
    assert_eq!(fmt, Some(StandardFormat::Heic), "HEIC detection from file");
}

// ============================================================================
// Decode Dimensions from File
// ============================================================================

fn assert_decode_dimensions(format_dir: &str, expected_format: StandardFormat) {
    let gt = match load_standard_ground_truth(format_dir) {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("{}/{}", format_dir, gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let img = decode_standard_image(&data, expected_format)
        .unwrap_or_else(|e| panic!("{} decode failed: {}", gt.format, e));

    assert_eq!(
        img.width(),
        gt.width,
        "{} width mismatch: expected {}, got {}",
        gt.format,
        gt.width,
        img.width()
    );
    assert_eq!(
        img.height(),
        gt.height,
        "{} height mismatch: expected {}, got {}",
        gt.format,
        gt.height,
        img.height()
    );
    assert_eq!(
        img.data.len(),
        (gt.width * gt.height * gt.channels) as usize,
        "{} pixel data length mismatch",
        gt.format
    );
}

#[test]
fn decode_jpeg_dimensions_from_file() {
    assert_decode_dimensions("jpeg", StandardFormat::Jpeg);
}

#[test]
fn decode_png_dimensions_from_file() {
    assert_decode_dimensions("png", StandardFormat::Png);
}

#[test]
fn decode_gif_dimensions_from_file() {
    assert_decode_dimensions("gif", StandardFormat::Gif);
}

#[test]
fn decode_tiff_dimensions_from_file() {
    assert_decode_dimensions("tiff", StandardFormat::Tiff);
}

#[test]
fn decode_webp_dimensions_from_file() {
    assert_decode_dimensions("webp", StandardFormat::WebP);
}

#[cfg(feature = "svg")]
#[test]
fn decode_svg_dimensions_from_file() {
    assert_decode_dimensions("svg", StandardFormat::Svg);
}

#[cfg(feature = "avif")]
#[test]
fn decode_avif_dimensions_from_file() {
    assert_decode_dimensions("avif", StandardFormat::Avif);
}

#[cfg(feature = "heic")]
#[test]
fn decode_heic_dimensions_from_file() {
    assert_decode_dimensions("heic", StandardFormat::Heic);
}

// ============================================================================
// Full Detect + Decode Pipeline from File
// ============================================================================

fn assert_detect_then_decode(format_dir: &str, expected_format: StandardFormat) {
    let gt = match load_standard_ground_truth(format_dir) {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("{}/{}", format_dir, gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();

    let detected = detect_standard_format(&data);
    assert_eq!(
        detected,
        Some(expected_format),
        "{}: detection should match expected format",
        gt.format
    );

    let img = decode_standard_image(&data, detected.unwrap())
        .unwrap_or_else(|e| panic!("{} decode after detection failed: {}", gt.format, e));

    assert_eq!(img.width(), gt.width);
    assert_eq!(img.height(), gt.height);
}

#[test]
fn detect_then_decode_jpeg_from_file() {
    assert_detect_then_decode("jpeg", StandardFormat::Jpeg);
}

#[test]
fn detect_then_decode_png_from_file() {
    assert_detect_then_decode("png", StandardFormat::Png);
}

#[test]
fn detect_then_decode_gif_from_file() {
    assert_detect_then_decode("gif", StandardFormat::Gif);
}

#[test]
fn detect_then_decode_tiff_from_file() {
    assert_detect_then_decode("tiff", StandardFormat::Tiff);
}

#[test]
fn detect_then_decode_webp_from_file() {
    assert_detect_then_decode("webp", StandardFormat::WebP);
}

#[cfg(feature = "svg")]
#[test]
fn detect_then_decode_svg_from_file() {
    assert_detect_then_decode("svg", StandardFormat::Svg);
}

#[cfg(feature = "avif")]
#[test]
fn detect_then_decode_avif_from_file() {
    assert_detect_then_decode("avif", StandardFormat::Avif);
}

#[cfg(feature = "heic")]
#[test]
fn detect_then_decode_heic_from_file() {
    assert_detect_then_decode("heic", StandardFormat::Heic);
}

// ============================================================================
// Pixel Value Verification
// ============================================================================

#[test]
fn decode_png_pixel_values_from_file() {
    let gt = match load_standard_ground_truth("png") {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("png/{}", gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let img = decode_standard_image(&data, StandardFormat::Png).unwrap();

    // PNG is lossless, so first pixel (red: 255,0,0) should be exact after u8->u16 scaling.
    // u8 255 -> u16 65535 (255 * 257)
    assert_eq!(img.data[0], 65535, "PNG first pixel R should be 65535");
    assert_eq!(img.data[1], 0, "PNG first pixel G should be 0");
    assert_eq!(img.data[2], 0, "PNG first pixel B should be 0");
}

#[test]
fn decode_tiff_pixel_values_from_file() {
    let gt = match load_standard_ground_truth("tiff") {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("tiff/{}", gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let img = decode_standard_image(&data, StandardFormat::Tiff).unwrap();

    // TIFF is lossless, first pixel (red: 255,0,0) should be exact.
    assert_eq!(img.data[0], 65535, "TIFF first pixel R should be 65535");
    assert_eq!(img.data[1], 0, "TIFF first pixel G should be 0");
    assert_eq!(img.data[2], 0, "TIFF first pixel B should be 0");
}

#[test]
fn decode_gif_first_pixel_from_file() {
    let gt = match load_standard_ground_truth("gif") {
        Some(gt) => gt,
        None => return,
    };
    let path = test_data_path(&format!("gif/{}", gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let img = decode_standard_image(&data, StandardFormat::Gif).unwrap();

    // GIF palette index 0 = red (255, 0, 0) -> u16: (65535, 0, 0)
    assert_eq!(img.data[0], 65535, "GIF first pixel R should be 65535");
    assert_eq!(img.data[1], 0, "GIF first pixel G should be 0");
    assert_eq!(img.data[2], 0, "GIF first pixel B should be 0");
}

// ============================================================================
// Metadata Round-Trip Verification
// ============================================================================

fn assert_metadata_extraction(format_dir: &str, expected_format: StandardFormat) {
    let gt = match load_standard_ground_truth(format_dir) {
        Some(gt) => gt,
        None => return,
    };
    let meta_gt = match gt.metadata {
        Some(ref m) => m,
        None => {
            eprintln!(
                "Skipping metadata test for {}: no metadata in expected.json",
                gt.format
            );
            return;
        }
    };
    let path = test_data_path(&format!("{}/{}", format_dir, gt.file_name));
    skip_if_missing!(path);

    let data = std::fs::read(&path).unwrap();
    let md = read_standard_image_metadata(&data, expected_format);

    if let Some(ref make) = meta_gt.make {
        assert_eq!(
            &md.camera.make, make,
            "{} metadata: make mismatch",
            gt.format
        );
    }
    if let Some(ref model) = meta_gt.model {
        assert_eq!(
            &md.camera.model, model,
            "{} metadata: model mismatch",
            gt.format
        );
    }
    if let Some(expected_iso) = meta_gt.iso {
        assert_eq!(
            md.exif.iso,
            Some(expected_iso),
            "{} metadata: ISO mismatch",
            gt.format
        );
    }
    if let Some(expected_fl) = meta_gt.focal_length_num {
        let actual_fl = md.exif.focal_length.map(|r| r.numerator);
        assert_eq!(
            actual_fl,
            Some(expected_fl),
            "{} metadata: focal length mismatch",
            gt.format
        );
    }
    if let Some(ref expected_dt) = meta_gt.datetime_original {
        assert_eq!(
            md.datetime.datetime_original.as_deref(),
            Some(expected_dt.as_str()),
            "{} metadata: datetime_original mismatch",
            gt.format
        );
    }
}

#[test]
fn read_metadata_jpeg_from_file() {
    assert_metadata_extraction("jpeg", StandardFormat::Jpeg);
}

#[test]
fn read_metadata_png_from_file() {
    assert_metadata_extraction("png", StandardFormat::Png);
}

#[test]
fn read_metadata_webp_from_file() {
    assert_metadata_extraction("webp", StandardFormat::WebP);
}

#[cfg(feature = "avif")]
#[test]
fn read_metadata_avif_from_file() {
    assert_metadata_extraction("avif", StandardFormat::Avif);
}

#[cfg(feature = "heic")]
#[test]
fn read_metadata_heic_from_file() {
    assert_metadata_extraction("heic", StandardFormat::Heic);
}
