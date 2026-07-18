//! HEIC auxiliary-image and metadata integration tests.
//!
//! These exercise the `HeicFile` API on gamut-heic. Container parsing,
//! metadata extraction, and auxiliary-image enumeration are **backend-less**
//! — they must pass with no hardware HEVC decoder at all. Pixel decode is
//! asserted to report the matchable `RawError::HwDecoderUnavailable` until a
//! rawshift-hwdec platform backend lands (VAAPI is #29); those assertions
//! flip to real decode checks then.
//!
//! Two fixture sources:
//! - A synthetic HEIF container (built with gamut-isobmff) that always runs.
//! - An on-disk HEIC in `test_data/standard/heic/` when present (e.g. a real
//!   camera file); skipped gracefully otherwise.

use rawshift_image::core::MetadataNamespace;
use rawshift_image::error::RawError;
use rawshift_image::formats::{HeicAuxKind, HeicFile, heic_hw_decode_available};
use std::path::PathBuf;

// ── fixture sources ──────────────────────────────────────────────────────────

/// Locate an on-disk HEIC file to test against, or `None` to skip.
fn sample_heic() -> Option<Vec<u8>> {
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
            if is_heic && let Ok(data) = std::fs::read(&path) {
                return Some(data);
            }
        }
    }
    None
}

macro_rules! heic_or_skip {
    () => {
        match sample_heic() {
            Some(data) => data,
            None => {
                eprintln!("Skipping HEIC fixture test: no file in test_data/standard/heic");
                return;
            }
        }
    };
}

/// A synthetic HEIC container: an hvc1 primary with a thumbnail, depth /
/// gain-map / alpha auxiliaries, and EXIF + XMP metadata items. Pure
/// container bytes — decodable structure with no real HEVC codestream, which
/// is exactly what the backend-less paths consume.
fn synthetic_heic() -> Vec<u8> {
    use gamut_isobmff::{IsoBmffImage, Item, ItemReference, Property, PropertyKind, write};

    // Minimal valid hvcC: version 1, 4:2:0, 4-byte NAL length prefixes.
    let mut hvcc = vec![0u8; 23];
    hvcc[0] = 1;
    hvcc[16] = 0b0000_0001;
    hvcc[21] = 0b0000_0011;

    let coded = |id: u32, width: u32, height: u32| Item {
        id,
        item_type: *b"hvc1",
        name: String::new(),
        content_type: None,
        content_encoding: None,
        hidden: false,
        references: vec![],
        properties: vec![
            Property {
                essential: true,
                kind: PropertyKind::CodecConfiguration {
                    kind: *b"hvcC",
                    data: hvcc.clone(),
                },
            },
            Property {
                essential: false,
                kind: PropertyKind::ImageSpatialExtents { width, height },
            },
        ],
        // 4-byte length prefix + an IDR_W_RADL NAL (type 19).
        payload: vec![0x00, 0x00, 0x00, 0x03, 0x26, 0x01, 0xDD],
    };

    let primary = coded(1, 8, 6);
    let mut thumb = coded(2, 4, 3);
    thumb.references.push(ItemReference {
        reference_type: *b"thmb",
        to_item_ids: vec![1],
    });
    let aux = |id: u32, urn: &str| {
        let mut item = coded(id, 8, 6);
        item.references.push(ItemReference {
            reference_type: *b"auxl",
            to_item_ids: vec![1],
        });
        item.properties.push(Property {
            essential: false,
            kind: PropertyKind::AuxiliaryType {
                aux_type: urn.to_string(),
                aux_subtype: vec![],
            },
        });
        item
    };
    let depth = aux(3, "urn:mpeg:hevc:2015:auxid:2");
    let gain = aux(4, "urn:com:apple:photo:2020:aux:hdrgainmap");
    let alpha = aux(5, "urn:mpeg:mpegB:cicp:systems:auxiliary:alpha");

    // EXIF item: 4-byte exif_tiff_header_offset + a minimal TIFF stream.
    let mut exif_payload = vec![0u8; 4];
    exif_payload.extend_from_slice(b"II");
    exif_payload.extend_from_slice(&42u16.to_le_bytes());
    exif_payload.extend_from_slice(&8u32.to_le_bytes());
    exif_payload.extend_from_slice(&0u16.to_le_bytes());
    exif_payload.extend_from_slice(&0u32.to_le_bytes());
    let exif = Item {
        id: 6,
        item_type: *b"Exif",
        name: String::new(),
        content_type: None,
        content_encoding: None,
        hidden: false,
        references: vec![ItemReference {
            reference_type: *b"cdsc",
            to_item_ids: vec![1],
        }],
        properties: vec![],
        payload: exif_payload,
    };

    write(&IsoBmffImage {
        major_brand: *b"heic",
        minor_version: 0,
        compatible_brands: vec![*b"heic", *b"mif1"],
        primary_item_id: 1,
        items: vec![primary, thumb, depth, gain, alpha, exif],
        groups: vec![],
    })
    .expect("write synthetic HEIC")
}

// ── backend-less paths: synthetic container (always runs) ───────────────────

