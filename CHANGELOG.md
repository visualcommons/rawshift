# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- *(hwdec)* VAAPI hardware decode backend (linux-gnu): HEVC Main/Main10 and
  AV1 Profile 0 still pictures to NV12/P010, libva dlopen'd at runtime —
  absence of libva or a render node degrades to `decoder() == None`, never a
  link failure. NVIDIA is covered via the `nvidia-vaapi-driver` translation
  layer (see `docs/SUPPORT.md`). HEIC pixel decode in `rawshift-image` now
  works end-to-end on hardware through this backend.

### Changed

- *(image)* **breaking**: JPEG migrated to gamut-jpeg (pure Rust, baseline +
  progressive both ways). Decode replaces `zune-jpeg` (`DecodeOptions::Jpeg`,
  codec id `jpeg/gamut`; CMYK/YCCK conversion is bit-identical to the previous
  backend); encode replaces `jpeg-encoder` **and** the entire jpegli stack
  with one `EncodeOptions::Jpeg(JpegEncodeConfig)` exposing gamut's knobs
  (quality, subsampling, progressive, restart interval, JFIF density).
  JPEG EXIF/XMP/ICC now embed via the encoder and extract via
  `gamut_jpeg::metadata` (JPEG metadata reads now also surface ICC + XMP);
  `probe_standard_image` reads the JPEG header through `gamut_jpeg::info`.

### Removed

- *(image)* **breaking**: the `jpeg-decode-zune`, `jpeg-encode-jpeg-enc`,
  `jpeg-encode-jpegli`, `jpeg-encode-jpegli-vendored`, and `container-embed`
  features; the `zune-jpeg`, `jpeg-encoder`, and `img-parts` dependencies; the
  vendored `google/jpegli` submodule and the cc/cmake/bindgen build-script
  slice; `EncodeError::Jpeg`/`EncodeError::Jpegli`; `ExifContainer::Jpeg` and
  the hand-rolled JPEG APP1 scanner (jpegli parity is tracked upstream:
  gamut#19/#29/#30).

## [0.1.1](https://github.com/justin13888/rawshift/compare/v0.1.0...v0.1.1) - 2026-05-29

### Added

- *(image)* add libjxl as an optional JPEG XL encoder backend

### Other

- move format and feature docs to per-crate READMEs
- add the per-crate README files

## [0.1.0](https://github.com/justin13888/rawshift/releases/tag/v0.1.0) - 2026-05-29

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
