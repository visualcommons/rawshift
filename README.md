# rawshift

> Project status: Alpha as of May 31, 2026. APIs are actively being stabilized.

Raw image processing library focused on compatibility, correctness, and interoperability.

## Why rawshift?

Image processing is messy and no single library could do it all (e.g., accurate colour, complete metadata, consistent format support). Rawshift seeks to stand out in at least some of the following ways:

- Compatibility: A single library processes all forms of image formats including all popular compressed and RAW image formats used by both consumers and creative professionals.
- Correctness: While decoding implementations should remain flexible (e.g. to slightly non-conformant image), it should strictly conform to open standards and retain maximum metadata across formats.
- Interoperability: This library compiles and is optimized for several standard desktop and mobile platforms. This is possible because the majority of the library is written in pure Rust and non-Rust dependencies are encapsulated with best practices.

## Scope

rawshift is both a wrapper for image and video encoding/decoding needs, as well as implementations for various (often proprietary) RAW formats validated against specific camera bodies.

A number of formats (e.g. HEIF, AV1) still depend on C/C++ libraries that are much more mature and battle-tested. The goal is to eventually support portable Rust equivalents that have the same features and performance characteristics of the benchmark implementations.

While performance remains a core pillar in development, the primary priority remains to be feature-parity and output quality.

<!-- TODO: Add in-repo benchmark comparing our current outputs to other libraries -->

## Getting Started

> This library is still in active development. See `master` branch for latest improvements (noting potential instability). Alpha packages may be published to crates.io occasionally.

<!-- TODO: Add docs on the specific features, etc. on the docs.rs page -->

## Format Support

Rawshift targets both still image and video formats. Image decoding is the
current focus; video support is planned but not yet implemented (see
[Video](#video)).

> Features and performance are constantly improving. Most functionality is
> implemented from scratch to meet project goals, so expect progressive
> format-support improvements over time. We aim to be liberal in what we accept
> (decode) and strict in what we give (encode).

### Images

Decode support spans the common compressed formats (JPEG, PNG, WebP, JXL, AVIF,
HEIC, GIF, TIFF, SVG, PPM) plus a growing set of RAW formats, prioritised as:

- Sony ARW (all variations at least up to v5.0.1)
- Adobe DNG (up to v1.7, including what is necessary for Apple ProRAW)
- Standard TIFF
- Canon CR3
- Canon CR2

The full per-format decode/encode support table — backend libraries, encode
availability, and maturity status, plus notes on encoding and default
implementations — lives in the
[`rawshift-image` README](./crates/rawshift-image/README.md#format-support).

### Video

Video support is **planned and not yet implemented** — no video code ships
today, and there are no video feature flags. The roadmap of formats
(XAVC HS/S, Apple ProRes, HEVC, H.264) and the container-first implementation
plan live in the [`rawshift-video` README](./crates/rawshift-video/README.md).

## Crates

rawshift is a Cargo workspace:

| Crate            | Purpose                                                                                                          |
| ---------------- | ---------------------------------------------------------------------------------------------------------------- |
| [`rawshift`](./crates/rawshift)                       | Facade. Re-exports the libraries below behind coarse `image` / `video` features. Most consumers depend on this.  |
| [`rawshift-image`](./crates/rawshift-image/README.md) | Still-image decoding, RAW processing, and encoding. Carries the full per-format feature system.                  |
| [`rawshift-video`](./crates/rawshift-video/README.md) | Video support — planned, not yet implemented (see [Video](#video)).                                              |
| [`rawshift-core`](./crates/rawshift-core/README.md)   | Shared types — geometry, pixel samples, the metadata model — used by both libraries.                             |

## Feature Flags

### Facade — `rawshift`

The `rawshift` facade deliberately exposes only four coarse features:

- `image` *(default)* — still-image support (`rawshift-image` with its own default formats).
- `video` — video support (`rawshift-video`).
- `serde` — `Serialize`/`Deserialize` for metadata and option types.
- `full` — every image format, all video formats, and `serde`.

The facade does **not** re-export per-format flags. Cargo cannot forward a child
crate's features, so re-listing them would be duplicated, rot-prone state — and
a build that wants only video should never have to reason about image flags. For
fine-grained control (individual formats, alternative codec backends, the
`tiff-parser` API, `heic-vendored` linking) depend on `rawshift-image` directly.

### Per-crate feature systems

The image and video libraries each carry their own feature systems — restating
them here would duplicate state that drifts. See each crate's README:

- [`rawshift-image` feature flags](./crates/rawshift-image/README.md#feature-flags)
  — a five-tier per-format system (bundles → formats → directions →
  implementations → infrastructure) with selectable codec backends.
- [`rawshift-video` feature flags](./crates/rawshift-video/README.md#feature-flags)
  — mirrors the image tiers; currently gates no code (video is unimplemented).

## Official supported device list

*A device is officially supported if we have thoroughly tested compatibility for it.*

> Compatibility is verified against the **default decoder implementation** for each format (the named library for that format in the [`rawshift-image` support table](./crates/rawshift-image/README.md#format-support)). Non-default implementations selected via implementation feature flags are not covered by this list.

> The **Image Formats** column lists formats with verified decode support. The **Video Formats** column lists formats produced by the device that are on the [video roadmap](#video) — these are not yet implemented or verified.

| Device                 | Image Formats   | Video Formats         | Notes |
| ---------------------- | --------------- | --------------------- | ----- |
| Sony A7RV (ILCE-7RM5)  | ARW, JPEG, HEIC | XAVC HS, XAVC S       |       |
| Sony A7IV (ILCE-7M4)   | ARW, JPEG, HEIC | XAVC HS, XAVC S       |       |
| Sony a6700 (ILCE-6700) | ARW, JPEG, HEIC | XAVC HS, XAVC S       |       |
| iPhone 13 Pro (Max)    | DNG, HEIC, JPEG | HEVC, H.264, ProRes   |       |
| iPhone 16 Pro (Max)    | DNG, HEIC, JXL  | HEVC, H.264, ProRes   |       |

## MSRV

The minimum supported Rust version (MSRV) is **1.90.0**. This may be bumped as new language features stabilize.

## Development

It is important that development velocity is maintained regardless of project complexity. Unit tests for all contributions are expected, especially for platform-specific behaviours!

### Setup

Install [lefthook](https://github.com/evilmartians/lefthook) and activate the pre-commit and pre-push hooks:

```sh
# macOS / Homebrew
brew install lefthook

# Linux (Homebrew on Linux)
brew install lefthook

# via cargo
cargo install lefthook

# then install the hooks
lefthook install
```

The pre-commit hook runs `cargo fmt --check` and `cargo clippy`.
The pre-push hook runs the full test suite.

### Testing

```sh
# whole workspace, default features
cargo test --workspace

# everything, all image formats
just test-all
```

Fixture-based integration tests need test data — `just setup-test-data` fetches
real fixtures and generates synthetic ones. See the `justfile` for the full set
of recipes (`just build-image`, `just build-video`, `just test-features`, …).

## Sovereignty

It is my intention (as developer and maintainer) to ensure rawshift remains open permissively to all.

## License

While many open-source implementations historically used LGPL or similar licenses, rawshift prefers a more permissive license (MPL-2.0; see [LICENSE](./LICENSE)). You are free to link to any software although we welcome contributions in any way.
