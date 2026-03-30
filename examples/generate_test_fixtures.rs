//! Generate synthetic test fixture files for standard image formats.
//!
//! This binary creates small test images in `test_data/standard/<format>/` and
//! corresponding ground-truth JSON in `test_fixtures/standard/<format>/`.
//!
//! Usage:
//!   cargo run --example generate_test_fixtures
//!
//! The generated fixtures are used by integration tests in
//! `tests/standard_decode_fixtures.rs`.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rawshift::core::image::RgbImage;
use rawshift::core::metadata::{
    CameraInfo, DateTimeInfo, ExifInfo, GpsInfo, ImageInfo, ImageMetadata, SRational, URational,
};
use rawshift::formats::encode_rgb_image;
use rawshift::formats::export::EncodeOptions;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn test_data_dir() -> PathBuf {
    project_root().join("test_data").join("standard")
}

fn test_fixtures_dir() -> PathBuf {
    project_root().join("test_fixtures").join("standard")
}

// ── Reference metadata ──────────────────────────────────────────────────────

/// Known metadata values embedded into every metadata-capable fixture.
/// Tests verify these exact values survive the encode → read round-trip.
fn reference_metadata() -> ImageMetadata {
    ImageMetadata {
        camera: CameraInfo {
            make: "rawshift-test".into(),
            model: "Synthetic-v1".into(),
            lens_make: Some("rawshift-optics".into()),
            lens_model: Some("TestLens 50mm f/1.4".into()),
            ..Default::default()
        },
        exif: ExifInfo {
            iso: Some(200),
            exposure_time: Some(URational::new(1, 125)),
            f_number: Some(URational::new(56, 10)),    // f/5.6
            focal_length: Some(URational::new(50, 1)), // 50mm
            focal_length_35mm: Some(75),
            exposure_program: Some(2), // Normal
            metering_mode: Some(5),    // Pattern
            flash: Some(0),            // No flash
            exposure_compensation: Some(SRational::new(0, 1)),
            max_aperture: Some(URational::new(14, 10)), // f/1.4
            brightness_value: Some(SRational::new(7, 1)),
        },
        datetime: DateTimeInfo {
            datetime_original: Some("2025:01:15 10:30:00".into()),
            create_date: Some("2025:01:15 10:30:00".into()),
            modify_date: Some("2025:01:15 10:30:01".into()),
            offset_time: Some("-05:00".into()),
            subsec_time: Some("123".into()),
        },
        gps: GpsInfo {
            latitude: Some([
                URational::new(43, 1),
                URational::new(39, 1),
                URational::new(0, 1),
            ]),
            latitude_ref: Some('N'),
            longitude: Some([
                URational::new(79, 1),
                URational::new(23, 1),
                URational::new(0, 1),
            ]),
            longitude_ref: Some('W'),
            altitude: Some(URational::new(76, 1)),
            altitude_ref: Some(0),
            ..Default::default()
        },
        image: ImageInfo {
            orientation: Some(1),
            ..Default::default()
        },
        xmp: Some(reference_xmp_bytes()),
        ..Default::default()
    }
}

/// Minimal valid XMP packet with Dublin Core fields for round-trip testing.
fn reference_xmp_bytes() -> Vec<u8> {
    // The XMP spec requires a BOM (U+FEFF) after begin='. We build it as a
    // regular string so Rust handles the UTF-8 encoding of the BOM correctly.
    r#"<?xpacket begin='﻿' id='W5M0MpCehiHzreSzNTczkc9d'?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:dc="http://purl.org/dc/elements/1.1/"
      xmlns:xmp="http://ns.adobe.com/xap/1.0/">
      <dc:description>rawshift synthetic test fixture</dc:description>
      <dc:creator><rdf:Seq><rdf:li>rawshift-test</rdf:li></rdf:Seq></dc:creator>
      <xmp:CreatorTool>rawshift generate_test_fixtures</xmp:CreatorTool>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end='w'?>"#
        .as_bytes()
        .to_vec()
}

// ── Pixel generation ────────────────────────────────────────────────────────

/// 8x8 RGB pixels with known colors for verification.
/// Row 0: red, green, blue, white, black, cyan, magenta, yellow
/// Rows 1-7: gradient patterns
fn reference_pixels_u8() -> (u32, u32, Vec<u8>) {
    let w = 8u32;
    let h = 8u32;
    let mut pixels = Vec::with_capacity((w * h * 3) as usize);

    // Row 0: primary + secondary colors
    let colors: [(u8, u8, u8); 8] = [
        (255, 0, 0),     // red
        (0, 255, 0),     // green
        (0, 0, 255),     // blue
        (255, 255, 255), // white
        (0, 0, 0),       // black
        (0, 255, 255),   // cyan
        (255, 0, 255),   // magenta
        (255, 255, 0),   // yellow
    ];
    for &(r, g, b) in &colors {
        pixels.extend_from_slice(&[r, g, b]);
    }

    // Rows 1-7: horizontal gradient
    for row in 1..h {
        for col in 0..w {
            let v = ((row * w + col) * 255 / (w * h - 1)) as u8;
            pixels.extend_from_slice(&[v, (255 - v), (v / 2 + 64)]);
        }
    }

    (w, h, pixels)
}

