//! End-to-end AVIF hardware decode: `AvifFile` through gamut-avif's pipeline
//! into the rawshift-hwdec VAAPI backend (#29), on a real GPU.
//!
//! Fixtures are generated **at test time** with `avifenc` (libavif) from
//! synthetic Y4M/PNG sources written by the test itself, so nothing is
//! committed and nothing needs human sourcing. Gated three ways: compiled
//! only with `hw` + `avif-decode`, and each test skips gracefully (eprintln +
//! return) when `avifenc` is not installed or no hardware AV1 decoder is
//! usable at runtime — CI without a GPU stays green, while a machine with a
//! working VAAPI/VideoToolbox/MediaCodec driver asserts real pixels.
//!
//! Coverage per issue #33: 8-bit 4:2:0 (lossless → bit-exact), alpha
//! auxiliary, `grid` derivation, 10-bit (hardware decode works; the RGBA
//! presentation gates upstream — visualcommons/gamut#303 — so the specific
//! error is asserted), Exif/XMP items, and the AV1-profile scope of
//! rawshift's own gamut-encoded output.

#![cfg(all(feature = "hw", feature = "avif-decode"))]

use std::path::{Path, PathBuf};
use std::process::Command;

use gamut_color::{ColorRange, ycbcr_to_rgb};
use rawshift_image::error::RawError;
use rawshift_image::formats::{AvifAuxKind, AvifFile, avif_hw_decode_available};

// ── fixture generation ───────────────────────────────────────────────────────

