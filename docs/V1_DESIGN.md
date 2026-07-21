# rawshift v1 Design

The finalized shape of rawshift v1: full dependence on
[gamut](https://github.com/visualcommons/gamut) wherever it covers the ground,
upstream-first for every gap (see the policy in [AGENTS.md](../AGENTS.md)),
hardware codestream decode for HEVC/AV1, and no video. **v1 is the clean
final state: 0.x source compatibility is a non-goal.**

gamut is consumed as versioned crates.io dependencies managed in the workspace
`Cargo.toml` (see README "Updating gamut dependencies" for the procedure).

## Workspace layout

```
rawshift/                      facade — features: image (default), serde, hw, hw-*, full
├── crates/rawshift-core       #![forbid(unsafe_code)]; deps: gamut-core, gamut-color, serde(opt)
├── crates/rawshift-image      safe except processing/ + transforms/ hot paths;
│                              deps: rawshift-core, rawshift-hwdec(opt), gamut codec/metadata crates,
│                              gif, resvg, zune-ppm (permanent exceptions), rayon
├── crates/rawshift-hwdec      all platform FFI; hardware still-frame decode of HEVC + AV1
│                              (VideoToolbox / libva / NDK MediaCodec)
└── crates/rawshift-video      parked placeholder — excluded from publish set + facade
```

## rawshift-core

Charter: only types needed by both image and (future) video.
Modules: `geometry`, `sensor`, `color`, `codec`, `metadata`.

| Type | v1 disposition |
| --- | --- |
| `Size` | deleted → re-export `gamut_core::Dimensions` |
| `Point`, `Rect` | kept (`Rect { origin: Point, size: Dimensions }`) |
| `RawImage`(+Builder), `CfaPattern`, `XTransPattern`, `white_level_from_bit_depth` | kept unchanged (sensor domain); bridge to `gamut_dng` types lives in rawshift-image |
| `RgbImage` | deleted from core → rawshift-image wrapper over `ImageBuf<Rgb16>` |
| `pixel.rs` (`Sample`/`FromF32`/`Rgb<S>`/`Rgba<S>`) | deleted → re-export gamut-core `Sample`/`Pixel` markers; f32 stays transform-internal scratch |
| `ColorSpace` | deleted → new `ColorDescription` (CICP primaries+transfer pair; consts `SRGB`, `LINEAR_SRGB` (pipeline working space), `DISPLAY_P3`, `REC2020`, `UNSPECIFIED`; ICC-authoritative spaces such as Adobe RGB map to `UNSPECIFIED` with the preserved ICC profile as the authority); manual serde via CICP code points until gamut derives land |
| `BitDepth` | deleted → re-export `gamut_color::BitDepth` (gated on upstream adding `Sixteen`) |
| `CodecId`/`CodecInfo`/`CodecDirection`, `MetadataEmbedOptions` | kept (no gamut equivalent; video-shared / policy layer) |
| `ImageMetadata` + typed structs + `extra` table, `MetadataValue/Key/Namespace/Entry`, `URational`/`SRational` | kept — camera color science is rawshift's domain; gamut-metadata is the carrier; bridge `to_gamut`/`from_gamut` in rawshift-image |
| `MetadataExtractor` | kept, renamed `ExtractMetadata` (collision with gamut trait) |
| `IccProfile` | deleted → gamut-icc profile type (full ICC.1:2022) |

Root re-exports: gamut-core (`Dimensions, ImageBuf, ImageRef, Sample, Pixel,
ColorModel`, pixel markers) and gamut-color (`ColourPrimaries,
TransferCharacteristics, MatrixCoefficients, ColorRange, BitDepth,
ChromaSubsampling`).

## High-level image API

```rust
pub struct RgbImage { buf: ImageBuf<Rgb16>, color: ColorDescription,
                      baseline_exposure: Option<f32>, default_crop: Option<Rect> }
// new / with_color / from_buf / size / width / height / color /
// baseline_exposure / default_crop / as_buf() -> &ImageBuf<Rgb16> /
// into_buf() / into_data() / data() / data_mut() / replace_data() /
// set_color / set_baseline_exposure / set_default_crop
```

- Detection/probe: `detect_standard_format`, `probe_standard_image`,
  `read_standard_image_metadata` (shapes kept; `Dimensions`/`ColorDescription`).
- Decode: `decode_standard_image(_with)`; everything normalizes to `Rgb16`
  (u8×257, gray expanded, alpha dropped, sRGB-tagged, ICC preserved in
  metadata). `decode_jxl_partial` dropped until gamut-jxl supports truncated
  streams.
- Encode: `encode_rgb_image{,_to_vec,_to_writer}(image, metadata, …, options)`.
- RAW: `RawFile<R>::{open, format, metadata, thumbnail, decode_raw, process,
  export, is_linear_raw_dng}`; `ProcessingOptions` unchanged.
- Working format: u16 interleaved end-to-end; f32 transform-internal only.
- Errors: `RawError` gains `Gamut { context, source: gamut_core::Error }` and
  `HwDecoderUnavailable { codec, reason }`; backend-specific variants deleted.
- Registry: `available_decoders/encoders()` report the compiled backends with
  hand-maintained pinned versions. The AVIF/HEIC entries report the
  container/pipeline decoder (always compiled with the feature); whether the
  codestream can be decoded on this machine is runtime-conditional — probe
  with `avif_hw_decode_available()` / `heic_hw_decode_available()`.

Options are **format-keyed** (the backend-selection axis is gone — gamut is
the backend). Every variant is format-named; where a non-gamut backend
remains (blocked migrations, permanent exceptions) the *config struct* names
it honestly (`LibwebpDecodeConfig`, `TiffDecodeConfig`, `GifDecodeConfig`,
`ResvgDecodeConfig`, `ZunePpmDecodeConfig`). Config structs are
rawshift-owned mirrors of committed gamut knobs; see the tables below.

### DecodeOptions

| Variant | Fields (defaults) |
| --- | --- |
| `Png` | `max_width`, `max_height`, `max_image_bytes`, `max_metadata_bytes` (all None = gamut defaults; hostile-input resource guards) |
| `Svg` | `dpi: f32` (96.0) |
| `Jpeg`, `WebP`, `Jxl`, `Tiff`, `Avif`, `Heic`, `Gif`, `Ppm` | — (empty config types reserved for future knobs; JPEG decode resource guards are an upstream ask, gamut#306) |

### EncodeOptions

| Variant | Fields (defaults) |
| --- | --- |
| `Png` | `common`, `compression` (Default), `filter` (MinSumAbs), `auto_reduce` (false) |
| `Jpeg` | `common`, `quality: u8` (90), `subsampling` (4:2:0), `progressive` (false), `restart_interval: u16` (0), `density` (1:1 aspect ratio) |
| `WebP` | `common`, `mode` (Lossy), `quality: f32` (75.0), `method: u32` (4), `near_lossless: u32` (100 = off) |
| `Avif` | `common`, `lossless: bool` (true), `quality: u8` (80, lossy only); 10/12-bit output pending gamut#251 |
| `Jxl` | `common`, `lossless: bool` (true), `distance: f32` (1.0), `effort: u8` (7), `use_container` (false), `coded_bit_depth` (None) |
| `Dng` | `software`, `embed_exif` (true), `embed_gps` (own config shape; LinearRaw 16-bit uncompressed) |

TIFF encode (a `Tiff` variant) lands together with the gamut-tiff migration —
blocked upstream (gamut#299/#300, tracked by rawshift#22).

`CommonEncodeOptions { metadata: MetadataEmbedOptions (all true), bit_depth:
BitDepth (Sixteen where supported) }`.

## Format capability matrix

| Format | Decode | Encode | Backend |
| --- | --- | --- | --- |
| JPEG | ✅ | ✅ | gamut JPEG (upstream #28) |
| PNG | ✅ | ✅ | gamut-png (decoder upstream) |
| WebP | ✅ | ✅ | libwebp (`libwebp-sys`) — gamut-webp migration blocked upstream (gamut#302, tracked by rawshift#24) |
| JXL | ✅ (pure Rust) | ✅ (libjxl via gamut-jxl-sys) | gamut-jxl |
| TIFF | ✅ | — | `tiff` crate — gamut-tiff migration (incl. new encode) blocked upstream (gamut#299/#300, tracked by rawshift#22) |
| AVIF | ✅ hardware | ✅ (Rgb8 lossless now; 10/12-bit upstream) | gamut-avif container + rawshift-hwdec AV1 |
| HEIC | ✅ hardware | — | gamut-heic container + rawshift-hwdec HEVC |
| DNG | ✅ | ✅ | gamut-dng |
| GIF / SVG / PPM | ✅ | — | `gif` / `resvg` / `zune-ppm` (permanent exceptions) |
| APV | detect-only | — | magic bytes |
| ARW / CR2 / CR3 / CRW / NEF / RAF | ✅ (in-repo) | — | gamut-ifd / gamut-isobmff engines + rawshift tag catalogue (CRW: in-repo CIFF) |

## Feature flags & compile boundaries

Defaults: `jpeg, png, webp, jxl-decode, gif, tiff, ppm` — `jxl-encode` is
excluded from defaults because it wraps the reference libjxl (cmake + C++
toolchain via gamut-jxl-sys); it is part of `jxl` and `full`. Formats compose
`<format>-decode`/`<format>-encode`. Bundles: `raw-stabilizing` (arw, dng),
`raw-incomplete` (cr2, cr3, crw, nef, raf), `experimental`, `serde`, and
`full` = all formats + serde + experimental + `hw`.

Hardware backends are **verified feature flags**:

- `hw` — the native backend for the compile target (VideoToolbox on Apple,
  VAAPI on linux-gnu, MediaCodec on Android). Valid everywhere; on targets
  with no hardware API in [SUPPORT.md](./SUPPORT.md) it emits a build-script
  warning and compiles the stub.
- `hw-videotoolbox` / `hw-vaapi` / `hw-mediacodec` — pin one explicit
  backend; **`compile_error!` on any other target.**
- `heic`/`avif` without any hw feature = container/metadata-only build
  (valid; pixel decode returns `HwDecoderUnavailable`).

CI compiles the invalid combinations expecting failure and `full` on every
tier-1 target expecting success. Deleted feature axes: the per-implementation
flags of every gamut-backed format, `container-embed`, `tiff-parser`
(replaced by `ifd-parser` over gamut-ifd), `heic-vendored`, every
`*-vendored` linking flag. Retained (delivered reality, post-#34 audit):
six implementation aliases — `gif-decode-gif` / `svg-decode-resvg` /
`ppm-decode-zune` (permanent exceptions per AGENTS.md) and
`tiff-decode-tiff` / `webp-decode-libwebp` / `webp-encode-libwebp` (blocked
migrations: gamut#299/#300 via rawshift#22, gamut#302 via rawshift#24) —
plus the `zune-runtime` and `exif` infrastructure flags they and the
gamut metadata stack hang off.

## Hardware decode (rawshift-hwdec)

```rust
pub enum HwCodec  { Hevc, Av1 }
pub enum HwBackend { VideoToolbox, Vaapi, MediaCodec }
pub enum CodecConfig<'a> { Hvcc(&'a [u8]), Av1c(&'a [u8]) } // variant implies the codec
pub struct StillDecodeRequest<'a> { config: CodecConfig<'a>,
    payload /* NAL units | OBUs */, width, height, bit_depth, chroma /* advisory */ }
pub struct DecodedFrame { /* validated: format (Nv12|P010|I420|I010), width,
    height, bit_depth, range, planes — constructed via DecodedFrame::new only */ }
pub trait HwStillDecoder: Send { fn decode_still(&mut self, req: &StillDecodeRequest<'_>) -> Result<DecodedFrame, HwDecodeError>; }
pub fn decoder(codec: HwCodec) -> Option<Box<dyn HwStillDecoder>>;
pub fn backend() -> Option<HwBackend>;
pub fn available_codecs() -> &'static [HwCodec];
```

- HEIC: gamut-heic parses the container (landed upstream in #238) → hwdec
  decodes items → tile stitch + irot/imir/clap in safe Rust → YCbCr→RGB via
  gamut-color CICP → `RgbImage`.
- AVIF: same pipeline (landed upstream in gamut#250): gamut-avif parses the
  container → hwdec decodes items via av1C + OBU payload → grid stitch +
  alpha merge + irot/imir/clap in gamut-avif → `RgbImage`.
- Safety: all platform unsafe lives in this crate;
  `#![deny(unsafe_op_in_unsafe_fn)]`; safe public items; VAAPI is dlopen'd.
- Platform/API commitments and exclusion justifications: [SUPPORT.md](./SUPPORT.md).

## TIFF parser

The binrw `tiff/` module (public `TiffParser`/`TiffWriter`/`TiffValue`) is
deleted along with the `binrw` dependency. ARW/CR2/NEF (+ RAF metadata)
rebuild on `gamut_ifd`; CR3 box walking moves to gamut-isobmff; CRW keeps its
CIFF reader; DNG moves wholesale to gamut-dng. rawshift keeps a private
vendor tag catalogue (Sony SR2, Nikon maker, DNG tags). The swap is gated on
upstream capability (RAW-grade parsing) **and** `chore`-labeled hardening
verification (bounds/cycle checks, byte completeness, fuzz coverage) matching
the current parser's guarantees.

## Dependencies deleted

zune-jpeg, zune-png, zune-jpegxl, jpeg-encoder, vendored jpegli
(+submodule/cc/cmake/bindgen), jxl-oxide, direct libjxl glue,
ravif, avif-serialize, libaom-sys, image, libheif-rs, little_exif,
img-parts, binrw. `build.rs` shrinks to cfg aliases + feature/target
verification.

Still present, pending blocked upstream migrations (post-#34 audit):
`libwebp-sys` (gamut-webp — gamut#302 via rawshift#24) and `tiff`
(gamut-tiff — gamut#299/#300 via rawshift#22). Permanent exceptions that
stay: `gif`, `resvg`, `zune-ppm` (+ its `zune-core` runtime).

## Release

All gamut dependencies are published on crates.io, so the rawshift workspace
can be packaged and published in dependency order by release-plz.
