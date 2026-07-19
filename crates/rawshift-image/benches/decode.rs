//! Benchmarks for image decode paths.
//!
//! Covers three layers:
//! - RAW data-structure primitives (creation, pixel access),
//! - the gamut-backed standard codecs (JPEG/PNG encode + decode round-trips
//!   on a synthetic image — the going-forward per-format regression baseline
//!   after the gamut migration),
//! - hardware HEIC/AVIF still decode (`hw` feature). The hardware benches
//!   generate their fixtures locally with `heif-enc` / `avifenc` and skip
//!   cleanly when the tool is not installed or no hardware decoder is usable
//!   at runtime, so `cargo bench` stays green on GPU-less machines and CI.

use criterion::{Criterion, criterion_group, criterion_main};
use rawshift_image::core::image::{CfaPattern, Dimensions, Point, RawImage, Rect};

/// Benchmark creating a RawImage (allocation + init).
fn bench_raw_image_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("raw_image_creation");

    for &(w, h) in &[(1000, 1000), (4000, 3000), (8000, 6000)] {
        group.bench_function(format!("{}x{}", w, h), |b| {
            b.iter(|| {
                let size = Dimensions {
                    width: w,
                    height: h,
                };
                let area = Rect::new(Point::ORIGIN, size);
                RawImage::new(size, area, 14, CfaPattern::Rggb)
            });
        });
    }

    group.finish();
}

/// Benchmark pixel access patterns.
fn bench_pixel_access(c: &mut Criterion) {
    let size = Dimensions {
        width: 4000,
        height: 3000,
    };
    let area = Rect::new(Point::ORIGIN, size);
    let raw = RawImage::new(size, area, 14, CfaPattern::Rggb);

    c.bench_function("pixel_get_4000x3000", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for y in (0..3000).step_by(10) {
                for x in (0..4000).step_by(10) {
                    sum += raw.get_pixel(x, y).unwrap_or(0) as u64;
                }
            }
            sum
        });
    });
}

// ── Standard-codec round-trips (gamut backends) ──────────────────────────────

/// A synthetic 16-bit RGB gradient with per-pixel variation, so codec work is
/// representative (not a flat plane the entropy coder shortcuts).
#[cfg(any(
    all(feature = "jpeg-decode", feature = "jpeg-encode"),
    all(feature = "png-decode", feature = "png-encode"),
))]
fn synthetic_rgb(width: u32, height: u32) -> rawshift_image::core::RgbImage {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let r = (x * 65535 / width.max(1)) as u16;
            let g = (y * 65535 / height.max(1)) as u16;
            let b = (((x ^ y) & 0xFF) as u16) * 257;
            data.extend_from_slice(&[r, g, b]);
        }
    }
    rawshift_image::core::RgbImage::new(width, height, data).expect("valid RGB buffer")
}

#[cfg(all(feature = "jpeg-decode", feature = "jpeg-encode"))]
fn bench_jpeg_codec(c: &mut Criterion) {
    use rawshift_image::core::metadata::ImageMetadata;
    use rawshift_image::formats::export::EncodeOptions;
    use rawshift_image::formats::{StandardFormat, decode_standard_image, encode_rgb_image_to_vec};

    let image = synthetic_rgb(512, 512);
    let metadata = ImageMetadata::default();
    let opts = EncodeOptions::jpeg();

    c.bench_function("jpeg_encode_512x512", |b| {
        b.iter(|| encode_rgb_image_to_vec(&image, &metadata, &opts).expect("encode JPEG"));
    });

    let bytes = encode_rgb_image_to_vec(&image, &metadata, &opts).expect("encode JPEG");
    c.bench_function("jpeg_decode_512x512", |b| {
        b.iter(|| decode_standard_image(&bytes, StandardFormat::Jpeg).expect("decode JPEG"));
    });
}

#[cfg(not(all(feature = "jpeg-decode", feature = "jpeg-encode")))]
fn bench_jpeg_codec(_c: &mut Criterion) {}

#[cfg(all(feature = "png-decode", feature = "png-encode"))]
fn bench_png_codec(c: &mut Criterion) {
    use rawshift_image::core::metadata::ImageMetadata;
    use rawshift_image::formats::export::EncodeOptions;
    use rawshift_image::formats::{StandardFormat, decode_standard_image, encode_rgb_image_to_vec};

    let image = synthetic_rgb(512, 512);
    let metadata = ImageMetadata::default();
    let opts = EncodeOptions::png();

    c.bench_function("png_encode_512x512", |b| {
        b.iter(|| encode_rgb_image_to_vec(&image, &metadata, &opts).expect("encode PNG"));
    });

    let bytes = encode_rgb_image_to_vec(&image, &metadata, &opts).expect("encode PNG");
    c.bench_function("png_decode_512x512", |b| {
        b.iter(|| decode_standard_image(&bytes, StandardFormat::Png).expect("decode PNG"));
    });
}

#[cfg(not(all(feature = "png-decode", feature = "png-encode")))]
fn bench_png_codec(_c: &mut Criterion) {}

