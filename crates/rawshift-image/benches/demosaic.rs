//! Benchmarks for demosaicing algorithms.

use criterion::{Criterion, criterion_group, criterion_main};
use rawshift_image::core::image::{CfaPattern, Point, RawImage, Rect, Size};
use rawshift_image::processing::demosaic::{Bilinear, Demosaic, bayer::Amaze};

fn create_test_raw(width: u32, height: u32) -> RawImage {
    let size = Size::new(width, height);
    let area = Rect::new(Point::ORIGIN, size);
    let pixel_count = (width * height) as usize;
    let mut data = vec![0u16; pixel_count];

    // Fill with a simple gradient pattern
    for y in 0..height {
        for x in 0..width {
            let val = ((x as f32 / width as f32 + y as f32 / height as f32) * 0.5 * 8000.0) as u16;
            data[(y * width + x) as usize] = val;
        }
    }

    RawImage::builder(size, area, 14, CfaPattern::Rggb)
        .white_level(16383)
        .data(data)
        .build()
}

fn bench_bilinear(c: &mut Criterion) {
    let mut group = c.benchmark_group("demosaic_bilinear");

    for &(w, h) in &[(100, 100), (500, 500), (1000, 1000), (2000, 2000)] {
        let raw = create_test_raw(w, h);
        group.bench_function(format!("{}x{}", w, h), |b| {
            b.iter(|| Bilinear.demosaic(&raw));
        });
    }

    group.finish();
}

fn bench_amaze(c: &mut Criterion) {
    let mut group = c.benchmark_group("demosaic_amaze");

    for &(w, h) in &[(100, 100), (500, 500), (1000, 1000), (2000, 2000)] {
        let raw = create_test_raw(w, h);
        group.bench_function(format!("{}x{}", w, h), |b| {
            b.iter(|| Amaze.demosaic(&raw));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_bilinear, bench_amaze);
criterion_main!(benches);
