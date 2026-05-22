# rawshift

Raw image processing library focused on compatibility, correctness, and interoperability.

## Why rawshift?

Image processing is messy and no single library could do it all (e.g., accurate colour, complete metadata, consistent format support). Rawshift seeks to stand out in at least some of the following ways:

- Compatibility: A single library processes all forms of image formats including all popular compressed and RAW image formats used by both consumers and creative professionals.
- Correctness: While decoding implementations should remain flexible (e.g. to slightly non-conformant image), it should strictly conform to open standards and retain maximum metadata across formats.
- Interoperability: This library compiles and is optimized for several standard desktop and mobile platforms. This is possible because the majority of the library is written in pure Rust and non-Rust dependencies are encapsulated with best practices.

## Getting Started

> This library is still in active development. See `master` branch for latest improvements (noting potential instability). Alpha packages may be published to crates.io occasionally.

<!-- TODO: Add docs on the specific features, etc. on the docs.rs page -->

## Format Support

Rawshift targets both still image and video formats. Image decoding is the
current focus; video support is planned but not yet implemented (see
[Video](#video)).

### Images

Here is the list of image formats that are being worked on in order of priority:

- Sony ARW (all variations at least up to v5.0.1)
- Adobe DNG (up to v1.7, including what is necessary for Apple ProRAW)
- Standard TIFF

- Canon CR3
- Canon CR2

> Features and performance are being constantly improved. As most functionality are implemented from scratch to meet project goals, expect progressive improvements for format support over time.

> We aim to be liberal in what we accept (decode) and strict in what we give (encode).

| Format       | Decoding                                                                                 | Encoding                                                                                         | Notes                                     |
| ------------ | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | ----------------------------------------- |
| Sony ARW     | Custom LJPEG (Stabilizing)                                                               | N/A                                                                                              |                                           |
| Canon CR2    | Custom LJPEG (Incomplete)                                                                | N/A                                                                                              | No test fixtures.                         |
| Canon CR3    | Custom ISOBMFF parser (Incomplete)                                                       | N/A                                                                                              | Metadata only. CRX codec not implemented. |
| Canon CRW    | Custom CIFF parser (Incomplete)                                                          | N/A                                                                                              | Detection only. No pixel decode.          |
| Adobe DNG    | Custom TIFF + jxl-oxide (Stabilizing)                                                    | Custom TIFF writer (Stabilizing)                                                                 | Includes Apple ProRAW (DNG 1.7 + JXL).    |
| Nikon NEF    | Custom TIFF parser (Incomplete)                                                          | N/A                                                                                              | No test fixtures.                         |
| Fujifilm RAF | Custom RAF parser (Incomplete)                                                           | N/A                                                                                              | No test fixtures.                         |
| JPEG         | [zune-jpeg](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg) (Stable) | [jpeg-encoder](https://github.com/vstroebel/jpeg-encoder) (Stable)                               |                                           |
| PNG          | [zune-png](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-png) (Stable)   | [zune-png](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-png) (Stable)           |                                           |
| WebP         | [libwebp-sys](https://github.com/noxf/libwebp-sys) (Stable)                              | [libwebp-sys](https://github.com/noxf/libwebp-sys) (Stable)                                      | C FFI bindings to libwebp.                |
| GIF          | [gif](https://github.com/image-rs/image-gif) (Stable)                                    | Not planned                                                                                      |                                           |
| TIFF         | [tiff](https://github.com/image-rs/image-tiff) (Stable)                                  | Not planned                                                                                      |                                           |
| JXL          | [jxl-oxide](https://github.com/tirr-c/jxl-oxide) (Stable)                                | [zune-jpegxl](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpegxl) (Functional) |                                           |
| AVIF         | [image/avif-native](https://github.com/image-rs/image) (Functional)                      | [ravif](https://github.com/kornelski/cavif-rs/tree/main/ravif) (Functional)                      |                                           |
| HEIC         | [libheif](https://github.com/strukturag/libheif) (Functional)                            | Not planned                                                                                      | Requires `heic` feature; `heic-vendored` builds libheif from source. |
| SVG          | [resvg/tiny-skia](https://github.com/linebender/resvg) (Functional)                      | Not planned                                                                                      |                                           |
| PPM          | [zune-ppm](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-ppm) (Functional) | Not planned                                                                                    | Netpbm family: P5, P6, P7, PFM.           |

Note on encoding support: For formats that we do not support encoding, you may still take the decoded pixel data and metadata, and encode it with your own encoding logic.

Note on implementations: the decoder/encoder library named for each compressed format above is that format's **default implementation** — the one selected by its direction feature flag (e.g. `jpeg-decode`). A format may gain alternative implementations over time; see [Feature Flags](#feature-flags) for how implementations are named and selected.

### Video

Video support is **planned and not yet implemented** — no video code ships today, and there are no video feature flags. The formats below are on the roadmap, prioritised by the cameras in our [official supported device list](#official-supported-device-list):

| Format / Codec       | Container       | Status  | Notes                                          |
| -------------------- | --------------- | ------- | ---------------------------------------------- |
| XAVC HS (H.265/HEVC) | MP4             | Planned | Sony mirrorless video.                         |
| XAVC S (H.264/AVC)   | MP4             | Planned | Sony mirrorless video.                         |
| Apple ProRes         | QuickTime (MOV) | Planned | iPhone Pro and professional editing workflows. |
| HEVC (H.265)         | QuickTime (MOV) | Planned | Default iPhone video.                          |
| H.264 (AVC)          | QuickTime (MOV) | Planned | Legacy and compatibility video.                |

Initial work will focus on container parsing and metadata extraction, reusing the in-repo ISOBMFF parser already used for Canon CR3 (both MP4 and QuickTime are ISOBMFF-based). Codec-level decoding is a later milestone.

## Crates

rawshift is a Cargo workspace:

| Crate            | Purpose                                                                                                          |
| ---------------- | ---------------------------------------------------------------------------------------------------------------- |
| `rawshift`       | Facade. Re-exports the libraries below behind coarse `image` / `video` features. Most consumers depend on this.  |
| `rawshift-image` | Still-image decoding, RAW processing, and encoding. Carries the full per-format feature system described below.  |
| `rawshift-video` | Video support — planned, not yet implemented (see [Video](#video)).                                              |
| `rawshift-core`  | Shared types — geometry, pixel samples, the metadata model — used by both libraries.                             |

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

### Image library — `rawshift-image`

Cargo features are organised in five tiers, from high-level bundles down to
individual library bindings. Each tier is defined purely in terms of the tier
below it; only tier-4 features (and RAW tier-3 features) pull in an external
crate.

1. **Bundle features** — coarse, ready-made groupings.
   - `default` — `jpeg`, `png`, `webp`, `jxl-decode`, `gif-decode`, `tiff-decode`, `ppm-decode`.
   - `full` — every format, `serde`, and all RAW formats.
   - `experimental` — all RAW formats (`raw-stabilizing` + `raw-incomplete`).
   - `raw-stabilizing` — RAW formats with test fixtures and working decode (ARW, DNG).
   - `raw-incomplete` — RAW formats still missing fixtures or pixel decode (CR2, CR3, CRW, NEF, RAF).
2. **Format features** — one per image format; enables decode **and** encode for
   that format (or decode only, where encode is unsupported).
   - `jpeg`, `png`, `webp`, `jxl`, `avif`, `dng`
   - `gif`, `tiff`, `heic`, `svg`, `ppm` — decode-only
   - `arw`, `cr2`, `cr3`, `crw`, `nef`, `raf` — RAW, decode-only
3. **Direction features** — one per format per direction.
   - Compressed formats: `jpeg-decode`, `jpeg-encode`, `png-decode`, `png-encode`,
     `webp-decode`, `webp-encode`, `jxl-decode`, `jxl-encode`, `gif-decode`,
     `tiff-decode`, `avif-decode`, `avif-encode`, `heic-decode`, `svg-decode`,
     `ppm-decode` — each is an **alias for that format+direction's default
     implementation**.
     This is where the per-format default is defined.
   - RAW formats: `arw-decode`, `cr2-decode`, `cr3-decode`, `crw-decode`,
     `dng-decode`, `dng-encode`, `nef-decode`, `raf-decode` — RAW formats have a
     single in-repo implementation, so there is no tier-4 layer below them.
4. **Implementation features** — *compressed formats only*, named
   `format-direction-impl`. Each selects exactly one backend library and is the
   only tier that pulls an external crate. Multiple implementations of the same
   format+direction may be enabled simultaneously; the active backend is chosen
   at the API level via `DecodeOptions` / `EncodeOptions`.
   - `jpeg-decode-zune`, `jpeg-encode-jpeg-enc`
   - `png-decode-zune`, `png-encode-zune`
   - `webp-decode-libwebp`, `webp-encode-libwebp`
   - `jxl-decode-jxl-oxide`, `jxl-encode-zune`
   - `gif-decode-gif`, `tiff-decode-tiff`
   - `avif-decode-image`, `avif-encode-ravif`
   - `heic-decode-libheif`, `svg-decode-resvg`
   - `ppm-decode-zune`
5. **Infrastructure / linking features** — cross-cutting, not tied to one format.
   - `tiff-parser` — internal TIFF structure parser plus the public `TiffParser` API.
   - `serde` — `Serialize`/`Deserialize` for metadata and option types.
   - `zune-runtime` — `zune-core` codec primitives; pulled by zune-backed impls.
   - `exif` — typed EXIF read/write (`little_exif`); pulled by impls that touch EXIF.
   - `container-embed` — container segment muxing (`img-parts`); pulled by encode
     impls that embed EXIF/ICC/XMP.
   - `heic-vendored` — build libheif from source and link it statically, instead
     of linking the system libheif (`heic`). Requires a C/C++ toolchain + cmake.

   The `zune-runtime` / `exif` / `container-embed` features are pulled in
   automatically by the format implementations that need them — they exist so
   that a minimal `rawshift-image` build links no decoder/metadata crate it does
   not use.

Resolution example: enabling `default` pulls in `png` → `png-decode` →
`png-decode-zune` → the `zune-png` crate. To use a non-default implementation,
enable its tier-4 feature explicitly and select it per call through
`DecodeOptions` / `EncodeOptions`; the default implementation stays available
alongside it.

### Video library — `rawshift-video`

Video features mirror the image crate's tier structure but currently gate no
code or dependencies — they exist so the surface is laid out ahead of the
decoder work (see [Video](#video)):

- **Bundles** — `video` (all formats), `full`.
- **Formats** — `xavc-hs`, `xavc-s`, `hevc`, `h264`, `prores`.
- **Directions** — `xavc-hs-decode`, `xavc-s-decode`, `hevc-decode`,
  `h264-decode`, `prores-decode` (decode-only for now).

## Official supported device list

*A device is officially supported if we have thoroughly tested compatibility for it.*

> Compatibility is verified against the **default decoder implementation** for each format (the first/named library for that format in the Format Support table). Non-default implementations selected via implementation feature flags are not covered by this list.

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
