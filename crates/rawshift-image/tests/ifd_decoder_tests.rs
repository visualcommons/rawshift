//! Decoder-level ground-truth tests over the gamut-ifd-backed ARW path.
//!
//! These port the behaviours of `tiff_parser_tests.rs` that the migrated
//! decoders rely on (raw-frame discovery in the SubIFD tree, dimension /
//! bit-depth / Make / Model extraction, strip bounds) to the decoder surface.
//! Parser-internal assertions (header fields, IFD-chain length) stay with the
//! legacy binrw tests and are removed with them in the DNG migration (#21).

mod common;

use common::{load_ground_truth, test_data_path, test_fixture_path};
use rawshift_image::formats::RawFile;
use std::fs::File;
use std::io::BufReader;

/// Helper to check if test data exists (skip test gracefully if not).
fn skip_if_no_test_data(filename: &str) -> bool {
    let path = test_data_path(filename);
    if !path.exists() {
        eprintln!("Skipping test: test data file not found: {:?}", path);
        return true;
    }
    false
}

fn open_arw(filename: &str) -> RawFile<BufReader<File>> {
    let file = File::open(test_data_path(filename)).unwrap();
    RawFile::open(BufReader::new(file)).unwrap()
}

#[test]
fn test_arw_raw_frame_dimensions_jic7790() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let raw = open_arw("SONY/ILCE-6700/_JIC7790.ARW");
    let RawFile::Arw(arw) = &raw else {
        panic!("expected ARW");
    };
    let metadata = arw.metadata().expect("metadata extracted");

    // The decoder must find the raw SubIFD (CFA photometric interpretation)
    // and read its dimensions and bit depth.
    assert_eq!(metadata.sensor_size.width, gt.primary_raw_frame.width);
    assert_eq!(metadata.sensor_size.height, gt.primary_raw_frame.height);
    assert_eq!(metadata.bit_depth, gt.primary_raw_frame.bit_depth);
}

#[test]
fn test_arw_raw_frame_dimensions_jic7792() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7792.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7792/expected.json")).unwrap();
    let raw = open_arw("SONY/ILCE-6700/_JIC7792.ARW");
    let RawFile::Arw(arw) = &raw else {
        panic!("expected ARW");
    };
    let metadata = arw.metadata().expect("metadata extracted");

    assert_eq!(metadata.sensor_size.width, gt.primary_raw_frame.width);
    assert_eq!(metadata.sensor_size.height, gt.primary_raw_frame.height);
}

#[test]
fn test_arw_make_model_extraction() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let raw = open_arw("SONY/ILCE-6700/_JIC7790.ARW");
    let RawFile::Arw(arw) = &raw else {
        panic!("expected ARW");
    };
    let metadata = arw.metadata().expect("metadata extracted");

    if let Some(ref camera_info) = gt.camera_info {
        assert!(
            metadata.make.contains(&camera_info.make),
            "Make mismatch: expected to contain '{}', got '{}'",
            camera_info.make,
            metadata.make
        );
        assert!(
            metadata.model.contains(&camera_info.model)
                || camera_info.model.contains(&metadata.model),
            "Model mismatch: expected to contain '{}', got '{}'",
            camera_info.model,
            metadata.model
        );
    }
}

#[test]
fn test_arw_strip_data_within_bounds_jic7790() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let mut raw = open_arw("SONY/ILCE-6700/_JIC7790.ARW");
    let RawFile::Arw(ref mut arw) = raw else {
        panic!("expected ARW");
    };
    let metadata = arw.metadata().expect("metadata extracted").clone();

    // The strip range the decoder extracted must lie inside the file.
    assert!(metadata.raw_data_offset > 0, "no raw data offset extracted");
    assert!(metadata.raw_data_size > 0, "no raw data size extracted");
    let end = metadata.raw_data_offset + metadata.raw_data_size;
    assert!(
        end <= gt.file_size,
        "Strip data exceeds file size: offset {} + size {} = {} > file size {}",
        metadata.raw_data_offset,
        metadata.raw_data_size,
        end,
        gt.file_size
    );

    // ...and be readable through the decoder's bounds-checked slice path.
    let data = arw.read_raw_data().expect("raw data readable");
    assert_eq!(data.len() as u64, metadata.raw_data_size);
}
