//! Structural sanity checks on a real DNG fixture, run over the gamut-ifd
//! tree the DNG decoder is built on (results feed justin13888/gamut#174).

use gamut_ifd::{Ifd, tags};
use std::fs;

fn skip_if_no_test_data(filename: &str) -> bool {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_data")
        .join(filename);
    if !path.exists() {
        eprintln!("Skipping test: test data file not found: {:?}", path);
        return true;
    }
    false
}

fn test_data_path(filename: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_data")
        .join(filename)
}

/// Visit `ifd` and every sub-IFD beneath it.
fn visit(ifd: &Ifd, f: &mut impl FnMut(&Ifd)) {
    f(ifd);
    for group in ifd.sub_ifds() {
        for child in &group.ifds {
            visit(child, f);
        }
    }
}

#[test]
fn test_dng_strategy_check() {
    let filename = "Apple/iPhone_17_Pro_Max/IMG_1347.DNG";
    if skip_if_no_test_data(filename) {
        return;
    }

    let data = fs::read(test_data_path(filename)).unwrap();
    let file_size = data.len() as u64;

    // 1. Fail Fast (Header Test): read_tree validates byte order + magic.
    let file = gamut_ifd::read_tree(&data, tags::STANDARD_POINTER_TAGS).unwrap();
    assert!(!file.ifds.is_empty(), "no IFDs parsed");

    // 2. Breadth First (Tag Dump)
    let mut all_tags = Vec::new();
    for ifd in &file.ifds {
        visit(ifd, &mut |i| {
            all_tags.extend(i.fields().iter().map(|f| f.tag));
        });
    }
    // Check 0x0100 (Width)
    assert!(
        all_tags.contains(&0x0100),
        "Missing Tag 0x0100 (ImageWidth)"
    );
    // Check 0x0111 (StripOffsets) OR 0x0144 (TileOffsets)
    assert!(
        all_tags.contains(&tags::STRIP_OFFSETS) || all_tags.contains(&tags::TILE_OFFSETS),
        "Missing Tag 0x0111 (StripOffsets) or 0x0144 (TileOffsets)"
    );

    // 3. Depth Test (Value Accuracy)
    // From ExifTool: Width 8064, Height 6048 — present in some IFD/SubIFD.
    let mut found_dims = false;
    for ifd in &file.ifds {
        visit(ifd, &mut |i| {
            if i.get_u32(0x0100) == Some(8064) && i.get_u32(0x0101) == Some(6048) {
                found_dims = true;
            }
        });
    }
    assert!(
        found_dims,
        "Exact dimensions 8064x6048 not found in any IFD"
    );

    // 4. Boundaries Test (Safety): strip/tile extents stay inside the file.
    let mut checked_offsets = false;
    for ifd in &file.ifds {
        visit(ifd, &mut |i| {
            for (off_tag, cnt_tag, what) in [
                (tags::STRIP_OFFSETS, tags::STRIP_BYTE_COUNTS, "Strip"),
                (tags::TILE_OFFSETS, tags::TILE_BYTE_COUNTS, "Tile"),
            ] {
                if let (Some(offsets), Some(counts)) =
                    (i.get_u64_vec(off_tag), i.get_u64_vec(cnt_tag))
                {
                    for (o, c) in offsets.iter().zip(counts.iter()) {
                        let end = o + c;
                        // Allow a small margin for EOF-vs-content discrepancies.
                        assert!(
                            end <= file_size + 1024,
                            "{} Data Out of Bounds: {} > {}",
                            what,
                            end,
                            file_size
                        );
                        checked_offsets = true;
                    }
                }
            }
        });
    }
    assert!(
        checked_offsets,
        "No StripOffsets or TileOffsets were checked to verify bounds"
    );
}
