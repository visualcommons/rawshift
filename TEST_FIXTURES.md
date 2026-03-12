# Test Data & Fixtures

This directory structure organizes camera raw files and their corresponding reference metadata.

## Directory Structure

Files are organized by Camera Make and Model (RAW formats) or by format name (standard formats).

### `test_data/`

Contains the image files used for integration testing.

```plaintext
test_data/
├── <Make>/                         # RAW formats (by camera)
│   └── <Model>/
│       └── <Filename>.<Ext>
└── standard/                       # Standard formats (synthetic)
    ├── jpeg/
    │   └── test_8x8.jpg
    ├── png/
    │   └── test_8x8.png
    ├── gif/
    │   └── test_4x4.gif
    ├── tiff/
    │   └── test_8x8.tiff
    ├── webp/
    │   └── test_8x8.webp
    └── svg/
        └── test_8x8.svg
```

### `test_fixtures/`

Contains reference data (sidecar files) for verification.

```plaintext
test_fixtures/
├── <Make>/                         # RAW format fixtures
│   └── <Model>/
│       └── <Filename>/
│           ├── expected.json         # Primary source-of-truth for unit tests
│           ├── exiftool.json         # Full output from `exiftool -j -g -struct`
│           ├── libraw_identify.txt   # Output from `raw-identify -v`
│           └── dcraw_identify.txt    # Output from `dcraw -i -v`
└── standard/                       # Standard format fixtures
    └── <format>/
        └── expected.json             # Ground truth (dimensions, channels, etc.)
```

## Fixture Status by Format

### RAW Formats

| Format | Status | Test Data | Fixtures | Source |
|--------|--------|-----------|----------|--------|
| Sony ARW | Complete | `SONY/ILCE-6700/_JIC7790.ARW`, `_JIC7792.ARW` | Full (expected.json + reference tools) | Camera sample |
| Adobe DNG | Complete | `Apple/iPhone_17_Pro_Max/IMG_1347.DNG` | Full (expected.json + reference tools) | Camera sample |
| Canon CR2 | Needs sourcing | None | None | See sourcing guide below |
| Canon CR3 | Needs sourcing | None | None | See sourcing guide below |
| Canon CRW | Needs sourcing | None | None | See sourcing guide below |
| Nikon NEF | Needs sourcing | None | None | See sourcing guide below |
| Fujifilm RAF | Needs sourcing | None | None | See sourcing guide below |

### Standard Formats

| Format | Status | Decode | Export | Fixture Type |
|--------|--------|--------|--------|--------------|
| JPEG | Complete | Full | Full | Synthetic (generate_test_fixtures) |
| PNG | Complete | Full | Full | Synthetic (generate_test_fixtures) |
| GIF | Complete | Full | N/A | Synthetic (generate_test_fixtures) |
| TIFF | Complete | Full | N/A | Synthetic (generate_test_fixtures) |
| WebP | Complete | Full | Full | Synthetic (generate_test_fixtures) |
| SVG | Complete (feature-gated) | Full | N/A | Synthetic (generate_test_fixtures) |
| JPEG XL | Unit tests only | Full | Optional | In-memory roundtrip tests exist |
| AVIF | Detection only | Stub | Optional | Format detection unit tests exist |
| HEIC | Detection only | Stub | N/A | Format detection unit tests exist |
| APV | Detection only | Stub | N/A | Format detection unit tests exist |

## Generating Standard Format Fixtures

Standard format fixtures are small synthetic images generated programmatically:

```bash
cargo run --example generate_test_fixtures
```

This creates test images in `test_data/standard/` and ground-truth JSON in `test_fixtures/standard/`.

## Adding New RAW Test Images

1. Place new raw files into `test_data/` (root or any subfolder).
2. Run the organization script:

```bash
python3 scripts/organize_test_data.py
```

This script will:

- Detect the Make/Model of the new files.
- Move them to the correct `test_data/<Make>/<Model>/` folder.
- Generate the `test_fixtures` folder structure.
- Run `exiftool`, `raw-identify`, and `dcraw` to populate the sidecar files.

3. Manually create `expected.json` with ground truth values (see existing examples).

## Sourcing RAW Test Files

RAW test files must be sourced from actual cameras or public sample repositories.

### Recommended Sources

- **raw.pixls.us**: Community collection of CC0-licensed raw samples from many cameras
- **DPReview sample galleries**: Full-resolution samples from camera reviews
- **Camera manufacturer sample images**: Some manufacturers provide sample RAW files
- **Personal camera samples**: If you own the camera

### What to Look For

For each missing format, source **one representative file** from a common camera model:

| Format | Suggested Camera | File Extension |
|--------|-----------------|----------------|
| Canon CR2 | Canon EOS 5D Mark IV, 6D Mark II, or similar | `.CR2` |
| Canon CR3 | Canon EOS R5, R6, or similar | `.CR3` |
| Canon CRW | Canon PowerShot G3/G5 or similar (legacy) | `.CRW` |
| Nikon NEF | Nikon D850, Z6/Z7, or similar | `.NEF` |
| Fujifilm RAF | Fujifilm X-T5, X-H2, or similar (X-Trans sensor) | `.RAF` |

### After Sourcing

1. Place the file in `test_data/` (root is fine)
2. Run `python3 scripts/organize_test_data.py`
3. Create `expected.json` based on exiftool output and the GroundTruth struct in `tests/common/mod.rs`
4. Run tests: `cargo test --test raw_decode_fixtures`

## Running Fixture Tests

```bash
# All fixture-based tests (skips gracefully if files missing)
cargo test --test standard_decode_fixtures
cargo test --test raw_decode_fixtures
cargo test --test tiff_parser_tests
cargo test --test dng_check

# Generate standard fixtures first, then test
cargo run --example generate_test_fixtures && cargo test --test standard_decode_fixtures
```
