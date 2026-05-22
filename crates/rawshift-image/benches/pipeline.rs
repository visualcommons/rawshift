//! Benchmarks for the processing pipeline stages.

use criterion::{Criterion, criterion_group, criterion_main};
use rawshift_image::core::image::{CfaPattern, Point, RawImage, Rect, RgbImage, Size};
use rawshift_image::processing::color::{apply_color_matrix, apply_white_balance};
use rawshift_image::transforms::black_level::apply_black_level;
use rawshift_image::transforms::color::compute_camera_to_srgb;
use rawshift_image::transforms::tonemap::apply_tone_reproduction;

fn create_test_raw(width: u32, height: u32) -> RawImage {
    let size = Size::new(width, height);
    let area = Rect::new(Point::ORIGIN, size);
    let pixel_count = (width * height) as usize;
    let data = vec![5000u16; pixel_count];

    RawImage::builder(size, area, 14, CfaPattern::Rggb)
        .black_levels([512; 4])
        .white_level(16383)
        .data(data)
        .build()
}

fn create_test_rgb(width: u32, height: u32) -> RgbImage {
    let data = vec![5000u16; (width * height * 3) as usize];
    RgbImage::new(width, height, data)
}

fn bench_black_level(c: &mut Criterion) {
    let mut group = c.benchmark_group("black_level");

    for &(w, h) in &[(1000, 1000), (4000, 3000)] {
        let mut raw = create_test_raw(w, h);
        group.bench_function(format!("{}x{}", w, h), |b| {
            b.iter(|| {
                apply_black_level(&mut raw);
            });
        });
    }

    group.finish();
}

fn bench_white_balance(c: &mut Criterion) {
    let mut group = c.benchmark_group("white_balance");

    for &(w, h) in &[(1000, 1000), (4000, 3000)] {
        let mut rgb = create_test_rgb(w, h);
        let wb = (1.5f32, 1.0, 1.8);
        group.bench_function(format!("{}x{}", w, h), |b| {
            b.iter(|| {
                apply_white_balance(&mut rgb, wb);
            });
        });
    }

    group.finish();
}

fn bench_color_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group("color_matrix");

    // Sony ILCE-7RM5 ColorMatrix2
    let cm = [
        0.8200, -0.2976, -0.0719, -0.4296, 1.2053, 0.2532, -0.0429, 0.1282, 0.5774,
    ];
    let matrix = compute_camera_to_srgb(&cm).unwrap();

    for &(w, h) in &[(1000, 1000), (4000, 3000)] {
        let mut rgb = create_test_rgb(w, h);
        group.bench_function(format!("{}x{}", w, h), |b| {
            b.iter(|| {
                apply_color_matrix(&mut rgb, &matrix);
            });
        });
    }

    group.finish();
}

fn bench_tone_mapping(c: &mut Criterion) {
    let mut group = c.benchmark_group("tone_mapping");

    for &(w, h) in &[(1000, 1000), (4000, 3000)] {
        let mut rgb = create_test_rgb(w, h);
        group.bench_function(format!("{}x{}", w, h), |b| {
            b.iter(|| {
                apply_tone_reproduction(&mut rgb, None);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_black_level,
    bench_white_balance,
    bench_color_matrix,
    bench_tone_mapping
);
criterion_main!(benches);
