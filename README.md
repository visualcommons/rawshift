# rawshift

> Project status: Alpha as of May 31, 2026. APIs are actively being stabilized.

Raw image processing library focused on compatibility, correctness, and interoperability.

## Why rawshift?

Image processing is messy and no single library could do it all (e.g., accurate colour, complete metadata, consistent format support). Rawshift seeks to stand out in at least some of the following ways:

- Compatibility: A single library processes all forms of image formats including all popular compressed and RAW image formats used by both consumers and creative professionals.
- Correctness: While decoding implementations should remain flexible (e.g. to slightly non-conformant image), it should strictly conform to open standards and retain maximum metadata across formats.
- Interoperability: This library compiles and is optimized for several standard desktop and mobile platforms. This is possible because the majority of the library is written in pure Rust and non-Rust dependencies are encapsulated with best practices.

## Scope and Priorities

rawshift is both a wrapper for image and video encoding/decoding needs, as well as implementations for various (often proprietary) RAW formats validated against specific camera bodies.

The key priorities in order are:
- **Porting and stabilizing capabilities into Rust:** A number of formats (e.g. HEIF, AV1) still depend on C/C++ libraries that are much more mature and battle-tested. It is unreasonable to port them in the short term and it is equally important to actively avoid implementations that are vibe-coded or that have dubious code licensing. The goal is to eventually support portable Rust equivalents that have the same features and performance characteristics of the benchmark implementations.
- **Output Quality:** Porting to Rust expands our test coverage and allows us to contribute to dependent libraries for feature parity. We validate these improvements by testing decoding against our camera database and encoding against our user base.
- **Performance:** Given mature and feature-complete libraries, we optimize the cost of common operations for various tasks end-to-end (e.g. format transcoding, metadata modification)

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

Video is **parked for v1** — rawshift v1 ships image only. No video code ships
today, `rawshift-video` is unpublished (`publish = false`), and it is not a
dependency of the `rawshift` facade, so there is no `video` feature to enable.
The roadmap of formats (XAVC HS/S, Apple ProRes, HEVC, H.264) and the
container-first implementation plan live in the
[`rawshift-video` README](./crates/rawshift-video/README.md); the crate is
re-added to the publish set and the facade when it has an implementation.

## Crates

rawshift is a Cargo workspace:

| Crate            | Purpose                                                                                                          |
| ---------------- | ---------------------------------------------------------------------------------------------------------------- |
| [`rawshift`](./crates/rawshift)                       | Facade. Re-exports `rawshift-image` behind the coarse `image` feature. Most consumers depend on this.            |
| [`rawshift-image`](./crates/rawshift-image/README.md) | Still-image decoding, RAW processing, and encoding. Carries the full per-format feature system.                  |
| [`rawshift-video`](./crates/rawshift-video/README.md) | Video support — parked and unpublished for v1 (see [Video](#video)).                                             |
| [`rawshift-core`](./crates/rawshift-core/README.md)   | Shared types — geometry, codec descriptors, the metadata model. Charter is documented on the crate.              |

## Feature Flags

### Facade — `rawshift`

The `rawshift` facade deliberately exposes only coarse features:

- `image` *(default)* — still-image support (`rawshift-image` with its own default formats).
- `serde` — `Serialize`/`Deserialize` for metadata and option types.
- `hw`, `hw-videotoolbox`, `hw-vaapi`, `hw-mediacodec` — hardware still-frame
  decode (HEVC for HEIC, AV1 for AVIF) via `rawshift-hwdec`; `hw` picks the
  native backend for the compile target, the `hw-*` flags pin one explicitly
  and fail the compile elsewhere (see [docs/SUPPORT.md](./docs/SUPPORT.md)).
- `full` — every image format, `serde`, and `hw`.

There is no `video` feature: video is parked for v1 (see [Video](#video)).

The facade does **not** re-export per-format flags. Cargo cannot forward a child
crate's features, so re-listing them would be duplicated, rot-prone state — and
a build that wants only video should never have to reason about image flags. For
fine-grained control (individual formats, alternative codec backends) depend
on `rawshift-image` directly.

### Per-crate feature systems

The image and video libraries each carry their own feature systems — restating
them here would duplicate state that drifts. See each crate's README:

- [`rawshift-image` feature flags](./crates/rawshift-image/README.md#feature-flags)
  — a tiered per-format system (bundles → formats → directions →
  infrastructure; gamut is the backend, plus six retained implementation
  aliases for the permanent exceptions and blocked migrations).
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

The minimum supported Rust version (MSRV) tracks the minimum required by our upstream dependencies (currently **1.92.0**, set by [gamut](https://github.com/justin13888/gamut)) and will remain as low as the upstream dependencies require — it is never raised independently.

## Upstream dependency: gamut

rawshift consumes the published [gamut](https://github.com/justin13888/gamut) crates for image primitives, color, metadata, container parsing, and codecs. Their versions are managed centrally in the workspace `Cargo.toml`; git dependencies are not permitted because they prevent publishing rawshift. See [AGENTS.md](AGENTS.md) for the upstream-first policy that governs when rawshift may change in response to a gamut gap.

### Updating gamut dependencies

The workspace dependency table is the single point where upstream behaviour enters rawshift. Updating it is a deliberate, reviewed change, not a drive-by edit.

1. **One commit, one concern.** The commit updates only the gamut requirements in `[workspace.dependencies]` (plus `Cargo.lock`). No code changes ride along — migrations that *depend* on the update land in follow-up commits or a separate PR.
2. **Confirm the gate shipped.** Every gamut issue that blocked rawshift work must be closed and merged to gamut `master`, and the affected crates must be published to crates.io.
3. **Full test run.** `cargo test --workspace` and
   `cargo test -p rawshift-image --features full` must pass, including
   fixture-driven tests. Do not use workspace-wide `--all-features`: it
   deliberately enables mutually exclusive platform hardware backends.
4. **Full benchmark run.** Run the criterion benches (`decode`, `demosaic`, `pipeline`) against the pre-update baseline. Unexplained regressions block the update.
5. **CHANGELOG note.** Any behavioural change — decoder output bytes, error variants, metadata round-trip fidelity — gets a `CHANGELOG.md` entry. A pure no-op update is recorded as such.

Crates are added to `[workspace.dependencies]` **lazily**, as each migration issue starts consuming one, so the dependency list stays an accurate record of what rawshift actually uses.

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
