//! TIFF Parser integration tests using ground truth validation.
//!
//! These tests verify that the parser correctly extracts metadata
//! from real ARW files by comparing against known-good values.

mod common;

use common::{load_ground_truth, test_data_path, test_fixture_path};
use rawshift_image::tiff::{TiffParser, TiffTag};
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

// ============================================================================
// Phase 1: Header Parsing Tests (Fail Fast)
// ============================================================================

#[test]
fn test_header_byte_order_jic7790() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let parser = TiffParser::new(reader).unwrap();

    let header = parser.header();
    let actual_byte_order = header.byte_order.as_str();

    assert_eq!(
        actual_byte_order, gt.structure.byte_order,
        "Byte order mismatch: expected {}, got {}",
        gt.structure.byte_order, actual_byte_order
    );
}

#[test]
fn test_header_byte_order_jic7792() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7792.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7792/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7792.ARW")).unwrap();
    let reader = BufReader::new(file);
    let parser = TiffParser::new(reader).unwrap();

    let header = parser.header();
    assert_eq!(header.byte_order.as_str(), gt.structure.byte_order);
}

#[test]
fn test_is_not_bigtiff() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let parser = TiffParser::new(reader).unwrap();

    assert_eq!(parser.is_bigtiff(), gt.structure.is_big_tiff);
}

#[test]
fn test_magic_number() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let parser = TiffParser::new(reader).unwrap();

    // Standard TIFF magic is 42
    assert_eq!(parser.header().magic, 42);
}

// ============================================================================
// Phase 1: IFD Walking Tests
// ============================================================================

#[test]
fn test_ifd_count_jic7790() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let mut parser = TiffParser::new(reader).unwrap();

    let ifds = parser.walk_ifd_chain().unwrap();

    assert_eq!(
        ifds.len(),
        gt.structure.ifd_count,
        "IFD count mismatch: expected {}, got {}",
        gt.structure.ifd_count,
        ifds.len()
    );
}

#[test]
fn test_ifd_count_jic7792() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7792.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7792/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7792.ARW")).unwrap();
    let reader = BufReader::new(file);
    let mut parser = TiffParser::new(reader).unwrap();

    let ifds = parser.walk_ifd_chain().unwrap();
    assert_eq!(ifds.len(), gt.structure.ifd_count);
}

// ============================================================================
// Phase 2: Tag Presence Tests (Breadth First)
// ============================================================================

#[test]
fn test_required_tags_present() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let mut parser = TiffParser::new(reader).unwrap();

    let ifds = parser.walk_ifd_chain().unwrap();

    // Collect all tag IDs from all IFDs and SubIFDs
    let mut all_tags: Vec<u16> = Vec::new();

    fn collect_tags(ifd: &rawshift_image::tiff::Ifd, tags: &mut Vec<u16>) {
        tags.extend(ifd.all_tag_ids());
        for sub_ifd in &ifd.sub_ifds {
            collect_tags(sub_ifd, tags);
        }
    }

    for ifd in &ifds {
        collect_tags(ifd, &mut all_tags);
    }

    // Must find ImageWidth (0x0100) somewhere
    assert!(
        all_tags.contains(&0x0100),
        "Required tag ImageWidth (0x0100) not found in any IFD"
    );

    // Must find StripOffsets (0x0111) somewhere
    assert!(
        all_tags.contains(&0x0111),
        "Required tag StripOffsets (0x0111) not found in any IFD"
    );
}

#[test]
fn test_sub_ifd_parsing() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let mut parser = TiffParser::new(reader).unwrap();

    let ifd0 = parser.parse_ifd0().unwrap();

    // Sony ARW should have SubIFDs
    assert!(
        !ifd0.sub_ifds.is_empty() || ifd0.contains(TiffTag::SubIFDs),
        "IFD0 should have SubIFDs tag or parsed SubIFDs"
    );
}

// ============================================================================
// Phase 2: Value Extraction Tests (Depth Test)
// ============================================================================

