# rawshift-image

Still-image decoding, RAW processing, and encoding for
[rawshift](https://github.com/visualcommons/rawshift).

This crate carries the full per-format feature system described below. Most
consumers should depend on the [`rawshift`](https://crates.io/crates/rawshift)
facade (which re-exports this crate behind a coarse `image` feature) rather than
depending on `rawshift-image` directly. Depend on this crate directly only when
you need fine-grained control — individual formats, alternative codec backends,
or an explicit hardware-decode backend pin (`hw-*`).

## Format Support

| Format       | Decoding                                                                                 | Encoding                                                                                         | Notes                                     |
| ------------ | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | ----------------------------------------- |
| Sony ARW     | [gamut-ifd](https://github.com/visualcommons/gamut) structure + in-repo LJPEG (Stabilizing) | N/A                                                                                              |                                           |
| Canon CR2    | [gamut-ifd](https://github.com/visualcommons/gamut) structure + in-repo LJPEG (Incomplete)  | N/A                                                                                              | No test fixtures.                         |
| Canon CR3    | In-repo ISOBMFF box walk + [gamut-ifd](https://github.com/visualcommons/gamut) for embedded TIFF (Incomplete) | N/A                                                                            | Metadata only. CRX codec not implemented. |
| Canon CRW    | Custom CIFF parser (Incomplete)                                                          | N/A                                                                                              | Detection only. No pixel decode.          |
| Adobe DNG    | [gamut-dng](https://github.com/visualcommons/gamut) (Stabilizing)                          | [gamut-dng](https://github.com/visualcommons/gamut) `DngEncoder` (Stabilizing)                     | Includes Apple ProRAW (DNG 1.7 + JXL).    |
| Nikon NEF    | [gamut-ifd](https://github.com/visualcommons/gamut) structure (Incomplete)                  | N/A                                                                                              | No test fixtures.                         |
| Fujifilm RAF | Custom RAF parser (Incomplete)                                                           | N/A                                                                                              | No test fixtures.                         |
| JPEG         | [gamut-jpeg](https://github.com/visualcommons/gamut) (Stable)                             | [gamut-jpeg](https://github.com/visualcommons/gamut) (Stable)                                      | Pure Rust. Decode: baseline + progressive, grayscale/YCbCr/RGB/CMYK/YCCK. Encode: baseline or progressive 8-bit DCT (quality/subsampling/restart/density); APP1/APP2 EXIF/XMP/ICC both ways. |
| PNG          | [gamut-png](https://github.com/visualcommons/gamut) (Stable)                              | [gamut-png](https://github.com/visualcommons/gamut) (Stable)                                       | Pure Rust. Decode: every colour type/bit depth incl. Adam7, eXIf/iCCP/XMP extraction, resource guards. Encode: 8/16-bit RGB, eXIf/iCCP/XMP chunks. |
| WebP         | [libwebp-sys](https://github.com/noxf/libwebp-sys) (Stable)                              | [libwebp-sys](https://github.com/noxf/libwebp-sys) (Stable)                                      | C FFI bindings to libwebp. gamut-webp migration blocked upstream ([gamut#302](https://github.com/visualcommons/gamut/issues/302), tracked by [rawshift#24](https://github.com/visualcommons/rawshift/issues/24)). |
| GIF          | [gif](https://github.com/image-rs/image-gif) (Stable)                                    | Not planned                                                                                      | Permanent exception to the gamut migration (AGENTS.md). |
| TIFF         | [tiff](https://github.com/image-rs/image-tiff) (Stable)                                  | Not planned                                                                                      | gamut-tiff migration blocked upstream ([gamut#299](https://github.com/visualcommons/gamut/issues/299)/[#300](https://github.com/visualcommons/gamut/issues/300), tracked by [rawshift#22](https://github.com/visualcommons/rawshift/issues/22)). |
| JXL          | [gamut-jxl](https://github.com/visualcommons/gamut) (Stable)                               | [gamut-jxl](https://github.com/visualcommons/gamut) (Stable)                                       | Decode is pure Rust (jxl-rs); encode wraps the reference libjxl, cmake-built and statically linked by gamut-jxl-sys. |
| AVIF         | [gamut-avif](https://github.com/visualcommons/gamut) container/pipeline + [rawshift-hwdec](../rawshift-hwdec) hardware AV1 (Functional) | [gamut-avif](https://github.com/visualcommons/gamut) (Functional)                                  | Decode: container, metadata, and auxiliary enumeration always work; pixel decode needs a hardware AV1 backend (`hw`/`hw-*`, AV1 Profile 0) and reports `HwDecoderUnavailable` without one — software fallback is post-v1 ([gamut#259](https://github.com/visualcommons/gamut/issues/259)); 10/12-bit presentation pending [gamut#303](https://github.com/visualcommons/gamut/issues/303). Encode via gamut (pure Rust; 8-bit RGB, lossless/lossy AV1 intra, 4:4:4). 10/12-bit encode temporarily unavailable, pending [gamut#251](https://github.com/visualcommons/gamut/issues/251). |
| HEIC         | [gamut-heic](https://github.com/visualcommons/gamut) container/pipeline + [rawshift-hwdec](../rawshift-hwdec) hardware HEVC (Functional) | Not planned                                                                                      | Requires `heic` feature. Container, metadata, and auxiliary enumeration always work; pixel decode needs a hardware HEVC backend (`hw`/`hw-*`) and reports `HwDecoderUnavailable` without one. |
| SVG          | [resvg/tiny-skia](https://github.com/linebender/resvg) (Functional)                      | Not planned                                                                                      | Permanent exception to the gamut migration (AGENTS.md). |
| PPM          | [zune-ppm](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-ppm) (Functional) | Not planned                                                                                    | Netpbm family: P5, P6, P7, PFM. Permanent exception to the gamut migration (AGENTS.md). |

Note on encoding support: for formats without encoding support, you may still
take the decoded pixel data and metadata and encode it with your own logic.

Note on implementations: the decoder/encoder library named for each compressed
format above is that format's **only implementation** — the one selected by
its direction feature flag (e.g. `jpeg-decode`). gamut is the backend for
every migrated format; the exceptions are the retained tier-4 aliases listed
under [Feature Flags](#feature-flags).

## Feature Flags

Cargo features are organised in tiers, from high-level bundles down to
infrastructure flags. gamut is the backend: every gamut-backed direction
feature pulls its `gamut-*` crate directly, so the tier-4 implementation
layer has collapsed to six retained aliases — the permanent exceptions
(GIF/SVG/PPM) and the blocked migrations (TIFF/WebP) listed under tier 4
below.

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
   - gamut-backed formats: `jpeg-decode`, `jpeg-encode`, `png-decode`,
     `png-encode`, `jxl-decode`, `jxl-encode`, `avif-decode`, `avif-encode`,
     `heic-decode` — each has a single gamut-backed implementation
     (`gamut-jpeg` / `gamut-png` / `gamut-jxl` / `gamut-avif` / `gamut-heic`)
     and pulls it directly, with no tier-4 layer below it. (`jxl-encode` wraps
     the reference libjxl, which `gamut-jxl-sys` cmake-builds and links
     statically — it needs cmake and a C++ toolchain. `avif-encode` is pure
     Rust: 8-bit RGB, lossless or lossy AV1 intra; 10/12-bit AVIF encode is
     temporarily unavailable, pending
     [gamut#251](https://github.com/visualcommons/gamut/issues/251).
     `avif-decode` is container/metadata pure Rust; its pixel decode needs a
     hardware AV1 backend — see the `hw` flags under tier 5.)
   - Exception/blocked formats: `webp-decode`, `webp-encode`, `gif-decode`,
     `tiff-decode`, `svg-decode`, `ppm-decode` — each is an alias for its
     retained tier-4 implementation feature (see tier 4).
   - RAW formats: `arw-decode`, `cr2-decode`, `cr3-decode`, `crw-decode`,
     `dng-decode`, `dng-encode`, `nef-decode`, `raf-decode` — RAW formats have a
     single in-repo implementation, so there is no tier-4 layer below them.
4. **Implementation features** — six retained aliases, named
   `format-direction-impl`. The gamut migrations collapsed this tier for every
   gamut-backed format; these remain, each for a named reason:
   - `gif-decode-gif`, `svg-decode-resvg`, `ppm-decode-zune` — permanent
     exceptions to the gamut migration (AGENTS.md upstream-first policy).
   - `tiff-decode-tiff` — gamut-tiff migration blocked upstream
     ([gamut#299](https://github.com/visualcommons/gamut/issues/299)/[#300](https://github.com/visualcommons/gamut/issues/300),
     tracked by [rawshift#22](https://github.com/visualcommons/rawshift/issues/22)).
   - `webp-decode-libwebp`, `webp-encode-libwebp` — gamut-webp migration
     blocked upstream
     ([gamut#302](https://github.com/visualcommons/gamut/issues/302), tracked by
     [rawshift#24](https://github.com/visualcommons/rawshift/issues/24)).
5. **Infrastructure / linking features** — cross-cutting, not tied to one format.
   - `ifd-parser` — the gamut-ifd TIFF/IFD structure engine used by the
     TIFF-based RAW decoders and format detection.
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
     flags). Without any `hw` flag, `heic` and `avif` are valid
     container/metadata-only builds whose pixel decode returns
     `RawError::HwDecoderUnavailable`.

   The `zune-runtime` / `exif` features are pulled in automatically by the
   format implementations that need them — they exist so that a minimal
   `rawshift-image` build links no decoder/metadata crate it does not use.

Resolution example: enabling `default` pulls in `ppm` → `ppm-decode` →
`ppm-decode-zune` → the `zune-ppm` crate. Every format+direction currently has
exactly one implementation (gamut for migrated formats, the retained alias's
backend otherwise), selected per call through `DecodeOptions` /
`EncodeOptions`.

## License

Licensed under [MPL-2.0](../../LICENSE).
