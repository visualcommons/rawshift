# Benchmark Baselines

Criterion benches live in `crates/rawshift-image/benches/` (`decode`,
`demosaic`, `pipeline`). Run them with:

```bash
# All three criterion benches (add --features full for hw HEIC/AVIF benches)
cargo bench -p rawshift-image --features full --bench decode --bench demosaic --bench pipeline

# Fast pass (what produced the table below)
cargo bench -p rawshift-image --features full --bench decode --bench demosaic --bench pipeline -- --quick
```

The hardware HEIC/AVIF benches (`decode` bench, `hw` feature) generate their
fixtures locally with `heif-enc` / `avifenc` and skip cleanly when either tool
is missing or no hardware decoder is usable at runtime.

## Baseline provenance (gamut migration, issue #35)

Pre-migration baselines were **not captured** before the gamut migration epic
(#38) started, so no repo-wide before/after comparison exists. Per-codec
before/after numbers were recorded in the individual migration PRs where they
were measured (PNG: #47, JPEG: #52). The table below is the **post-migration
baseline** — the reference point for future regressions — captured on
2026-07-18 at commit `217c596` (post `chore(features,ci)`), `--quick` mode.

Machine: AMD Ryzen 7 7800X3D (16 threads), Radeon RX 7900 (VAAPI/radeonsi),
Linux (Fedora 44). `--quick` numbers are indicative, not tightly converged —
re-measure with a full `cargo bench` run before acting on small deltas.

## Post-migration baseline (2026-07-18)

### decode (`--features full`)

| Benchmark | Time (mid estimate) |
|---|---|
| raw_image_creation/1000x1000 | 16.6 µs |
| raw_image_creation/4000x3000 | 192.8 µs |
| raw_image_creation/8000x6000 | 5.7 µs¹ |
| pixel_get_4000x3000 | 47.3 µs |
| jpeg_encode_512x512 (gamut-jpeg) | 6.48 ms |
| jpeg_decode_512x512 (gamut-jpeg) | 6.04 ms |
| png_encode_512x512 (gamut-png) | 23.0 ms |
| png_decode_512x512 (gamut-png) | 4.72 ms |
| heic_hw_decode_primary_512x512 (gamut-heic + VAAPI HEVC) | 9.20 ms |
| avif_hw_decode_primary_512x512 (gamut-avif + VAAPI AV1) | 9.48 ms |

¹ Lazy zero-page allocation artifact at this size; not a real throughput
number.

### demosaic

| Benchmark | Time (mid estimate) |
|---|---|
| demosaic_bilinear/100x100 | 22.3 µs |
| demosaic_bilinear/500x500 | 261.9 µs |
| demosaic_bilinear/1000x1000 | 917.6 µs |
| demosaic_bilinear/2000x2000 | 3.45 ms |
| demosaic_amaze/100x100 | 393.4 µs |
| demosaic_amaze/500x500 | 9.06 ms |
| demosaic_amaze/1000x1000 | 36.7 ms |
| demosaic_amaze/2000x2000 | 199.5 ms |

### pipeline

| Benchmark | Time (mid estimate) |
|---|---|
| black_level/1000x1000 | 1.28 ms |
| black_level/4000x3000 | 15.8 ms |
| white_balance/1000x1000 | 2.77 ms |
| white_balance/4000x3000 | 34.0 ms |
| color_matrix/1000x1000 | 3.06 ms |
| color_matrix/4000x3000 | 35.3 ms |
| tone_mapping/1000x1000 | 1.15 ms |
| tone_mapping/4000x3000 | 9.39 ms |

## Updating this baseline

Gamut pin bumps require a full test + benchmark run (AGENTS.md). When a run
shows a deliberate, explained shift (new backend, algorithm change), refresh
the affected rows here in the same PR and note the change in CHANGELOG.md.
