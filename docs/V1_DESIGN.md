# rawshift v1 Design

The finalized shape of rawshift v1: full dependence on
[gamut](https://github.com/justin13888/gamut) wherever it covers the ground,
upstream-first for every gap (see the policy in [AGENTS.md](../AGENTS.md)),
hardware codestream decode for HEVC/AV1, and no video. **v1 is the clean
final state: 0.x source compatibility is a non-goal.**

gamut is consumed as a git dependency pinned to commit
`295c89248b9130873fe40c99529a99769c590f98`.

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
| `ColorSpace` | deleted → new `ColorDescription` (CICP primaries+transfer pair; consts `SRGB`, `LINEAR_SRGB` (pipeline working space), `DISPLAY_P3`, `REC2020_PQ`, `ADOBE_RGB` (= Unspecified + ICC-authoritative), `UNKNOWN`); manual serde via CICP code points until gamut derives land |
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
// new / from_buf / dimensions / color / baseline_exposure / default_crop /
// as_ref() -> ImageRef<'_, Rgb16> / into_buf() / data() / data_mut()
```

- Detection/probe: `detect_standard_format`, `probe_standard_image`,
  `read_standard_image_metadata` (shapes kept; `Dimensions`/`ColorDescription`).
- Decode: `decode_standard_image(_with)`; everything normalizes to `Rgb16`
  (u8×257, gray expanded, alpha dropped, sRGB-tagged, ICC preserved in
  metadata). `decode_jxl_partial` dropped until gamut-jxl supports truncated
  streams.
- Encode: `encode_image{,_to_vec,_to_writer}(image, metadata, …, options)`.
- RAW: `RawFile<R>::{open, format, metadata, thumbnail, decode_raw, process,
  export, is_linear_raw_dng}`; `ProcessingOptions` unchanged.
- Working format: u16 interleaved end-to-end; f32 transform-internal only.
- Errors: `RawError` gains `Gamut { context, source: gamut_core::Error }` and
  `HwDecoderUnavailable { codec, reason }`; backend-specific variants deleted.
- Registry: `available_decoders/encoders()` report gamut backends with pinned
  versions and runtime-conditional hardware entries.

Options are **format-keyed** (the backend-selection axis is gone — gamut is
the backend). Config structs are rawshift-owned mirrors of committed gamut
knobs; see the tables below.

### DecodeOptions

| Variant | Fields (defaults) |
| --- | --- |
| `Jpeg` | `max_width`, `max_height` (None), `strict` (false) |
| `Png` | `max_width`, `max_height` (None), `strict` (false) |
| `WebP`, `Jxl`, `Tiff`, `Avif`, `Heic`, `Gif`, `Ppm` | — |
| `Svg` | `dpi: f32` (96.0) |

### EncodeOptions

| Variant | Fields (defaults) |
| --- | --- |
| `Png` | `common` |
| `Jpeg` | `common`, `quality: u8` (85), `subsampling` (4:2:0), `progressive` (false) |
| `WebP` | `common`, `mode` (Lossy), `quality: f32` (75.0), `method: u8` (4) |
| `Avif` | `common`, `quality: Option<u8>` (None = lossless), `speed: u8` (6) |
| `Jxl` | `common`, `distance: f32` (1.0), `lossless` (false), `effort: u8` (7) |
| `Tiff` | `common`, `compression` (Deflate) — new capability |
| `Dng` | `common`, `compression` (LosslessJpeg), preview/profile options |

`CommonEncodeOptions { metadata: MetadataEmbedOptions (all true), bit_depth:
BitDepth (Sixteen where supported) }`.

## Format capability matrix

| Format | Decode | Encode | Backend |
| --- | --- | --- | --- |
| JPEG | ✅ | ✅ | gamut JPEG (upstream #28) |
| PNG | ✅ | ✅ | gamut-png (decoder upstream) |
| WebP | ✅ | ✅ | gamut-webp |
| JXL | ✅ (pure Rust) | ✅ (libjxl via gamut-jxl-sys) | gamut-jxl |
| TIFF | ✅ | ✅ (new) | gamut-tiff |
| AVIF | ✅ hardware | ✅ (Rgb8 lossless now; 10/12-bit upstream) | gamut-avif container + rawshift-hwdec AV1 |
| HEIC | ✅ hardware | — | gamut-heic container + rawshift-hwdec HEVC |
| DNG | ✅ | ✅ | gamut-dng |
| GIF / SVG / PPM | ✅ | — | `gif` / `resvg` / `zune-ppm` (permanent exceptions) |
| APV | detect-only | — | magic bytes |
| ARW / CR2 / CR3 / CRW / NEF / RAF | ✅ (in-repo) | — | gamut-ifd / gamut-isobmff engines + rawshift tag catalogue (CRW: in-repo CIFF) |

## Feature flags & compile boundaries

Defaults: `jpeg, png, webp, jxl, tiff, gif, ppm`. Formats compose
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
tier-1 target expecting success. Deleted feature axes: all per-implementation
flags, `zune-runtime`, `exif`, `container-embed`, `tiff-parser`,
`heic-vendored`, every `*-vendored` linking flag.

## Hardware decode (rawshift-hwdec)

```rust
pub enum HwCodec  { Hevc, Av1 }
pub enum HwBackend { VideoToolbox, Vaapi, MediaCodec }
pub struct StillDecodeRequest<'a> { codec, config /* hvcC | av1C */,
    payload /* NAL units | OBUs */, width, height, bit_depth, chroma }
pub struct DecodedFrame { width, height, bit_depth, planes /* Nv12|P010|I420|I010 */, range }
pub trait HwStillDecoder: Send { fn decode_frame(&mut self, req: &StillDecodeRequest) -> Result<DecodedFrame, HwDecodeError>; }
pub fn decoder(codec: HwCodec) -> Option<Box<dyn HwStillDecoder>>;
pub fn backend() -> Option<HwBackend>;
pub fn available_codecs() -> Vec<HwCodec>;
```

- HEIC: gamut-heic parses the container (landed upstream in #238) → hwdec
  decodes items → tile stitch + irot/imir/clap in safe Rust → YCbCr→RGB via
  gamut-color CICP → `RgbImage`.
- AVIF: same pipeline once gamut-avif exposes the container/av1C/OBU handoff
  surface (upstream ask, analogous to #238).
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

zune-jpeg, zune-png, zune-jpegxl, zune-core, jpeg-encoder, vendored jpegli
(+submodule/cc/cmake/bindgen), libwebp-sys, jxl-oxide, direct libjxl glue,
ravif, avif-serialize, libaom-sys, image, libheif-rs, tiff, little_exif,
img-parts, binrw. `build.rs` shrinks to cfg aliases + feature/target
verification.

## Release

git-pinned gamut means rawshift cannot publish to crates.io until gamut
publishes; v1 is a git-consumable release (revisited at the v1 release
issue).