/// A scratch directory for this test binary's generated fixtures.
fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rawshift_avif_hw_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Whether `avifenc` (libavif) is runnable on this machine.
fn avifenc_available() -> bool {
    Command::new("avifenc")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Run `avifenc args… input output`, returning the encoded bytes.
fn avifenc(args: &[&str], input: &Path, output: &Path) -> Vec<u8> {
    let status = Command::new("avifenc")
        .args(args)
        .arg("-o")
        .arg(output)
        .arg(input)
        .output()
        .expect("spawn avifenc");
    assert!(
        status.status.success(),
        "avifenc failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    std::fs::read(output).expect("read avifenc output")
}

/// Write an 8-bit 4:2:0 Y4M with constant planes (Y4M carries no range
/// marker, so libavif treats it as limited range).
fn write_y4m_420(path: &Path, w: usize, h: usize, y: u8, cb: u8, cr: u8) {
    let mut out = format!("YUV4MPEG2 W{w} H{h} F25:1 Ip A1:1 C420jpeg\nFRAME\n").into_bytes();
    out.extend(std::iter::repeat_n(y, w * h));
    out.extend(std::iter::repeat_n(cb, (w / 2) * (h / 2)));
    out.extend(std::iter::repeat_n(cr, (w / 2) * (h / 2)));
    std::fs::write(path, out).expect("write y4m");
}

/// Write a 10-bit 4:2:0 Y4M (`C420p10`, 16-bit little-endian samples) with
/// constant planes.
fn write_y4m_420p10(path: &Path, w: usize, h: usize, y: u16, cb: u16, cr: u16) {
    let mut out = format!("YUV4MPEG2 W{w} H{h} F25:1 Ip A1:1 C420p10\nFRAME\n").into_bytes();
    let mut plane = |value: u16, samples: usize| {
        for _ in 0..samples {
            out.extend_from_slice(&value.to_le_bytes());
        }
    };
    plane(y, w * h);
    plane(cb, (w / 2) * (h / 2));
    plane(cr, (w / 2) * (h / 2));
    std::fs::write(path, out).expect("write 10-bit y4m");
}

/// Write an 8-bit RGBA PNG: solid `rgb`, left half opaque, right half fully
/// transparent. Self-contained writer (stored-DEFLATE zlib stream), so the
/// test needs no PNG encoder dependency.
fn write_rgba_png(path: &Path, w: usize, h: usize, rgb: [u8; 3]) {
    // Raw scanlines: filter byte 0 + RGBA pixels. Every row is identical.
    let mut row = vec![0u8]; // filter: None
    for x in 0..w {
        let a = if x < w / 2 { 255 } else { 0 };
        row.extend_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
    }
    let raw = row.repeat(h);

    // zlib stream: header + stored (uncompressed) DEFLATE blocks + adler32.
    let mut zlib = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(block) = chunks.next() {
        zlib.push(if chunks.peek().is_none() { 1 } else { 0 });
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (ty, body) in [
        (*b"IHDR", ihdr.as_slice()),
        (*b"IDAT", zlib.as_slice()),
        (*b"IEND", &[][..]),
    ] {
        png.extend_from_slice(&(body.len() as u32).to_be_bytes());
        png.extend_from_slice(&ty);
        png.extend_from_slice(body);
        png.extend_from_slice(&crc32(&ty, body).to_be_bytes());
    }
    std::fs::write(path, png).expect("write png");
}

/// CRC-32 (ISO 3309, as PNG uses) over `ty || body` — bitwise, no table.
fn crc32(ty: &[u8; 4], body: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in ty.iter().chain(body) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

/// A minimal little-endian TIFF stream carrying one `Make` tag — the payload
/// handed to `avifenc --exif`.
fn tiff_with_make(make: &str) -> Vec<u8> {
    let mut text = make.as_bytes().to_vec();
    text.push(0);
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&42u16.to_le_bytes());
    tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD at offset 8
    tiff.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
    tiff.extend_from_slice(&0x010Fu16.to_le_bytes()); // Make
    tiff.extend_from_slice(&2u16.to_le_bytes()); // ASCII
    tiff.extend_from_slice(&(text.len() as u32).to_le_bytes());
    tiff.extend_from_slice(&26u32.to_le_bytes()); // value offset (after IFD)
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    debug_assert_eq!(tiff.len(), 26);
    tiff.extend_from_slice(&text);
    tiff
}

macro_rules! avifenc_or_skip {
    () => {
        if !avifenc_available() {
            eprintln!("Skipping AVIF hardware decode test: avifenc (libavif) not installed");
            return;
        }
    };
}

macro_rules! hw_or_skip {
    () => {
        if !avif_hw_decode_available() {
            eprintln!(
                "Skipping AVIF hardware decode test: no hardware AV1 decoder \
                 usable at runtime on this machine"
            );
            return;
        }
    };
}

/// The 16-bit RGB triple gamut presents for a constant limited-range BT.601
/// YCbCr plane set (the expectation source is gamut-color itself, so a
/// lossless encode must match bit-exactly).
fn expected_rgb16(y: u8, cb: u8, cr: u8) -> [u16; 3] {
    let (r, g, b) = ycbcr_to_rgb(y, cb, cr, ColorRange::Limited);
    [u16::from(r) * 257, u16::from(g) * 257, u16::from(b) * 257]
}

// ── tests ────────────────────────────────────────────────────────────────────

/// The full stack on real hardware: avifenc 8-bit 4:2:0 **lossless** (qindex
/// 0) → container parse (gamut-avif) → av1C + OBU payload → rawshift-hwdec
/// decode → BT.601 presentation. The YUV planes survive bit-exactly and the
/// expectation is computed with gamut-color, so every output pixel is
/// asserted exactly.
#[test]
fn avif_8bit_420_hw_decodes_bit_exact_end_to_end() {
    avifenc_or_skip!();
    hw_or_skip!();
    let dir = scratch_dir();
    let (y, cb, cr) = (81u8, 90u8, 240u8);
    write_y4m_420(&dir.join("solid.y4m"), 64, 64, y, cb, cr);
    let data = avifenc(
        &["--min", "0", "--max", "0", "-s", "8"],
        &dir.join("solid.y4m"),
        &dir.join("solid.avif"),
    );

    let file = AvifFile::open(data.clone()).expect("open AVIF fixture");
    let image = file.decode_primary().expect("hardware decode primary");
    assert_eq!((image.width(), image.height()), (64, 64));

    let expected = expected_rgb16(y, cb, cr);
    for px in image.data().chunks_exact(3) {
        assert_eq!(px, expected, "lossless 4:2:0 decode must be bit-exact");
    }

    // The generic entry point routes to the same backend.
    let via_standard = rawshift_image::formats::decode_standard_image(
        &data,
        rawshift_image::formats::StandardFormat::Avif,
    )
    .expect("decode via decode_standard_image");
    assert_eq!(via_standard.data(), image.data());
    eprintln!("AVIF end-to-end hardware decode: 64x64 bit-exact, expected {expected:?}");
}

/// Alpha auxiliary: enumeration classifies it, metadata reports it, the
/// primary hardware-decodes with the alpha merged by gamut-avif's pipeline,
/// and decoding the auxiliary item itself yields the alpha plane as gray
/// (left half opaque, right half transparent).
#[test]
fn avif_alpha_auxiliary_hw_decodes() {
    avifenc_or_skip!();
    hw_or_skip!();
    let dir = scratch_dir();
    write_rgba_png(&dir.join("alpha.png"), 64, 64, [200, 50, 30]);
    // `-y 420` keeps the colour item in AV1 Profile 0 (avifenc's PNG default
    // is 4:4:4 = Profile 1, outside today's hardware scope).
    let data = avifenc(
        &["-s", "8", "-y", "420"],
        &dir.join("alpha.png"),
        &dir.join("alpha.avif"),
    );

    let file = AvifFile::open(data).expect("open alpha AVIF");
    let alpha = file
        .aux_images()
        .iter()
        .find(|a| a.kind == AvifAuxKind::Alpha)
        .expect("alpha auxiliary enumerated")
        .clone();
    assert_eq!((alpha.width, alpha.height), (64, 64));

    let md = file.metadata();
    assert_eq!(
        md.get(rawshift_image::core::MetadataNamespace::Avif, "has_alpha"),
        Some(&rawshift_image::core::MetadataValue::U64(1))
    );

    // Primary: alpha is merged inside the RGBA pipeline (then dropped by the
    // RGB presentation); the decode itself must succeed on hardware.
    let primary = file.decode_primary().expect("hardware decode primary");
    assert_eq!((primary.width(), primary.height()), (64, 64));

    // The auxiliary decodes standalone to a gray-expanded alpha plane:
    // opaque left, transparent right.
    let plane = file.decode_aux(&alpha).expect("hardware decode alpha item");
    let samples = plane.data();
    let row = 32usize;
    let left = samples[(row * 64 + 8) * 3];
    let right = samples[(row * 64 + 56) * 3];
    assert!(left > 60000, "opaque half must be ~white, got {left}");
    assert!(right < 3000, "transparent half must be ~black, got {right}");
    eprintln!("AVIF alpha auxiliary: merged primary + plane decode (left {left}, right {right})");
}

/// `grid` derivation: a 2x1 tiled AVIF (two 64x64 cells) is assembled by
/// gamut-avif in the plane domain from two hardware-decoded tiles; lossless
/// coding keeps the assembled output bit-exact.
#[test]
fn avif_grid_hw_decodes_bit_exact() {
    avifenc_or_skip!();
    hw_or_skip!();
    let dir = scratch_dir();
    let (y, cb, cr) = (81u8, 90u8, 240u8);
    write_y4m_420(&dir.join("grid.y4m"), 128, 64, y, cb, cr);
    let data = avifenc(
        &["--min", "0", "--max", "0", "-s", "8", "--grid", "2x1"],
        &dir.join("grid.y4m"),
        &dir.join("grid.avif"),
    );

    let file = AvifFile::open(data).expect("open grid AVIF");
    let image = file.decode_primary().expect("hardware decode grid");
    assert_eq!((image.width(), image.height()), (128, 64));
    let expected = expected_rgb16(y, cb, cr);
    for px in image.data().chunks_exact(3) {
        assert_eq!(px, expected, "lossless grid decode must be bit-exact");
    }
    eprintln!("AVIF grid 2x1: 128x64 assembled bit-exact from two hardware-decoded tiles");
}

/// 10-bit: the hardware decodes the P010 surface fine, but gamut-avif's RGBA
/// presentation surface is 8-bit-only until high-bit-depth presentation
/// lands upstream (the visualcommons/gamut#303 program). Assert the specific
/// upstream gate — not a hardware failure — so this test starts failing the
/// moment upstream unlocks it (then flip it to a pixel assertion).
#[test]
fn avif_10bit_hw_decode_gates_on_upstream_presentation() {
    avifenc_or_skip!();
    hw_or_skip!();
    let dir = scratch_dir();
    write_y4m_420p10(&dir.join("solid10.y4m"), 64, 64, 324, 360, 960);
    let data = avifenc(
        &["--min", "0", "--max", "0", "-s", "8"],
        &dir.join("solid10.y4m"),
        &dir.join("solid10.avif"),
    );

    let file = AvifFile::open(data).expect("open 10-bit AVIF");
    assert_eq!(file.metadata().image.bit_depth, 10);
    let err = file
        .decode_primary()
        .expect_err("10-bit presentation gates upstream");
    let text = err.to_string();
    assert!(
        matches!(err, RawError::Format(_)) && text.contains(">8-bit"),
        "expected the gamut#303 presentation gate (a Format error mentioning \
         >8-bit), got: {text}"
    );
    eprintln!("AVIF 10-bit: hardware P010 decode reaches the documented gamut#303 gate: {text}");
}

/// Exif and XMP **items** (a foreign, avifenc-authored file — rawshift's own
/// encoder splices these too, covered by the standard.rs round-trip test):
/// the backend-less metadata path reads both. No hardware needed.
#[test]
fn avif_exif_xmp_items_read_backend_less() {
    avifenc_or_skip!();
    let dir = scratch_dir();
    write_y4m_420(&dir.join("meta.y4m"), 64, 64, 128, 128, 128);
    std::fs::write(dir.join("exif.bin"), tiff_with_make("RawshiftTest")).unwrap();
    let xmp = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf/></x:xmpmeta>";
    std::fs::write(dir.join("meta.xmp"), xmp).unwrap();
    let data = avifenc(
        &[
            "--min",
            "0",
            "--max",
            "0",
            "-s",
            "8",
            "--exif",
            dir.join("exif.bin").to_str().unwrap(),
            "--xmp",
            dir.join("meta.xmp").to_str().unwrap(),
        ],
        &dir.join("meta.y4m"),
        &dir.join("meta.avif"),
    );

    // Both the AvifFile surface and the generic entry point see the items.
    let md = AvifFile::open(data.clone()).expect("open").metadata();
    assert_eq!(md.camera.make, "RawshiftTest");
    assert_eq!(md.xmp.as_deref(), Some(&xmp[..]));

    let md = rawshift_image::formats::read_standard_image_metadata(
        &data,
        rawshift_image::formats::StandardFormat::Avif,
    );
    assert_eq!(md.camera.make, "RawshiftTest");
    assert_eq!(md.xmp.as_deref(), Some(&xmp[..]));
}

/// rawshift's own gamut-avif encoder emits lossless identity 4:4:4 — AV1
/// **Profile 1** — while today's hardware backends decode Profile 0 only
/// (docs/SUPPORT.md; VAAPI advertises `VAProfileAV1Profile0`). Pin the honest
/// behaviour on both kinds of machine: a Profile-0-only decoder must reject
/// it with the backend's scope error (not a crash, not "unavailable"), and a
/// decoder that does handle Profile 1 must round-trip the lossless pixels
/// bit-exactly.
#[cfg(feature = "avif-encode")]
#[test]
fn avif_rawshift_encoded_profile1_is_honest_about_hw_scope() {
    hw_or_skip!();
    use rawshift_image::core::{ImageMetadata, RgbImage};
    use rawshift_image::formats::encode_rgb_image_to_vec;
    use rawshift_image::formats::export::{AvifEncodeConfig, EncodeOptions};

    // Solid red, values exact at both endpoints of the 8-bit scale.
    let data: Vec<u16> = [65535u16, 0, 0].repeat(64 * 64);
    let rgb = RgbImage::new(64, 64, data).expect("valid RGB buffer");
    let avif = encode_rgb_image_to_vec(
        &rgb,
        &ImageMetadata::default(),
        &EncodeOptions::Avif(AvifEncodeConfig::default()),
    )
    .expect("encode lossless AVIF with gamut-avif");

    let file = AvifFile::open(avif).expect("open rawshift-encoded AVIF");
    match file.decode_primary() {
        Ok(image) => {
            // Hardware with AV1 Profile 1 support: lossless identity 4:4:4
            // must round-trip bit-exactly.
            assert_eq!((image.width(), image.height()), (64, 64));
            for px in image.data().chunks_exact(3) {
                assert_eq!(px, [65535, 0, 0]);
            }
            eprintln!("AVIF Profile 1: hardware decoded rawshift's lossless output bit-exact");
        }
        Err(err) => {
            let text = err.to_string();
            assert!(
                matches!(err, RawError::Format(_))
                    && (text.contains("Profile 0") || text.contains("profile")),
                "a Profile-0-only backend must reject Profile 1 with its scope \
                 error, got: {text}"
            );
            eprintln!("AVIF Profile 1: hardware honestly reports its scope: {text}");
        }
    }
}