/// Convert 8-bit pixel data to 16-bit (u8 * 257 maps 0→0, 255→65535).
fn pixels_u8_to_u16(pixels_u8: &[u8]) -> Vec<u16> {
    pixels_u8.iter().map(|&v| v as u16 * 257).collect()
}

// ── Expected JSON ───────────────────────────────────────────────────────────

fn write_expected_json(
    dir: &Path,
    name: &str,
    width: u32,
    height: u32,
    format: &str,
    has_metadata: bool,
) {
    let metadata_block = if has_metadata {
        r#",
  "metadata": {
    "make": "rawshift-test",
    "model": "Synthetic-v1",
    "iso": 200,
    "focal_length_num": 50,
    "datetime_original": "2025:01:15 10:30:00",
    "has_icc": true,
    "has_xmp": true
  }"#
    } else {
        ""
    };

    let json = format!(
        r#"{{
  "format": "{}",
  "file_name": "{}",
  "width": {},
  "height": {},
  "channels": 3,
  "bit_depth_output": 16,
  "notes": "Synthetic {} fixture generated by generate_test_fixtures"{}
}}"#,
        format, name, width, height, format, metadata_block
    );
    fs::write(dir.join("expected.json"), json).expect("write expected.json");
}

// ── Format generators ───────────────────────────────────────────────────────

fn generate_jpeg(data_dir: &Path, fixture_dir: &Path) {
    let dir = data_dir.join("jpeg");
    let fdir = fixture_dir.join("jpeg");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&fdir).unwrap();

    let (w, h, pixels_u8) = reference_pixels_u8();
    let pixels_u16 = pixels_u8_to_u16(&pixels_u8);
    let img = RgbImage::new(w, h, pixels_u16);

    let name = "test_8x8.jpg";
    encode_rgb_image(
        &img,
        &reference_metadata(),
        &dir.join(name),
        &EncodeOptions::jpeg(),
    )
    .expect("JPEG encode");

    write_expected_json(&fdir, name, w, h, "JPEG", true);
    println!("  Generated {}", name);
}

fn generate_png(data_dir: &Path, fixture_dir: &Path) {
    let dir = data_dir.join("png");
    let fdir = fixture_dir.join("png");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&fdir).unwrap();

    let (w, h, pixels_u8) = reference_pixels_u8();
    let pixels_u16 = pixels_u8_to_u16(&pixels_u8);
    let img = RgbImage::new(w, h, pixels_u16);

    let name = "test_8x8.png";
    encode_rgb_image(
        &img,
        &reference_metadata(),
        &dir.join(name),
        &EncodeOptions::png(),
    )
    .expect("PNG encode");

    write_expected_json(&fdir, name, w, h, "PNG", true);
    println!("  Generated {}", name);
}

fn generate_gif(data_dir: &Path, fixture_dir: &Path) {
    use gif::{Encoder, Frame, Repeat};
    use std::borrow::Cow;

    let dir = data_dir.join("gif");
    let fdir = fixture_dir.join("gif");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&fdir).unwrap();

    // 4x4 GIF with 4-color palette
    let palette: &[u8] = &[
        255, 0, 0, // 0: red
        0, 255, 0, // 1: green
        0, 0, 255, // 2: blue
        255, 255, 255, // 3: white
        0, 0, 0, // padding
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    let mut out: Vec<u8> = Vec::new();
    let mut encoder = Encoder::new(&mut out, 4, 4, palette).expect("gif encoder");
    encoder.set_repeat(Repeat::Finite(0)).unwrap();

    let frame = Frame {
        width: 4,
        height: 4,
        // 4x4 grid: alternating pattern
        buffer: Cow::Owned(vec![0, 1, 2, 3, 1, 2, 3, 0, 2, 3, 0, 1, 3, 0, 1, 2]),
        ..Frame::default()
    };
    encoder.write_frame(&frame).unwrap();
    drop(encoder);

    let name = "test_4x4.gif";
    fs::write(dir.join(name), &out).expect("write GIF");
    write_expected_json(&fdir, name, 4, 4, "GIF", false);
    println!("  Generated {}", name);
}

