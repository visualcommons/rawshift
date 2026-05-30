# rawshift-image

Still-image decoding, RAW processing, and encoding for
[rawshift](https://github.com/justin13888/rawshift).

This crate carries the full per-format feature system described below. Most
consumers should depend on the [`rawshift`](https://crates.io/crates/rawshift)
facade (which re-exports this crate behind a coarse `image` feature) rather than
depending on `rawshift-image` directly. Depend on this crate directly only when
you need fine-grained control — individual formats, alternative codec backends,
the `tiff-parser` API, or `heic-vendored` linking.

## Format Support

| Format       | Decoding                                                                                 | Encoding                                                                                         | Notes                                     |
| ------------ | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | ----------------------------------------- |
| Sony ARW     | Custom LJPEG (Stabilizing)                                                               | N/A                                                                                              |                                           |
| Canon CR2    | Custom LJPEG (Incomplete)                                                                | N/A                                                                                              | No test fixtures.                         |
| Canon CR3    | Custom ISOBMFF parser (Incomplete)                                                       | N/A                                                                                              | Metadata only. CRX codec not implemented. |
| Canon CRW    | Custom CIFF parser (Incomplete)                                                          | N/A                                                                                              | Detection only. No pixel decode.          |
| Adobe DNG    | Custom TIFF + jxl-oxide (Stabilizing)                                                    | Custom TIFF writer (Stabilizing)                                                                 | Includes Apple ProRAW (DNG 1.7 + JXL).    |
| Nikon NEF    | Custom TIFF parser (Incomplete)                                                          | N/A                                                                                              | No test fixtures.                         |
| Fujifilm RAF | Custom RAF parser (Incomplete)                                                           | N/A                                                                                              | No test fixtures.                         |
| JPEG         | [zune-jpeg](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg) (Stable) | [jpeg-encoder](https://github.com/vstroebel/jpeg-encoder) (Stable, default) · [jpegli](https://github.com/google/jpegli) (distance/XYB + 16-bit input, opt-in) | jpegli via `jpeg-encode-jpegli` (system) / `jpeg-encode-jpegli-vendored` (from source). |
| PNG          | [zune-png](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-png) (Stable)   | [zune-png](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-png) (Stable)           |                                           |
| WebP         | [libwebp-sys](https://github.com/noxf/libwebp-sys) (Stable)                              | [libwebp-sys](https://github.com/noxf/libwebp-sys) (Stable)                                      | C FFI bindings to libwebp.                |
| GIF          | [gif](https://github.com/image-rs/image-gif) (Stable)                                    | Not planned                                                                                      |                                           |
| TIFF         | [tiff](https://github.com/image-rs/image-tiff) (Stable)                                  | Not planned                                                                                      |                                           |
| JXL          | [jxl-oxide](https://github.com/tirr-c/jxl-oxide) (Stable)                                | [zune-jpegxl](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpegxl) (Functional, default) · [libjxl](https://github.com/libjxl/libjxl) (16-bit + lossless, opt-in) | libjxl via `jxl-encode-libjxl` (system) / `jxl-encode-libjxl-vendored` (from source). |
| AVIF         | [image/avif-native](https://github.com/image-rs/image) (Functional)                      | [ravif](https://github.com/kornelski/cavif-rs/tree/main/ravif) (Functional)                      |                                           |
| HEIC         | [libheif](https://github.com/strukturag/libheif) (Functional)                            | Not planned                                                                                      | Requires `heic` feature; `heic-vendored` builds libheif from source. |
| SVG          | [resvg/tiny-skia](https://github.com/linebender/resvg) (Functional)                      | Not planned                                                                                      |                                           |
| PPM          | [zune-ppm](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-ppm) (Functional) | Not planned                                                                                    | Netpbm family: P5, P6, P7, PFM.           |

Note on encoding support: for formats without encoding support, you may still
take the decoded pixel data and metadata and encode it with your own logic.

Note on implementations: the decoder/encoder library named for each compressed
format above is that format's **default implementation** — the one selected by
its direction feature flag (e.g. `jpeg-decode`). A format may gain alternative
implementations over time; see [Feature Flags](#feature-flags) for how
implementations are named and selected.

## Feature Flags

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
   - `jpeg-decode-zune`, `jpeg-encode-jpeg-enc`, `jpeg-encode-jpegli`
   - `png-decode-zune`, `png-encode-zune`
   - `webp-decode-libwebp`, `webp-encode-libwebp`
   - `jxl-decode-jxl-oxide`, `jxl-encode-zune`, `jxl-encode-libjxl`
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
   - `jxl-encode-libjxl-vendored` — build libjxl from source via cmake and link
     it statically, instead of linking the system libjxl (`jxl-encode-libjxl`).
     Requires a C/C++ toolchain, cmake, and `libclang` (for bindgen).
   - `jpeg-encode-jpegli-vendored` — build the vendored `google/jpegli` submodule
     from source via cmake and link it statically, instead of linking a system
     libjpegli (`jpeg-encode-jpegli`). Requires a C/C++ toolchain, cmake, and
     `libclang`; init the submodule with
     `git submodule update --init --recursive crates/rawshift-image/third_party/jpegli`.

   The `zune-runtime` / `exif` / `container-embed` features are pulled in
   automatically by the format implementations that need them — they exist so
   that a minimal `rawshift-image` build links no decoder/metadata crate it does
   not use.

Resolution example: enabling `default` pulls in `png` → `png-decode` →
`png-decode-zune` → the `zune-png` crate. To use a non-default implementation,
enable its tier-4 feature explicitly and select it per call through
`DecodeOptions` / `EncodeOptions`; the default implementation stays available
alongside it.

## License

Licensed under [MPL-2.0](../../LICENSE).
