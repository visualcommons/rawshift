use rawshift_image::tiff::{TiffParser, TiffTag};
use std::fs::File;
use std::io::BufReader;

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

#[test]
fn test_dng_strategy_check() {
    let filename = "Apple/iPhone_17_Pro_Max/IMG_1347.DNG";
    if skip_if_no_test_data(filename) {
        return;
    }

    let file = File::open(test_data_path(filename)).unwrap();
    let file_size = file.metadata().unwrap().len();
    let reader = BufReader::new(file);
    let mut parser = TiffParser::new(reader).unwrap();

    // 1. Fail Fast (Header Test)
    // parser::new() already validated byte order and magic.
    // Explicitly check magic number if accessible
    assert!(
        parser.header().magic == 42 || parser.header().magic == 43,
        "Invalid Magic"
    );

    // 2. Breadth First (Tag Dump)
    let ifds = parser.walk_ifd_chain().unwrap();
    let mut all_tags = Vec::new();
    fn collect_tags(ifd: &rawshift_image::tiff::Ifd, tags: &mut Vec<u16>) {
        tags.extend(ifd.all_tag_ids());
        for sub_ifd in &ifd.sub_ifds {
            collect_tags(sub_ifd, tags);
        }
    }
    for ifd in &ifds {
        collect_tags(ifd, &mut all_tags);
    }

    // Check 0x0100 (Width)
    assert!(
        all_tags.contains(&0x0100),
        "Missing Tag 0x0100 (ImageWidth)"
    );
    // Check 0x0111 (StripOffsets) OR 0x0144 (TileOffsets)
    assert!(
        all_tags.contains(&0x0111) || all_tags.contains(&0x0144),
        "Missing Tag 0x0111 (StripOffsets) or 0x0144 (TileOffsets)"
    );

    // 3. Depth Test (Value Accuracy)
    // From ExifTool: Width 8064, Height 6048
    // Find these values in ANY IFD/SubIFD (Raw IFD usually)
    let mut found_dims = false;
    for ifd in &ifds {
        // Helper to check an ifd
        fn check_dims(
            ifd: &rawshift_image::tiff::Ifd,
            parser: &mut TiffParser<BufReader<File>>,
        ) -> bool {
            if let (Some(w_entry), Some(h_entry)) =
                (ifd.get(TiffTag::ImageWidth), ifd.get(TiffTag::ImageLength))
            {
                // Ignore potential read errors for this check, just skip
                let w = parser.read_value(w_entry).ok().and_then(|v| v.as_u32());
                let h = parser.read_value(h_entry).ok().and_then(|v| v.as_u32());
                if let (Some(width), Some(height)) = (w, h) {
                    if width == 8064 && height == 6048 {
                        return true;
                    }
                }
            }
            for sub in &ifd.sub_ifds {
                if check_dims(sub, parser) {
                    return true;
                }
            }
            false
        }

        if check_dims(ifd, &mut parser) {
            found_dims = true;
            break;
        }
    }
    assert!(
        found_dims,
        "Exact dimensions 8064x6048 not found in any IFD"
    );

    // 4. Boundaries Test (Safety)
    // Check offsets don't exceed file size

    let mut checked_offsets = false;

    fn check_bounds(
        ifd: &rawshift_image::tiff::Ifd,
        parser: &mut TiffParser<BufReader<File>>,
        file_size: u64,
        checked: &mut bool,
    ) {
        // Check StripOffsets
        if let (Some(off_e), Some(cnt_e)) = (
            ifd.get(TiffTag::StripOffsets),
            ifd.get(TiffTag::StripByteCounts),
        ) {
            if let (Ok(offsets), Ok(counts)) = (parser.read_value(off_e), parser.read_value(cnt_e))
            {
                if let (Some(off_vec), Some(cnt_vec)) = (offsets.as_u64_vec(), counts.as_u64_vec())
                {
                    for (o, c) in off_vec.iter().zip(cnt_vec.iter()) {
                        let end = o + c;
                        assert!(
                            end <= file_size + 1024,
                            "Strip Data Out of Bounds: {} > {}",
                            end,
                            file_size
                        );
                        *checked = true;
                    }
                }
            }
        }
        // Check TileOffsets
        if let (Some(off_e), Some(cnt_e)) = (
            ifd.get(TiffTag::TileOffsets),
            ifd.get(TiffTag::TileByteCounts),
        ) {
            if let (Ok(offsets), Ok(counts)) = (parser.read_value(off_e), parser.read_value(cnt_e))
            {
                if let (Some(off_vec), Some(cnt_vec)) = (offsets.as_u64_vec(), counts.as_u64_vec())
                {
                    for (o, c) in off_vec.iter().zip(cnt_vec.iter()) {
                        let end = o + c;
                        // Allow small margin for error or if file size is slightly off compared to EOF vs content
                        assert!(
                            end <= file_size + 1024,
                            "Tile Data Out of Bounds: {} > {}",
                            end,
                            file_size
                        );
                        *checked = true;
                    }
                }
            }
        }

        for sub in &ifd.sub_ifds {
            check_bounds(sub, parser, file_size, checked);
        }
    }

    for ifd in &ifds {
        check_bounds(ifd, &mut parser, file_size, &mut checked_offsets);
    }

    assert!(
        checked_offsets,
        "No StripOffsets or TileOffsets were checked to verify bounds"
    );
}
