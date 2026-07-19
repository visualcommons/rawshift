# Test Data & Fixtures

This document explains how rawshift's image-driven integration tests work,
how to get test data, and how to add new test images.

## Overview

Real RAW test images and their reference sidecar files live in a **separate repo**:

> **[justin13888/rawshift-test-fixtures](https://github.com/justin13888/rawshift-test-fixtures)**

Each camera device (Make/Model) is independently versioned and released as a
tarball via GitHub Releases. This repo pins specific device versions in
`fixtures.json` and fetches only the devices it needs.

Standard-format fixtures (JPEG, PNG, GIF, TIFF, WebP, SVG, AVIF, JXL) are
generated locally by `examples/generate_test_fixtures.rs` with full EXIF, ICC,
and XMP metadata embedded.

---

## Getting Test Data (Contributors)

```bash
# Download + extract all pinned device fixtures
just fetch-fixtures

# Also generate the tiny synthetic standard-format images
just setup-test-data     # fetch-fixtures + generate-fixtures

# Fetch a specific device only
just fetch-fixtures sony-ilce-6700
```

Or manually:
```bash
bash scripts/fetch_test_fixtures.sh
cargo run --example generate_test_fixtures
```

The download script is **idempotent** — re-running it is a no-op if you
already have the correct version for each device.

---

## Directory Structure

### `test_data/` (local, gitignored)

```
test_data/
├── <Make>/                         # RAW formats (from fixtures repo)
│   └── <Model>/
│       └── <Filename>.<Ext>
├── standard/                       # Standard formats (synthetic)
│   ├── jpeg/test_8x8.jpg
│   ├── png/test_8x8.png
│   ├── gif/test_4x4.gif
│   ├── tiff/test_8x8.tiff
│   ├── webp/test_8x8.webp
│   ├── svg/test_8x8.svg
│   ├── avif/test_8x8.avif         # (with avif-encode feature)
│   └── jxl/test_8x8.jxl           # (with jxl-encode feature)
└── .device-versions/               # Per-device version stamps (written by fetch script)
    ├── sony-ilce-6700              # contains "1"
    └── apple-iphone-17-pro-max     # contains "1"
```

### `test_fixtures/` (local, gitignored)

```
test_fixtures/
├── <Make>/                         # RAW format fixtures
│   └── <Model>/
│       └── <Filename>/
│           ├── expected.json         # Source-of-truth for Rust tests
│           ├── exiftool.json         # Full `exiftool -j -g -struct` output
│           ├── file_identify.txt     # `file --mime-type --mime-encoding` output
│           ├── libraw_identify.txt   # `raw-identify -v` output (if available)
│           └── dcraw_identify.txt    # `dcraw -i -v` output (if available)
└── standard/
    └── <format>/
        └── expected.json             # Ground truth (dimensions, channels, metadata)
```

### `fixtures.json` (committed, project root)

Pins which device versions to fetch:
```json
{
  "repo": "justin13888/rawshift-test-fixtures",
  "devices": {
    "sony-ilce-6700": { "version": 1, "make": "SONY", "model": "ILCE-6700" },
    "apple-iphone-17-pro-max": { "version": 1, "make": "Apple", "model": "iPhone_17_Pro_Max" }
  }
}
```

### Standard fixture `expected.json` format

Formats with metadata (JPEG, PNG, WebP, AVIF) include a `metadata` block:
```json
{
  "format": "JPEG",
  "file_name": "test_8x8.jpg",
  "width": 8, "height": 8,
  "channels": 3, "bit_depth_output": 16,
  "metadata": {
    "make": "rawshift-test",
    "model": "Synthetic-v1",
    "iso": 200,
    "focal_length_num": 50,
    "datetime_original": "2025:01:15 10:30:00",
    "has_icc": true,
    "has_xmp": true
  }
}
```

Formats without metadata support (GIF, SVG, TIFF) omit the `metadata` block.

---

## Test Coverage

See which decoders have test data and which test aspects are covered:

```bash
just coverage-report
# or:
python3 scripts/test_coverage_report.py
```

---

## Standard Formats

| Format | Status   | Metadata Embedded                   |
|--------|----------|-------------------------------------|
| JPEG   | Complete | EXIF + ICC + XMP                    |
| PNG    | Complete | EXIF + ICC + XMP                    |
| WebP   | Complete | EXIF + ICC + XMP                    |
| GIF    | Complete | None (format limitation)            |
| TIFF   | Complete | None (tiff crate encoder, no embed) |
| SVG    | Complete | None (not raster metadata)          |
| AVIF   | Feature  | EXIF + ICC + XMP (`avif-encode`)    |
| JXL    | Feature  | EXIF + ICC + XMP (`jxl-encode`)     |
| HEIC   | Feature  | Read-only EXIF/ICC/XMP (`heic`)     |
| APV    | N/A      | Detection only (no decode)          |

---

## Adding New RAW Test Images

See the [rawshift-test-fixtures README](https://github.com/justin13888/rawshift-test-fixtures)
for the full workflow. Summary:

1. Source a CC0-licensed sample from **raw.pixls.us** or **DPReview**
2. In the fixtures repo: `bash ingest.sh /path/to/file.ARW`
3. Review `expected.json`, commit, then `bash pack.sh <device-slug>`
4. In this repo: add the device to `fixtures.json` with its version
5. Run `bash scripts/fetch_test_fixtures.sh <device-slug>` to verify

---

## Running Fixture Tests

```bash
# Full test suite (fetches fixtures + generates standard ones first)
just test-fixtures

# Individual test files
cargo test --features=experimental --test raw_decode_fixtures
cargo test --test standard_decode_fixtures
cargo test --features=tiff-parser --test tiff_parser_tests
cargo test --features=tiff-parser --test dng_check

# With specific features
cargo test --features=full
```

Tests skip gracefully when fixture files are missing — `cargo test` always
passes even without test data.

---

## TODO

### Infrastructure
- [ ] End-to-end validation: pack → upload → fetch → test cycle
- [ ] Windows CI compatibility — `fetch_test_fixtures.sh` uses bash; needs
      validation on windows-latest runner (git-bash)

### Test data gaps
- [ ] RAW formats: CR2, CR3, CRW, NEF, RAF — need sample images sourced and
      added to rawshift-test-fixtures repo
- [ ] JXL fixtures — generator implemented but gated behind `jxl-encode` feature;
      needs `expected.json` and decode test
- [ ] TIFF metadata — library reads EXIF from TIFF but no metadata is embedded
      in the TIFF test fixture (tiff crate encoder doesn't use encode_rgb_image path)
- [ ] AVIF fixtures — generator implemented but gated behind `avif-encode` feature;
      needs `expected.json` and decode test

### Metadata coverage gaps
- [ ] IPTC metadata — not implemented in rawshift at all
- [ ] ICC profile round-trip — library embeds sRGB profiles but no test reads
      them back and verifies the profile data
- [ ] XMP round-trip — library embeds raw XMP bytes but no test reads them back
- [ ] Custom ICC profiles — only hardcoded sRGB; no custom profile reading
- [ ] Metadata loss detection — no tests verify what metadata is lost per format
