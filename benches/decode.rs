//! Benchmarks for RAW image data structure creation and basic operations.

use criterion::{Criterion, criterion_group, criterion_main};
use rawshift::core::image::{CfaPattern, Point, RawImage, Rect, Size};

/// Benchmark creating a RawImage (allocation + init).
fn bench_raw_image_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("raw_image_creation");

    for &(w, h) in &[(1000, 1000), (4000, 3000), (8000, 6000)] {
        group.bench_function(format!("{}x{}", w, h), |b| {
            b.iter(|| {
                let size = Size::new(w, h);
                let area = Rect::new(Point::ORIGIN, size);
                RawImage::new(size, area, 14, CfaPattern::Rggb)
            });
        });
    }

    group.finish();
}

/// Benchmark pixel access patterns.
fn bench_pixel_access(c: &mut Criterion) {
    let size = Size::new(4000, 3000);
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

criterion_group!(benches, bench_raw_image_creation, bench_pixel_access);
criterion_main!(benches);
