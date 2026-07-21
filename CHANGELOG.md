# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

v1 is the gamut migration release: rawshift's generic ground — image
primitives, colour, containers, metadata, and the migrated codecs — now comes
from the published [gamut](https://github.com/visualcommons/gamut) crates.
**Release shape:** v1 is publishable to crates.io now that every direct gamut
dependency is a registry package. 0.x source compatibility is a non-goal (see
`docs/V1_DESIGN.md`).

### Changed

All entries below are **breaking**, grouped by area.

#### Core types (`rawshift-core`)

- Generic primitives are gamut re-exports; only the sensor domain stays
  rawshift-owned. `Size` → `gamut_core::Dimensions`; the local `pixel` module
  (`Sample`/`FromF32`/`Rgb<S>`/`Rgba<S>`) → gamut-core's sealed
  `Pixel`/`Sample` traits and marker types (`Rgb16`, `Rgba8`, …) plus
  `ImageBuf`/`ImageRef`; local `BitDepth` → `gamut_color::BitDepth`
  (`#[non_exhaustive]`, serde via the `bit_depth_serde` with-module);
  `ColorSpace` → `ColorDescription`, a CICP (H.273) code-point pair with
  consts `SRGB`/`LINEAR_SRGB`/`DISPLAY_P3`/`REC2020`/`UNSPECIFIED`
  (ICC-authoritative spaces map to `UNSPECIFIED`; the preserved ICC profile
  is the authority); `MetadataExtractor` → `ExtractMetadata` (rename, same
  contract).
- `RgbImage` moved from rawshift-core to rawshift-image as a validated
  wrapper over `gamut_core::ImageBuf<Rgb16>` carrying `ColorDescription`,
  baseline exposure, and default crop. The public `data` field becomes
  `data()`/`data_mut()`; construction is fallible (`new`/`with_color` enforce
  `len == w*h*3`); the mutate-then-`set_size` two-step is replaced by the
  atomic validated `replace_data`.
- New error surface: `RawError::Gamut { context, source: gamut_core::Error }`
  names the rawshift operation that invoked gamut (structured upstream
  context is gamut#254), and `RawError::HwDecoderUnavailable { codec, reason }`
  reports missing hardware decode. Backend-specific error variants
  (`EncodeError::Jpeg`/`Jpegli`/`Jxl`) are gone.

#### Metadata stack

- little_exif → gamut-exif/gamut-icc/gamut-xmp for all EXIF parse/build, ICC
  internals, and XMP validation, plus an `ImageMetadata` ↔
  `gamut_metadata::Metadata` bridge (`to_gamut`/`from_gamut`).
  `ExifParser`'s container-format parameter is now the `ExifContainer` enum;
  EXIF built for embedding widens ISO to LONG instead of truncating past
  u16; XMP appends validate packets and reject malformed input
  (`XmpError::Invalid`) instead of embedding garbage.
- Codec-side metadata extraction: JPEG (APP1/APP2 via `gamut_jpeg::metadata`),
  PNG (eXIf/iCCP/iTXt via `gamut_png`), AVIF/HEIC (container items) now also
  surface ICC and XMP where the old paths read EXIF alone; the hand-rolled
  JPEG APP1 and PNG eXIf scanners and their `ExifContainer` variants are
  deleted. PNG metadata reads now require the `png-decode`/`png-encode`
  feature (previously the bare `exif` feature sufficed).

#### Per-codec migrations (`rawshift-image`)

- **JPEG** → gamut-jpeg (pure Rust, baseline + progressive both ways).
  Decode replaces `zune-jpeg` (CMYK/YCCK conversion bit-identical); encode
  replaces `jpeg-encoder` **and** the vendored jpegli stack with one
  `EncodeOptions::Jpeg(JpegEncodeConfig)` (quality, subsampling, progressive,
  restart interval, JFIF density). Codec id `jpeg/gamut`. Quality `0` no
  longer remaps to 90; default subsampling is 4:2:0. jpegli perceptual
  parity is tracked upstream (gamut#19/#29/#30).
- **PNG** → gamut-png both directions. Decode: every colour type and bit
  depth incl. Adam7, 16-bit native; `DecodeOptions::PngZune(ZunePngDecodeConfig)`
  → `DecodeOptions::Png(PngDecodeConfig)` exposing gamut's hostile-input
  resource guards; the `confirm_crc`/`strict` knobs are gone (critical-chunk
  CRCs always enforced). Encode: `ZunePngEncodeConfig` → `PngEncodeConfig`
  (compression level, filter strategy, `auto_reduce`); output bytes change
  (properly compressed). Codec id `png/gamut` both ways.
- **JPEG XL** → gamut-jxl. Decode replaces `jxl-oxide` (pure Rust);
  `decode_jxl_partial` is removed until truncated-stream decode lands
  upstream (gamut#256); animated and premultiplied-alpha streams are now
  rejected. Encode collapses `zune-jpegxl` and the in-repo libjxl glue onto
  gamut-jxl's encode feature (libjxl vendored hermetically by gamut-jxl-sys);
  one `JxlEncodeConfig` (lossless default, distance, effort, container,
  coded bit depth) — the 16-bit default now encodes true 16-bit lossless.
  Codec id `jxl/gamut`.
- **AVIF encode** → gamut-avif (pure Rust, lossless default + lossy).
  `EncodeOptions::AvifRavif`/`AvifLibaom` collapse to `Avif(AvifEncodeConfig)`;
  10/12-bit encode is temporarily unavailable pending gamut#251
  (`EncodeError::UnsupportedBitDepth`). Codec id `avif/gamut`.
- **AVIF decode** → gamut-avif container + rawshift-hwdec hardware AV1,
  replacing the `image` crate's dav1d-backed path.
  `DecodeOptions::AvifImage` → `Avif(AvifDecodeConfig)` (codec id
  `avif/gamut`); new `AvifFile` API and `avif_hw_decode_available()`.
  Container/metadata/auxiliary enumeration are backend-less; pixel decode
  needs a hardware AV1 decoder (`hw`/`hw-*`) and reports
  `RawError::HwDecoderUnavailable` without one (software fallback is post-v1,
  gamut#259; 10/12-bit RGBA presentation pending gamut#303).
- **HEIC** → gamut-heic container + rawshift-hwdec hardware HEVC, replacing
  libheif. `DecodeOptions::HeicLibheif` → `Heic(HeicDecodeConfig)` (codec id
  `heic/gamut`); new `HeicFile` API and `heic_hw_decode_available()`; same
  backend-less container/metadata contract and `HwDecoderUnavailable`
  behaviour as AVIF.
- **DNG** → gamut-dng both directions: uncompressed, Deflate, lossless JPEG,
  and DNG 1.7 JPEG XL (iPhone ProRAW) raw data all decode through gamut;
  export rebuilt on `gamut_dng::DngEncoder`. `DngExportConfig` →
  `DngEncodeConfig`. DNG 1.7 JXL raw data reports decoded bit depth 16
  (full-range, reference-SDK semantics).
- **ARW/CR2/NEF/CR3** IFD walking → gamut-ifd (CR3's ISOBMFF box walker stays
  in-repo pending gamut#301). New numeric `ParseError::MissingTag(u16)`
  replaces the `TiffTag`-typed variant; corrupt top-level value offsets now
  fail at parse instead of on first touch.
- **v1 API finalization** (#37): the option enums are strictly format-keyed —
  `DecodeOptions::WebpLibwebp`/`SvgResvg`/`PpmZune` →
  `WebP`/`Svg`/`Ppm`, and `EncodeOptions::PngGamut`/`WebpLibwebp` →
  `Png`/`WebP`. Backend names survive only in the config struct names of the
  un-migrated/exception backends (`LibwebpDecodeConfig`,
  `LibwebpEncodeConfig`, `TiffDecodeConfig`, `GifDecodeConfig`,
  `ResvgDecodeConfig`, `ZunePpmDecodeConfig`). The never-wired "planned
  backend" config structs (`LibjpegTurboEncodeConfig`, `MozjpegEncodeConfig`,
  `SvtAv1EncodeConfig`) are deleted — alternative encoder backends contradict
  the upstream-first policy; encoder improvements go to gamut.

#### Features & infrastructure

- MSRV raised to 1.92 (required by the gamut dependency pin).
- The five-tier feature hierarchy is consolidated: gamut-backed direction
  features pull their `gamut-*` dependency directly, deleting the
  per-implementation flags of every migrated format. Deleted feature axes:
  `container-embed`, `tiff-parser` (replaced by `ifd-parser` over gamut-ifd),
  `heic-vendored`, and every `*-vendored` linking flag. Retained tier-4
  aliases: `gif-decode-gif`/`svg-decode-resvg`/`ppm-decode-zune` (permanent
  exceptions) and `tiff-decode-tiff`/`webp-decode-libwebp`/
  `webp-encode-libwebp` (blocked migrations: gamut#299/#300 via rawshift#22,
  gamut#302 via rawshift#24).
- `--all-features` is no longer a valid build invocation by design: the
  hardware backend pins (`hw-videotoolbox`/`hw-vaapi`/`hw-mediacodec`) are
  mutually exclusive verified feature flags. Use `--features full`.
- `rawshift-video` is parked as an unpublished placeholder: no `video`
  feature on the facade until video has an implementation.
- `build.rs` shrinks to cfg-alias machinery and feature/target verification
  only (no more bindgen/cc/cmake/pkg-config build-dependency slice).

### Added

- *(hwdec)* New `rawshift-hwdec` crate: the workspace's hardware still-frame
  decode boundary (HEVC + AV1) with a safe public API (`HwCodec`,
  `HwBackend`, `CodecConfig`/`StillDecodeRequest`, validated `DecodedFrame`,
  `HwStillDecoder`, `decoder()`/`backend()`/`available_codecs()`), verified
  feature flags (`hw` portable; `videotoolbox`/`vaapi`/`mediacodec`
  `compile_error!` on foreign targets), and all platform FFI confined to it.
- *(hwdec)* VAAPI hardware decode backend (linux-gnu): HEVC Main/Main10 and
  AV1 Profile 0 still pictures to NV12/P010, libva dlopen'd at runtime —
  absence of libva or a render node degrades to `decoder() == None`, never a
  link failure. NVIDIA is covered via the `nvidia-vaapi-driver` translation
  layer (see `docs/SUPPORT.md`). HEIC and AVIF pixel decode in
  `rawshift-image` work end-to-end on hardware through this backend.
- *(image)* `RawFile::format()` — the detected `RawFormat` of an opened RAW
  file (#37).
- *(image)* `AvifFile` and `HeicFile` container APIs: open / metadata /
  thumbnail / auxiliary (alpha, depth, gain map) enumeration / decode, plus
  the `avif_hw_decode_available()` / `heic_hw_decode_available()` runtime
  probes and `MetadataNamespace::Avif` container facts.
- *(image)* gamut consumed as versioned crates.io dependencies managed in the
  workspace `Cargo.toml` (see the upstream-first policy in `AGENTS.md` and the
  update procedure in the README).
- *(bench)* First codec benches: JPEG and PNG encode+decode round-trips on
  the gamut backends, plus gated HEIC/AVIF hardware-decode benches that
  self-generate fixtures via heif-enc/avifenc and skip cleanly without the
  tools or a GPU (`docs/BENCHMARKS.md` records the post-migration baseline).
- *(examples)* `avif_inspect` example; all examples verified against the
  migrated backends with `--features full`.
- *(ci)* Compile-boundary jobs: invalid feature × target combinations must
  fail with the exact `compile_error!` text; `--features full` builds on the
  hosted tier-1 targets.

### Removed

The migration dependency graveyard — all **breaking** where a public API
hung off them:

- *(deps, codecs)* `zune-jpeg`, `zune-png`, `zune-jpegxl`, `jxl-oxide`,
  `jpeg-encoder`, `ravif`, `libaom-sys`, `avif-serialize`, `libheif-rs`,
  `image` (with its dav1d subtree leaving the lockfile), and the direct
  `jpegxl-src` dependency (it remains only as gamut-jxl-sys's own pinned
  build-dependency). `zune-core`/`zune-ppm` stay — PPM is a permanent
  exception; `libwebp-sys` and `tiff` stay pending their blocked upstream
  migrations (rawshift#24, rawshift#22); `gif` and `resvg` are permanent
  exceptions.
- *(deps, metadata/containers)* `little_exif`, `img-parts`, `binrw` — with
  the whole in-repo binrw TIFF layer: the `tiff/` module and its public
  `TiffParser`/`TiffWriter`/`TiffValue`/`TiffTag` API.
- *(build)* The vendored `google/jpegli` submodule (and `.gitmodules`), the
  in-repo libjxl bindgen glue, and the `bindgen`/`cc`/`cmake`/`pkg-config`
  build-dependencies.
- *(image)* `decode_jxl_partial` (until gamut#256); the hand-rolled JPEG
  APP1 and PNG eXIf scanners; `ExifContainer::Jpeg`/`Png`/`Avif` read paths;
  the never-implemented "planned backend" encode config structs.

### Fixed

- The synthesised sRGB ICC profile's `desc` element was a malformed
  `textDescriptionType`; the gamut-icc-built profile is spec-valid (pinned by
  a test parsing back colorants and TRC).
- Serialising empty/degenerate EXIF metadata no longer panics: gamut-exif
  emits a valid empty-IFD TIFF stream where little_exif panicked on this
  input.
- `CFAPattern` is now also accepted from UNDEFINED-typed IFD fields
  (previously BYTE only, silently defaulting to RGGB).

## [0.1.1](https://github.com/visualcommons/rawshift/compare/v0.1.0...v0.1.1) - 2026-05-29

### Added

- *(image)* add libjxl as an optional JPEG XL encoder backend

### Other

- move format and feature docs to per-crate READMEs
- add the per-crate README files

## [0.1.0](https://github.com/visualcommons/rawshift/releases/tag/v0.1.0) - 2026-05-29

Initial release.

### Added

- Cargo workspace layout: the `rawshift` facade crate plus `rawshift-core`
  (shared geometry, pixel, and metadata types), `rawshift-image` (still-image
  decoding, RAW processing, and encoding), and a `rawshift-video` placeholder.
- Still-image decoding with explicit, per-implementation decoder backend
  selection and configuration.
- PPM / Netpbm support via `zune-ppm`.
- HEIC / HEIF decode support via libheif.
- Layered RAW feature flags organized into a decode / encode / tier hierarchy.

### Build

- Workspace MSRV set to 1.90.0.
- Workspace-aware `justfile`, CI matrix, and lefthook pre-commit/pre-push hooks.
