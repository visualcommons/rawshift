#![cfg(feature = "experimental")]
//! Integration tests for RAW format decoding from on-disk fixture files.
//!
//! These tests load real RAW image files from `test_data/<Make>/<Model>/` and verify
//! format detection, metadata extraction, and raw decoding. Tests skip gracefully
//! when fixture files are not present.
//!
//! See TEST_FIXTURES.md for how to source test data files.

mod common;

use common::{load_ground_truth, test_data_path, test_fixture_path};
use rawshift_image::formats::RawFile;
use std::fs::File;
use std::io::BufReader;

/// Helper to skip test if data file is missing.
fn skip_if_no_test_data(filename: &str) -> bool {
    let path = test_data_path(filename);
    if !path.exists() {
        eprintln!("Skipping test: test data file not found: {:?}", path);
        return true;
    }
    false
}

// ============================================================================
// Sony ARW Tests
// ============================================================================

#[test]
fn arw_format_detection() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let raw = RawFile::open(reader).unwrap();
    assert!(matches!(raw, RawFile::Arw(_)), "Should detect as ARW");
}

#[test]
fn arw_metadata_extraction() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }
    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let raw = RawFile::open(reader).unwrap();
    let meta = raw.metadata();

    if let Some(ref camera_info) = gt.camera_info {
        assert!(
            meta.camera.make.contains(&camera_info.make),
            "Make should contain '{}', got '{}'",
            camera_info.make,
            meta.camera.make
        );
        assert!(
            meta.camera.model.contains(&camera_info.model)
                || camera_info.model.contains(&meta.camera.model),
            "Model should match '{}', got '{}'",
            camera_info.model,
            meta.camera.model
        );
    }
}

#[test]
fn arw_decode_raw_dimensions() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }
    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let mut raw = RawFile::open(reader).unwrap();

    if let RawFile::Arw(ref mut arw) = raw {
        let raw_image = arw.decode_raw().unwrap();
        assert_eq!(
            raw_image.size().width,
            gt.primary_raw_frame.width,
            "ARW raw width mismatch"
        );
        assert_eq!(
            raw_image.size().height,
            gt.primary_raw_frame.height,
            "ARW raw height mismatch"
        );
    } else {
        panic!("Expected ARW format");
    }
}

// ============================================================================
// Apple DNG (ProRAW) Tests
// ============================================================================

#[test]
fn dng_format_detection() {
    if skip_if_no_test_data("Apple/iPhone_17_Pro_Max/IMG_1347.DNG") {
        return;
    }
    let file = File::open(test_data_path("Apple/iPhone_17_Pro_Max/IMG_1347.DNG")).unwrap();
    let reader = BufReader::new(file);
    let raw = RawFile::open(reader).unwrap();
    assert!(matches!(raw, RawFile::Dng(_)), "Should detect as DNG");
}

#[test]
fn dng_metadata_extraction() {
    if skip_if_no_test_data("Apple/iPhone_17_Pro_Max/IMG_1347.DNG") {
        return;
    }
    let gt = load_ground_truth(&test_fixture_path(
        "Apple/iPhone_17_Pro_Max/IMG_1347/expected.json",
    ))
    .unwrap();
    let file = File::open(test_data_path("Apple/iPhone_17_Pro_Max/IMG_1347.DNG")).unwrap();
    let reader = BufReader::new(file);
    let raw = RawFile::open(reader).unwrap();
    let meta = raw.metadata();

    if let Some(ref camera_info) = gt.camera_info {
        assert!(
            meta.camera.make.contains(&camera_info.make),
            "Make should contain '{}', got '{}'",
            camera_info.make,
            meta.camera.make
        );
        assert!(
            meta.camera.model.contains(&camera_info.model)
                || camera_info.model.contains(&meta.camera.model),
            "Model should match '{}', got '{}'",
            camera_info.model,
            meta.camera.model
        );
    }
}

// ============================================================================
// Canon CR2 Tests (requires sourced fixture)
// ============================================================================

#[test]
fn cr2_format_detection() {
    // CR2 fixture: place a Canon CR2 file at test_data/Canon/<Model>/<file>.CR2
    // and run scripts/organize_test_data.py to set up fixtures.
    //
    // Look for any CR2 file in test_data/
    let cr2_path = find_test_file_by_extension("CR2");
    let path = match cr2_path {
        Some(p) => p,
        None => {
            eprintln!("Skipping CR2 test: no .CR2 file found in test_data/");
            return;
        }
    };
    let file = File::open(&path).unwrap();
    let reader = BufReader::new(file);
    let raw = RawFile::open(reader).unwrap();
    assert!(matches!(raw, RawFile::Cr2(_)), "Should detect as CR2");
}

// ============================================================================
// Canon CR3 Tests (requires sourced fixture)
// ============================================================================

#[test]
fn cr3_format_detection() {
    let cr3_path = find_test_file_by_extension("CR3");
    let path = match cr3_path {
        Some(p) => p,
        None => {
            eprintln!("Skipping CR3 test: no .CR3 file found in test_data/");
            return;
        }
    };
    let file = File::open(&path).unwrap();
    let reader = BufReader::new(file);
    let raw = RawFile::open(reader).unwrap();
    assert!(matches!(raw, RawFile::Cr3(_)), "Should detect as CR3");
}

// ============================================================================
// Canon CRW Tests (requires sourced fixture)
// ============================================================================

#[test]
fn crw_format_detection() {
    let crw_path = find_test_file_by_extension("CRW");
    let path = match crw_path {
        Some(p) => p,
        None => {
            eprintln!("Skipping CRW test: no .CRW file found in test_data/");
            return;
        }
    };
    let file = File::open(&path).unwrap();
    let reader = BufReader::new(file);
    let raw = RawFile::open(reader).unwrap();
    assert!(matches!(raw, RawFile::Crw(_)), "Should detect as CRW");
}

// ============================================================================
// Nikon NEF Tests (requires sourced fixture)
// ============================================================================

#[test]
fn nef_format_detection() {
    let nef_path = find_test_file_by_extension("NEF");
    let path = match nef_path {
        Some(p) => p,
        None => {
            eprintln!("Skipping NEF test: no .NEF file found in test_data/");
            return;
        }
    };
    let file = File::open(&path).unwrap();
    let reader = BufReader::new(file);
    let raw = RawFile::open(reader).unwrap();
    assert!(matches!(raw, RawFile::Nef(_)), "Should detect as NEF");
}

// ============================================================================
// Fujifilm RAF Tests (requires sourced fixture)
// ============================================================================

#[test]
fn raf_format_detection() {
    let raf_path = find_test_file_by_extension("RAF");
    let path = match raf_path {
        Some(p) => p,
        None => {
            eprintln!("Skipping RAF test: no .RAF file found in test_data/");
            return;
        }
    };
    let file = File::open(&path).unwrap();
    let reader = BufReader::new(file);
    let raw = RawFile::open(reader).unwrap();
    assert!(matches!(raw, RawFile::Raf(_)), "Should detect as RAF");
}

// ============================================================================
// Helpers
// ============================================================================

/// Search recursively under test_data/ for any file with the given extension.
fn find_test_file_by_extension(ext: &str) -> Option<std::path::PathBuf> {
    let test_data = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_data");
    if !test_data.exists() {
        return None;
    }
    find_recursive(&test_data, ext)
}

fn find_recursive(dir: &std::path::Path, ext: &str) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_recursive(&path, ext) {
                return Some(found);
            }
        } else if let Some(file_ext) = path.extension()
            && file_ext.eq_ignore_ascii_case(ext)
        {
            return Some(path);
        }
    }
    None
}