#[test]
fn synthetic_heic_enumerates_aux_images_backend_less() {
    let file = HeicFile::open(synthetic_heic()).expect("open synthetic HEIC");
    let aux = file.aux_images();
    assert_eq!(aux.len(), 4, "thumbnail + depth + gain map + alpha");
    let count = |k: HeicAuxKind| aux.iter().filter(|a| a.kind == k).count();
    assert_eq!(count(HeicAuxKind::Thumbnail), 1);
    assert_eq!(count(HeicAuxKind::DepthMap), 1);
    assert_eq!(count(HeicAuxKind::GainMap), 1);
    assert_eq!(count(HeicAuxKind::Auxiliary), 1, "alpha lists as Auxiliary");

    let thumb = aux
        .iter()
        .find(|a| a.kind == HeicAuxKind::Thumbnail)
        .unwrap();
    assert_eq!((thumb.width, thumb.height), (4, 3));
}

#[test]
fn synthetic_heic_metadata_backend_less() {
    let file = HeicFile::open(synthetic_heic()).expect("open synthetic HEIC");
    let md = file.metadata();
    assert!(md.exif_raw.is_some(), "EXIF item must surface");
    assert_eq!(
        md.get(MetadataNamespace::Heic, "width"),
        Some(&rawshift_image::core::MetadataValue::U64(8))
    );
    assert_eq!(
        md.get(MetadataNamespace::Heic, "height"),
        Some(&rawshift_image::core::MetadataValue::U64(6))
    );
    assert_eq!(
        md.get(MetadataNamespace::Heic, "has_alpha"),
        Some(&rawshift_image::core::MetadataValue::U64(1))
    );
    assert!(md.image.bit_depth >= 8, "bit depth must be populated");
}

/// Until a rawshift-hwdec platform backend lands, pixel decode reports
/// `HwDecoderUnavailable` — an explicit assertion of the accepted state, not
/// a skipped test. When #29 lands on a machine with a usable VAAPI driver,
/// `heic_hw_decode_available()` flips and this test must be updated to assert
/// real decode.
#[test]
fn synthetic_heic_pixel_decode_reports_unavailable_without_backend() {
    let file = HeicFile::open(synthetic_heic()).expect("open synthetic HEIC");
    if heic_hw_decode_available() {
        eprintln!("hardware HEVC decoder available; unavailability assertions do not apply");
        return;
    }
    let err = file.decode_primary().unwrap_err();
    assert!(
        matches!(err, RawError::HwDecoderUnavailable { codec: "HEVC", .. }),
        "expected HwDecoderUnavailable, got: {err}"
    );
    for aux in file.aux_images() {
        let err = file.decode_aux(aux).unwrap_err();
        assert!(matches!(err, RawError::HwDecoderUnavailable { .. }));
    }
    // thumbnail() decodes, so it reports the same unavailability.
    assert!(matches!(
        file.thumbnail(),
        Err(RawError::HwDecoderUnavailable { .. })
    ));
}

// ── backend-less paths: on-disk fixture (skips without a file) ──────────────

#[test]
fn heic_fixture_opens_and_enumerates_backend_less() {
    let data = heic_or_skip!();
    let file = HeicFile::open(data).expect("open HEIC");
    // Enumeration itself must work; a real camera file usually carries at
    // least a thumbnail, but that is fixture-dependent — only shape-check.
    for aux in file.aux_images() {
        assert!(aux.width > 0, "aux items should carry ispe dimensions");
        assert!(aux.height > 0);
    }
}

#[test]
fn heic_fixture_metadata_carries_container_facts() {
    let data = heic_or_skip!();
    let file = HeicFile::open(data).expect("open HEIC");
    let md = file.metadata();
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

#[test]
fn heic_fixture_pixel_decode_matches_advertised_availability() {
    let data = heic_or_skip!();
    let file = HeicFile::open(data).expect("open HEIC");
    if heic_hw_decode_available() {
        // A backend exists (post-#29): decode must actually work.
        let img = file.decode_primary().expect("decode primary");
        assert!(img.width() > 0 && img.height() > 0);
        assert_eq!(
            img.data().len(),
            img.width() as usize * img.height() as usize * 3
        );
    } else {
        // Accepted state until a platform backend lands.
        let err = file.decode_primary().unwrap_err();
        assert!(
            matches!(err, RawError::HwDecoderUnavailable { codec: "HEVC", .. }),
            "expected HwDecoderUnavailable, got: {err}"
        );
    }
}

#[test]
fn heic_fixture_thumbnail_agrees_with_aux_listing() {
    let data = heic_or_skip!();
    let file = HeicFile::open(data).expect("open HEIC");
    let has_thumb = file
        .aux_images()
        .iter()
        .any(|a| a.kind == HeicAuxKind::Thumbnail);
    match file.thumbnail() {
        Ok(thumb) => assert_eq!(
            has_thumb,
            thumb.is_some(),
            "thumbnail() must agree with the aux_images() listing"
        ),
        // Without a hardware backend the listing may know a thumbnail exists
        // while decoding it is unavailable.
        Err(RawError::HwDecoderUnavailable { .. }) => {
            assert!(has_thumb, "unavailability implies a thumbnail was listed");
            assert!(!heic_hw_decode_available());
        }
        Err(other) => panic!("unexpected thumbnail error: {other}"),
    }
}