// ── Hardware HEIC/AVIF decode (`hw` feature) ─────────────────────────────────

/// Local fixture generation for the hardware benches: a synthetic Y4M pushed
/// through an external encoder CLI (`heif-enc` / `avifenc`). Everything lands
/// in a per-process temp dir; nothing is committed and nothing needs human
/// sourcing.
#[cfg(all(feature = "hw", any(feature = "heic-decode", feature = "avif-decode")))]
mod hw_fixtures {
    use std::path::Path;
    use std::process::Command;

    /// Write an 8-bit 4:2:0 Y4M whose luma varies per pixel, so the encoded
    /// stream carries real content for the decoder to work on.
    fn write_y4m_420(path: &Path, w: usize, h: usize) {
        let mut out = format!("YUV4MPEG2 W{w} H{h} F25:1 Ip A1:1 C420jpeg\nFRAME\n").into_bytes();
        for y in 0..h {
            for x in 0..w {
                out.push(((x + y) & 0xFF) as u8);
            }
        }
        out.extend(std::iter::repeat_n(96u8, (w / 2) * (h / 2)));
        out.extend(std::iter::repeat_n(160u8, (w / 2) * (h / 2)));
        std::fs::write(path, out).expect("write y4m");
    }

    /// Encode a synthetic Y4M with `tool`, returning the container bytes, or
    /// `None` (after eprintln-ing why) when the tool is missing or fails —
    /// the caller skips its benchmark in that case.
    pub fn encode_synthetic(
        tool: &str,
        extra: &[&str],
        ext: &str,
        w: usize,
        h: usize,
    ) -> Option<Vec<u8>> {
        let dir = std::env::temp_dir().join(format!("rawshift_hw_bench_{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let y4m = dir.join(format!("bench_{w}x{h}.y4m"));
        write_y4m_420(&y4m, w, h);
        let out_path = dir.join(format!("bench_{w}x{h}.{ext}"));
        match Command::new(tool)
            .args(extra)
            .arg("-o")
            .arg(&out_path)
            .arg(&y4m)
            .output()
        {
            Ok(out) if out.status.success() => std::fs::read(&out_path).ok(),
            Ok(out) => {
                eprintln!(
                    "Skipping {ext} hardware decode bench: {tool} failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
                None
            }
            Err(_) => {
                eprintln!("Skipping {ext} hardware decode bench: {tool} not installed");
                None
            }
        }
    }
}

#[cfg(all(feature = "hw", feature = "heic-decode"))]
fn bench_heic_hw_decode(c: &mut Criterion) {
    use rawshift_image::formats::{HeicFile, heic_hw_decode_available};

    if !heic_hw_decode_available() {
        eprintln!(
            "Skipping HEIC hardware decode bench: no hardware HEVC decoder usable at runtime"
        );
        return;
    }
    let Some(data) = hw_fixtures::encode_synthetic("heif-enc", &[], "heic", 512, 512) else {
        return;
    };
    let file = match HeicFile::open(data) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("Skipping HEIC hardware decode bench: container rejected: {err}");
            return;
        }
    };
    // One probe decode: drivers can advertise support yet reject the stream —
    // skip instead of panicking mid-benchmark.
    if let Err(err) = file.decode_primary() {
        eprintln!("Skipping HEIC hardware decode bench: probe decode failed: {err}");
        return;
    }
    c.bench_function("heic_hw_decode_primary_512x512", |b| {
        b.iter(|| file.decode_primary().expect("hardware decode HEIC primary"));
    });
}

#[cfg(not(all(feature = "hw", feature = "heic-decode")))]
fn bench_heic_hw_decode(_c: &mut Criterion) {}

#[cfg(all(feature = "hw", feature = "avif-decode"))]
fn bench_avif_hw_decode(c: &mut Criterion) {
    use rawshift_image::formats::{AvifFile, avif_hw_decode_available};

    if !avif_hw_decode_available() {
        eprintln!("Skipping AVIF hardware decode bench: no hardware AV1 decoder usable at runtime");
        return;
    }
    let Some(data) = hw_fixtures::encode_synthetic("avifenc", &["-s", "8"], "avif", 512, 512)
    else {
        return;
    };
    let file = match AvifFile::open(data) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("Skipping AVIF hardware decode bench: container rejected: {err}");
            return;
        }
    };
    if let Err(err) = file.decode_primary() {
        eprintln!("Skipping AVIF hardware decode bench: probe decode failed: {err}");
        return;
    }
    c.bench_function("avif_hw_decode_primary_512x512", |b| {
        b.iter(|| file.decode_primary().expect("hardware decode AVIF primary"));
    });
}

#[cfg(not(all(feature = "hw", feature = "avif-decode")))]
fn bench_avif_hw_decode(_c: &mut Criterion) {}

criterion_group!(
    benches,
    bench_raw_image_creation,
    bench_pixel_access,
    bench_jpeg_codec,
    bench_png_codec,
    bench_heic_hw_decode,
    bench_avif_hw_decode
);
criterion_main!(benches);
