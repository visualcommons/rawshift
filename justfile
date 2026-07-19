# Default recipe — show available commands
default:
    @just --list

# Format all code
fmt:
    cargo fmt --all

# Check formatting (CI mode)
fmt-check:
    cargo fmt --all -- --check

# Run clippy lints across the whole workspace
clippy:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p rawshift-image --all-targets --features full -- -D warnings

# Auto-fix clippy lints across the whole workspace
clippy-fix:
    cargo clippy --fix --allow-dirty --allow-staged --workspace --all-targets -- -D warnings

# Build the whole workspace (default features)
build:
    cargo build --workspace

# Build rawshift-image with a specific feature set (e.g. `just build-features "gif,png"`)
build-features features:
    cargo build -p rawshift-image --no-default-features --features "{{features}}"

# Build the image-only facade — verifies the image half compiles standalone
build-image:
    cargo build -p rawshift --no-default-features --features image

# Build the parked video crate directly — the facade has no `video` feature
# while rawshift-video is parked (see crates/rawshift/Cargo.toml)
build-video:
    cargo build -p rawshift-video --all-features

# Run tests for the whole workspace (default features) — fetches fixtures first
test: setup-test-data
    cargo test --workspace

# Run rawshift-image tests with a specific feature set
test-features features:
    cargo test -p rawshift-image --no-default-features --features "{{features}}"

# Run all workspace tests with the full feature set. NOT --all-features: the
# explicit hw backend pins (hw-videotoolbox / hw-vaapi / hw-mediacodec) are
# mutually exclusive verified flags — enabling them together compile_error!s
# on every target by design.
test-all:
    cargo test --workspace --features rawshift-image/full

# Generate docs for the whole workspace
doc:
    cargo doc --workspace --no-deps --open

# Check docs build (no open). `rawshift-image/full`, not --all-features: the
# hw backend pins are mutually exclusive by design (compile_error!).
doc-check:
    cargo doc --workspace --no-deps --features rawshift-image/full

# Run doc tests
doc-test:
    cargo test --workspace --doc --features rawshift-image/full

# Pre-publish checks (`rawshift-image/full`, not --all-features: the hw
# backend pins are mutually exclusive by design)
publish-check:
    cargo clippy --workspace --all-targets --features rawshift-image/full -- -D warnings
    cargo test --workspace --features rawshift-image/full
    cargo doc --workspace --no-deps
    cargo publish --dry-run -p rawshift-core
    cargo publish --dry-run -p rawshift-image
    cargo publish --dry-run -p rawshift-video

# Install git hooks
setup:
    lefthook install
    cargo build --workspace

# Download test fixtures from rawshift-test-fixtures GitHub Releases
fetch-fixtures *args:
    bash scripts/fetch_test_fixtures.sh {{args}}

# Generate standard format test fixtures (synthetic images)
generate-fixtures:
    cargo run -p rawshift-image --example generate_test_fixtures

# Full test data setup: download real fixtures + generate synthetic ones
setup-test-data: fetch-fixtures generate-fixtures

# Show decoder test coverage report
coverage-report:
    python3 scripts/test_coverage_report.py

# Run all fixture-based integration tests (fetches fixtures first)
test-fixtures: setup-test-data
    cargo test -p rawshift-image --features=full --test raw_decode_fixtures --test standard_decode_fixtures --test dng_check
