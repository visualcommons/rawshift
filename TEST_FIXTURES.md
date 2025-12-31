# Test Data & Fixtures

This directory structure organizes camera raw files and their corresponding reference metadata.

## Directory Structure

Files are organized by Camera Make and Model.

### `test_data/`

Contains the raw image files.

```plaintext
test_data/
└── <Make>/
    └── <Model>/
        └── <Filename>.<Ext>
```

Example: `test_data/SONY/ILCE-6700/_JIC7790.ARW`

### `test_fixtures/`

Contains reference data (sidecar files) for verification.

```plaintext
test_fixtures/
└── <Make>/
    └── <Model>/
        └── <Filename>/
            ├── expected.json         # The primary source-of-truth JSON for unit tests
            ├── exiftool.json         # Full output from `exiftool -j -g -struct`
            ├── libraw_identify.txt   # Output from `raw-identify -v`
            └── dcraw_identify.txt    # Output from `dcraw -i -v`
```

## Adding New Test Images

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
