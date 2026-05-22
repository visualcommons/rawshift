//! HEIC auxiliary-image and metadata integration tests.
//!
//! These exercise the full `HeicFile` API: primary decode, auxiliary-image
//! enumeration and decode, thumbnails, and metadata extraction.
//!
//! They run against a committed project fixture in `test_data/standard/heic/`
//! when present, otherwise against libheif's bundled sample HEIC, and skip
//! gracefully when no HEIC file can be found.

use rawshift_image::core::MetadataNamespace;
use rawshift_image::formats::{HeicAuxKind, HeicFile};
use std::path::PathBuf;

/// Locate a HEIC file to test against, or `None` to skip.
fn sample_heic() -> Option<Vec<u8>> {
    // Prefer a committed project fixture.
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_data/standard/heic");
    if let Ok(entries) = std::fs::read_dir(&fixture_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_heic = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("heic") || s.eq_ignore_ascii_case("heif"))
                .unwrap_or(false);
            if is_heic {
                if let Ok(data) = std::fs::read(&path) {
                    return Some(data);
                }
            }
        }
    }
    // Fall back to libheif's bundled sample (Homebrew / system install).
    for path in [
        "/opt/homebrew/share/libheif/example.heic",
        "/usr/local/share/libheif/example.heic",
        "/usr/share/libheif/example.heic",
    ] {
        if let Ok(data) = std::fs::read(path) {
            return Some(data);
        }
    }
    None
}

macro_rules! heic_or_skip {
    () => {
        match sample_heic() {
            Some(data) => data,
            None => {
                eprintln!("Skipping HEIC test: no HEIC sample found");
                return;
            }
        }
    };
}

#[test]
fn heic_primary_decodes() {
    let data = heic_or_skip!();
    let file = HeicFile::open(data).expect("open HEIC");
    let img = file.decode_primary().expect("decode primary");
    assert!(img.width() > 0 && img.height() > 0);
    assert_eq!(
        img.data.len(),
        img.width() as usize * img.height() as usize * 3,
        "primary RGB buffer must be width*height*3"
    );
}

#[test]
fn heic_aux_images_decode() {
    let data = heic_or_skip!();
    let file = HeicFile::open(data).expect("open HEIC");
    for aux in file.aux_images() {
        let img = file
            .decode_aux(aux)
            .unwrap_or_else(|e| panic!("decode aux {:?}: {e}", aux.kind));
        assert_eq!(img.width(), aux.width, "aux width mismatch");
        assert_eq!(img.height(), aux.height, "aux height mismatch");
    }
}

#[test]
fn heic_thumbnail_matches_aux_listing() {
    let data = heic_or_skip!();
    let file = HeicFile::open(data).expect("open HEIC");
    let has_thumb = file
        .aux_images()
        .iter()
        .any(|a| a.kind == HeicAuxKind::Thumbnail);
    let thumb = file.thumbnail().expect("thumbnail");
    assert_eq!(
        has_thumb,
        thumb.is_some(),
        "thumbnail() must agree with the aux_images() listing"
    );
}

#[test]
fn heic_metadata_carries_container_facts() {
    let data = heic_or_skip!();
    let file = HeicFile::open(data).expect("open HEIC");
    let md = file.metadata();
    // The generic table always records HEIC container facts.
    assert!(
        md.get(MetadataNamespace::Heic, "width").is_some(),
        "HEIC width must be recorded in the generic metadata table"
    );
    assert!(
        md.get(MetadataNamespace::Heic, "height").is_some(),
        "HEIC height must be recorded in the generic metadata table"
    );
    assert!(md.image.bit_depth >= 8, "bit depth must be populated");
}
