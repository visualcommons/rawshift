# rawshift-image

Still-image decoding, RAW processing, and encoding for
[rawshift](https://github.com/justin13888/rawshift).

This crate carries the full per-format feature system described below. Most
consumers should depend on the [`rawshift`](https://crates.io/crates/rawshift)
facade (which re-exports this crate behind a coarse `image` feature) rather than
depending on `rawshift-image` directly. Depend on this crate directly only when
you need fine-grained control — individual formats, alternative codec backends,
or an explicit hardware-decode backend pin (`hw-*`).

## Format Support

| Format       | Decoding                                                                                 | Encoding                                                                                         | Notes                                     |
| ------------ | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | ----------------------------------------- |
| Sony ARW     | Custom LJPEG (Stabilizing)                                                               | N/A                                                                                              |                                           |
| Canon CR2    | Custom LJPEG (Incomplete)                                                                | N/A                                                                                              | No test fixtures.                         |
| Canon CR3    | Custom ISOBMFF parser (Incomplete)                                                       | N/A                                                                                              | Metadata only. CRX codec not implemented. |
| Canon CRW    | Custom CIFF parser (Incomplete)                                                          | N/A                                                                                              | Detection only. No pixel decode.          |
| Adobe DNG    | [gamut-dng](https://github.com/justin13888/gamut) (Stabilizing)                          | Custom TIFF writer (Stabilizing)                                                                 | Includes Apple ProRAW (DNG 1.7 + JXL).    |
| Nikon NEF    | Custom TIFF parser (Incomplete)                                                          | N/A                                                                                              | No test fixtures.                         |
| Fujifilm RAF | Custom RAF parser (Incomplete)                                                           | N/A                                                                                              | No test fixtures.                         |
| JPEG         | [gamut-jpeg](https://github.com/justin13888/gamut) (Stable)                             | [gamut-jpeg](https://github.com/justin13888/gamut) (Stable)                                      | Pure Rust. Decode: baseline + progressive, grayscale/YCbCr/RGB/CMYK/YCCK. Encode: baseline or progressive 8-bit DCT (quality/subsampling/restart/density); APP1/APP2 EXIF/XMP/ICC both ways. |
| PNG          | [gamut-png](https://github.com/justin13888/gamut) (Stable)                              | [gamut-png](https://github.com/justin13888/gamut) (Stable)                                       | Pure Rust. Decode: every colour type/bit depth incl. Adam7, eXIf/iCCP/XMP extraction, resource guards. Encode: 8/16-bit RGB, eXIf/iCCP/XMP chunks. |
| WebP         | [libwebp-sys](https://github.com/noxf/libwebp-sys) (Stable)                              | [libwebp-sys](https://github.com/noxf/libwebp-sys) (Stable)                                      | C FFI bindings to libwebp.                |
| GIF          | [gif](https://github.com/image-rs/image-gif) (Stable)                                    | Not planned                                                                                      |                                           |
| TIFF         | [tiff](https://github.com/image-rs/image-tiff) (Stable)                                  | Not planned                                                                                      |                                           |
| JXL          | [gamut-jxl](https://github.com/justin13888/gamut) (Stable)                               | [gamut-jxl](https://github.com/justin13888/gamut) (Stable)                                       | Decode is pure Rust (jxl-rs); encode wraps the reference libjxl, cmake-built and statically linked by gamut-jxl-sys. |
| AVIF         | [image/avif-native](https://github.com/image-rs/image) (Functional)                      | [gamut-avif](https://github.com/justin13888/gamut) (Functional)                                  | Encode via gamut (pure Rust; 8-bit RGB, lossless/lossy AV1 intra, 4:4:4). 10/12-bit encode temporarily unavailable, pending [gamut#251](https://github.com/justin13888/gamut/issues/251). |
| HEIC         | [gamut-heic](https://github.com/justin13888/gamut) container/pipeline + [rawshift-hwdec](../rawshift-hwdec) hardware HEVC (Functional) | Not planned                                                                                      | Requires `heic` feature. Container, metadata, and auxiliary enumeration always work; pixel decode needs a hardware HEVC backend (`hw`/`hw-*`) and reports `HwDecoderUnavailable` without one. |
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
below it; only tier-4 features (plus RAW tier-3 features and the gamut-backed
`jpeg-decode` / `jpeg-encode` / `png-decode` / `png-encode` / `jxl-decode` /
`jxl-encode` / `avif-encode`) pull in an external crate.

1. **Bundle features** — coarse, ready-made groupings.
   - `default` — `jpeg`, `png`, `webp`, `jxl-decode`, `gif-decode`, `tiff-decode`, `ppm-decode`.
   - `full` — every format, `serde`, all RAW formats, and `hw`.
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
     This is where the per-format default is defined. Exception: `jpeg-decode`,
     `jpeg-encode`, `png-decode`, `png-encode`, `jxl-decode`, `jxl-encode`,
     `avif-encode`, and `heic-decode` each have a single gamut-backed implementation
     (`gamut-jpeg` / `gamut-png` / `gamut-jxl` / `gamut-avif` / `gamut-heic`)
     and pull it directly, with no tier-4 layer
     below them. (`jxl-encode` wraps the
     reference libjxl, which `gamut-jxl-sys` cmake-builds and links statically
     — it needs cmake and a C++ toolchain. `avif-encode` is pure Rust: 8-bit
     RGB, lossless or lossy AV1 intra; 10/12-bit AVIF encode is temporarily
     unavailable, pending
     [gamut#251](https://github.com/justin13888/gamut/issues/251).)
   - RAW formats: `arw-decode`, `cr2-decode`, `cr3-decode`, `crw-decode`,
     `dng-decode`, `dng-encode`, `nef-decode`, `raf-decode` — RAW formats have a
     single in-repo implementation, so there is no tier-4 layer below them.
4. **Implementation features** — *compressed formats only*, named
   `format-direction-impl`. Each selects exactly one backend library and is the
   only tier that pulls an external crate. Multiple implementations of the same
   format+direction may be enabled simultaneously; the active backend is chosen
   at the API level via `DecodeOptions` / `EncodeOptions`.
   - `webp-decode-libwebp`, `webp-encode-libwebp`
   - `gif-decode-gif`, `tiff-decode-tiff`
   - `avif-decode-image`
   - `svg-decode-resvg`
   - `ppm-decode-zune`
5. **Infrastructure / linking features** — cross-cutting, not tied to one format.
   - `tiff-parser` — internal TIFF structure parser plus the public `TiffParser` API.
   - `serde` — `Serialize`/`Deserialize` for metadata and option types.
   - `zune-runtime` — `zune-core` codec primitives; pulled by zune-backed impls.
   - `exif` — typed EXIF read/write via the gamut metadata stack (`gamut-exif`,
     `gamut-metadata`, `gamut-xmp`); pulled by impls that touch EXIF.
   - `hw` — hardware still-frame decode via `rawshift-hwdec`, selecting the
     **native** backend for the compile target (VideoToolbox on Apple, VAAPI on
     linux-gnu, MediaCodec on Android — the permanent matrix in
     `docs/SUPPORT.md`). Valid everywhere; targets with no hardware decode API
     get a build warning and the no-backend stub.
   - `hw-videotoolbox` / `hw-vaapi` / `hw-mediacodec` — pin one explicit
     backend; **`compile_error!` on any other target** (verified feature
     flags). Without any `hw` flag, `heic` is a valid container/metadata-only
     build whose pixel decode returns `RawError::HwDecoderUnavailable`.

   The `zune-runtime` / `exif` features are pulled in automatically by the
   format implementations that need them — they exist so that a minimal
   `rawshift-image` build links no decoder/metadata crate it does not use.

Resolution example: enabling `default` pulls in `ppm` → `ppm-decode` →
`ppm-decode-zune` → the `zune-ppm` crate. To use a non-default implementation,
enable its tier-4 feature explicitly and select it per call through
`DecodeOptions` / `EncodeOptions`; the default implementation stays available
alongside it.

## License

Licensed under [MPL-2.0](../../LICENSE).