fn generate_tiff(data_dir: &Path, fixture_dir: &Path) {
    use tiff::encoder::{TiffEncoder, colortype::RGB8};

    let dir = data_dir.join("tiff");
    let fdir = fixture_dir.join("tiff");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&fdir).unwrap();

    let (w, h, pixels) = reference_pixels_u8();
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut enc = TiffEncoder::new(&mut cursor).unwrap();
        enc.write_image::<RGB8>(w, h, &pixels).unwrap();
    }

    let name = "test_8x8.tiff";
    fs::write(dir.join(name), cursor.into_inner()).expect("write TIFF");
    write_expected_json(&fdir, name, w, h, "TIFF", false);
    println!("  Generated {}", name);
}

fn generate_webp(data_dir: &Path, fixture_dir: &Path) {
    let dir = data_dir.join("webp");
    let fdir = fixture_dir.join("webp");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&fdir).unwrap();

    let (w, h, pixels_u8) = reference_pixels_u8();
    let pixels_u16 = pixels_u8_to_u16(&pixels_u8);
    let img = RgbImage::new(w, h, pixels_u16);

    let name = "test_8x8.webp";
    encode_rgb_image(
        &img,
        &reference_metadata(),
        &dir.join(name),
        &EncodeOptions::webp_lossless(),
    )
    .expect("WebP encode");

    write_expected_json(&fdir, name, w, h, "WebP", true);
    println!("  Generated {}", name);
}

fn generate_svg(data_dir: &Path, fixture_dir: &Path) {
    let dir = data_dir.join("svg");
    let fdir = fixture_dir.join("svg");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&fdir).unwrap();

    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
  <rect x="0" y="0" width="4" height="4" fill="red"/>
  <rect x="4" y="0" width="4" height="4" fill="green"/>
  <rect x="0" y="4" width="4" height="4" fill="blue"/>
  <rect x="4" y="4" width="4" height="4" fill="white"/>
</svg>"#;

    let name = "test_8x8.svg";
    fs::write(dir.join(name), svg).expect("write SVG");
    write_expected_json(&fdir, name, 8, 8, "SVG", false);
    println!("  Generated {}", name);
}

#[cfg(feature = "avif-encode")]
fn generate_avif(data_dir: &Path, fixture_dir: &Path) {
    let dir = data_dir.join("avif");
    let fdir = fixture_dir.join("avif");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&fdir).unwrap();

    let (w, h, pixels_u8) = reference_pixels_u8();
    let pixels_u16 = pixels_u8_to_u16(&pixels_u8);
    let img = RgbImage::new(w, h, pixels_u16);

    let name = "test_8x8.avif";
    encode_rgb_image(
        &img,
        &reference_metadata(),
        &dir.join(name),
        &EncodeOptions::avif(),
    )
    .expect("AVIF encode");

    write_expected_json(&fdir, name, w, h, "AVIF", true);
    println!("  Generated {}", name);
}

#[cfg(feature = "jxl-encode")]
fn generate_jxl(data_dir: &Path, fixture_dir: &Path) {
    let dir = data_dir.join("jxl");
    let fdir = fixture_dir.join("jxl");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&fdir).unwrap();

    let (w, h, pixels_u8) = reference_pixels_u8();
    let pixels_u16 = pixels_u8_to_u16(&pixels_u8);
    let img = RgbImage::new(w, h, pixels_u16);

    let name = "test_8x8.jxl";
    encode_rgb_image(
        &img,
        &reference_metadata(),
        &dir.join(name),
        &EncodeOptions::jxl(),
    )
    .expect("JXL encode");

    // JXL metadata reading is not yet implemented, so no metadata in ground truth
    write_expected_json(&fdir, name, w, h, "JXL", false);
    println!("  Generated {}", name);
}

fn main() {
    let data_dir = test_data_dir();
    let fixture_dir = test_fixtures_dir();

    println!("Generating standard format test fixtures...");
    println!("  Data dir:    {}", data_dir.display());
    println!("  Fixture dir: {}", fixture_dir.display());
    println!();

    generate_jpeg(&data_dir, &fixture_dir);
    generate_png(&data_dir, &fixture_dir);
    generate_gif(&data_dir, &fixture_dir);
    generate_tiff(&data_dir, &fixture_dir);
    generate_webp(&data_dir, &fixture_dir);
    generate_svg(&data_dir, &fixture_dir);
    #[cfg(feature = "avif-encode")]
    generate_avif(&data_dir, &fixture_dir);
    #[cfg(feature = "jxl-encode")]
    generate_jxl(&data_dir, &fixture_dir);

    println!();
    println!("Done. Run integration tests with:");
    println!("  cargo test --test standard_decode_fixtures");
    #[cfg(feature = "avif-encode")]
    println!("  cargo test --features avif --test standard_decode_fixtures");
    #[cfg(feature = "jxl-encode")]
    println!("  cargo test --features jxl --test standard_decode_fixtures");
}