#[test]
fn test_raw_dimensions_jic7790() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let mut parser = TiffParser::new(reader).unwrap();

    let ifd0 = parser.parse_ifd0().unwrap();

    // Find the SubIFD with the raw image (should have full resolution)
    let mut found = false;
    for sub_ifd in &ifd0.sub_ifds {
        if let Some(width_entry) = sub_ifd.get(TiffTag::ImageWidth) {
            let width_value = parser.read_value(width_entry).unwrap();
            if let Some(width) = width_value.as_u32()
                && width == gt.primary_raw_frame.width
            {
                found = true;

                // Also verify height
                if let Some(height_entry) = sub_ifd.get(TiffTag::ImageLength) {
                    let height_value = parser.read_value(height_entry).unwrap();
                    if let Some(height) = height_value.as_u32() {
                        assert_eq!(
                            height, gt.primary_raw_frame.height,
                            "Height mismatch: expected {}, got {}",
                            gt.primary_raw_frame.height, height
                        );
                    }
                }
                break;
            }
        }
    }

    assert!(
        found,
        "Could not find SubIFD with expected width {}",
        gt.primary_raw_frame.width
    );
}

#[test]
fn test_bit_depth_jic7790() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let mut parser = TiffParser::new(reader).unwrap();

    let ifd0 = parser.parse_ifd0().unwrap();

    // Find BitsPerSample in SubIFD
    for sub_ifd in &ifd0.sub_ifds {
        if let Some(entry) = sub_ifd.get(TiffTag::BitsPerSample) {
            let value = parser.read_value(entry).unwrap();
            if let Some(bits) = value.as_u32()
                && bits == gt.primary_raw_frame.bit_depth as u32
            {
                // Found matching bit depth
                return;
            }
        }
    }

    panic!(
        "Could not find BitsPerSample matching expected value {}",
        gt.primary_raw_frame.bit_depth
    );
}

// ============================================================================
// Phase 3: Boundary Tests (Safety)
// ============================================================================

#[test]
fn test_strip_offsets_within_bounds_jic7790() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let mut parser = TiffParser::new(reader).unwrap();

    let ifd0 = parser.parse_ifd0().unwrap();

    // Find StripOffsets and StripByteCounts in SubIFD
    for sub_ifd in &ifd0.sub_ifds {
        if let (Some(offset_entry), Some(count_entry)) = (
            sub_ifd.get(TiffTag::StripOffsets),
            sub_ifd.get(TiffTag::StripByteCounts),
        ) {
            let offsets = parser.read_value(offset_entry).unwrap();
            let counts = parser.read_value(count_entry).unwrap();

            if let (Some(offset_vec), Some(count_vec)) = (offsets.as_u32_vec(), counts.as_u32_vec())
            {
                for (offset, count) in offset_vec.iter().zip(count_vec.iter()) {
                    let end = *offset as u64 + *count as u64;
                    assert!(
                        end <= gt.file_size,
                        "Strip data exceeds file size: offset {} + count {} = {} > file size {}",
                        offset,
                        count,
                        end,
                        gt.file_size
                    );
                }
            }
        }
    }
}

#[test]
fn test_make_model_extraction() {
    if skip_if_no_test_data("SONY/ILCE-6700/_JIC7790.ARW") {
        return;
    }

    let gt =
        load_ground_truth(&test_fixture_path("SONY/ILCE-6700/_JIC7790/expected.json")).unwrap();
    let file = File::open(test_data_path("SONY/ILCE-6700/_JIC7790.ARW")).unwrap();
    let reader = BufReader::new(file);
    let mut parser = TiffParser::new(reader).unwrap();

    let ifd0 = parser.parse_ifd0().unwrap();

    // Check Make tag
    if let Some(make_entry) = ifd0.get(TiffTag::Make) {
        let make_value = parser.read_value(make_entry).unwrap();
        if let Some(make) = make_value.as_str()
            && let Some(ref camera_info) = gt.camera_info
        {
            assert!(
                make.contains(&camera_info.make),
                "Make mismatch: expected to contain '{}', got '{}'",
                camera_info.make,
                make
            );
        }
    }

    // Check Model tag
    if let Some(model_entry) = ifd0.get(TiffTag::Model) {
        let model_value = parser.read_value(model_entry).unwrap();
        if let Some(model) = model_value.as_str()
            && let Some(ref camera_info) = gt.camera_info
        {
            assert!(
                model.contains(&camera_info.model) || camera_info.model.contains(model),
                "Model mismatch: expected to contain '{}', got '{}'",
                camera_info.model,
                model
            );
        }
    }
}
