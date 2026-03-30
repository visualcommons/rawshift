# Default recipe — show available commands
default:
    @just --list

# Format all code
fmt:
    cargo fmt --all

# Check formatting (CI mode)
fmt-check:
    cargo fmt --all -- --check

# Run clippy lints
clippy:
    cargo clippy --all-targets -- -D warnings

# Auto-fix clippy lints
clippy-fix:
    cargo clippy --fix --allow-dirty --allow-staged --all-targets -- -D warnings

# Build (default features)
build:
    cargo build

# Build with specific features
build-features features:
    cargo build --no-default-features --features "{{features}}"

# Run tests (default features)
test:
    cargo test

# Run tests with specific features
test-features features:
    cargo test --no-default-features --features "{{features}}"

# Run all tests with all features
test-all:
    cargo test --all-features

# Generate docs
doc:
    cargo doc --no-deps --open

# Check docs build (no open)
doc-check:
    cargo doc --no-deps --all-features

# Run doc tests
doc-test:
    cargo test --doc --all-features

# Pre-publish checks
publish-check:
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all-features
    cargo doc --no-deps
    cargo publish --dry-run

# Install git hooks
setup:
    lefthook install
    cargo build

# Download test fixtures from rawshift-test-fixtures GitHub Releases
fetch-fixtures *args:
    bash scripts/fetch_test_fixtures.sh {{args}}

# Generate standard format test fixtures (synthetic images)
generate-fixtures:
    cargo run --example generate_test_fixtures

# Full test data setup: download real fixtures + generate synthetic ones
setup-test-data: fetch-fixtures generate-fixtures

# Show decoder test coverage report
coverage-report:
    python3 scripts/test_coverage_report.py

# Run all fixture-based integration tests (fetches fixtures first)
test-fixtures: setup-test-data
    cargo test --features=full --test raw_decode_fixtures --test standard_decode_fixtures --test tiff_parser_tests --test dng_check

