//! # rawshift-image
//!
//! Still-image support for rawshift: a high-performance RAW image processing
//! library with support for multiple camera formats and a full processing
//! pipeline, plus standard compressed-format decode/encode.
//!
//! This crate is normally consumed through the [`rawshift`] facade crate
//! (enable its `image` feature). Depend on `rawshift-image` directly when you
//! need per-format Cargo feature control.
//!
//! ## Architecture: gamut-backed
//!
//! rawshift-image is built on the published [gamut] crates, with versions
//! managed centrally in the workspace `Cargo.toml`. gamut owns the generic
//! ground: image primitives (`ImageBuf`, the sealed pixel vocabulary), colour
//! (CICP code points, ICC), container/IFD parsing, the metadata stack
//! (EXIF/ICC/XMP), and the codecs for every migrated format — JPEG, PNG,
//! JPEG XL, AVIF, HEIC, and DNG. rawshift adds what gamut deliberately does
//! not model: the camera/sensor domain (CFA containers, demosaicing, the RAW
//! colour pipeline), vendor tag catalogues, and the high-level API.
//!
//! Hardware still-frame decode of HEVC (HEIC) and AV1 (AVIF) is provided by
//! the `rawshift-hwdec` crate through the `hw`/`hw-*` features.
//!
//! The non-gamut backends that remain are either blocked upstream migrations
//! (`libwebp` for WebP, the `tiff` crate for TIFF) or permanent exceptions
//! (`gif`, `resvg`, `zune-ppm`); see the workspace upstream-first policy.
//!
//! [gamut]: https://github.com/visualcommons/gamut
//! [`rawshift`]: https://docs.rs/rawshift
//!
//! ## Supported Formats
//!
//! ### RAW Formats (full pipeline)
//! - Sony ARW (v1–v5)
//! - Adobe DNG (v1.7, including Apple ProRAW)
//! - Canon CR2
//! - Canon CR3 (metadata + format detection; pixel decode pending CRX codec)
//! - Nikon NEF
//! - Fujifilm RAF
//!
//! ### Standard Formats (direct RGB decode)
//! - GIF, JPEG, PNG, WebP, JPEG XL, TIFF
//! - SVG (requires `svg` feature)
//! - AVIF decode + encode (requires `avif` feature; gamut-avif
//!   container/pipeline — metadata and auxiliary enumeration always work;
//!   pixel decode needs a hardware AV1 decoder via `hw`, else
//!   `RawError::HwDecoderUnavailable`)
//! - HEIC decode (requires `heic` feature; gamut-heic container/pipeline —
//!   metadata and auxiliary enumeration always work; pixel decode needs a
//!   hardware HEVC decoder via `hw`, else `RawError::HwDecoderUnavailable`)
//! - APV (detection only; no Rust decoder exists yet)
//!
//! ## Quick Start
//!
//! ```no_run,ignore
//! // Requires features = ["experimental"]
//! use rawshift_image::formats::RawFile;
//! use std::fs::File;
//!
//! let file = File::open("image.arw").expect("Failed to open file");
//! let raw = RawFile::open(file).expect("Failed to parse RAW file");
//! let metadata = raw.metadata();
//! println!(
//!     "Camera: {} {}",
//!     metadata.camera.make,
//!     metadata.camera.model
//! );
//! ```
//!
//! ## Processing Pipeline
//!
//! Raw images go through these steps:
//! 1. Format decoding (ARW, DNG, CR2, NEF, RAF)
//! 2. Black level subtraction
//! 3. White balance
//! 4. Demosaicing (AMaZE, RCD, LMMSE, Markesteijn)
//! 5. Color matrix application
//! 6. Tone mapping / gamma
//!
//! ## Feature Flags
//!
//! Cargo features are organised in tiers, high-level to low-level. Each tier
//! is defined in terms of the tier below.
//!
//! 1. **Bundles** — `default`, `full`, `experimental`, `raw-stabilizing`,
//!    `raw-incomplete`.
//! 2. **Formats** — `jpeg`, `png`, `webp`, `jxl`, `avif`, `dng`, `gif`, `tiff`,
//!    `heic`, `svg`, `arw`, `cr2`, `cr3`, `crw`, `nef`, `raf` (decode + encode
//!    for that format).
//! 3. **Directions** — `jpeg-decode`, `jpeg-encode`, `arw-decode`, …
//!    gamut-backed direction features pull their `gamut-*` dependency
//!    directly — gamut is the backend, there is no implementation choice.
//! 4. **Implementation aliases** — the six retained flags naming the
//!    non-gamut backends: `gif-decode-gif` / `svg-decode-resvg` /
//!    `ppm-decode-zune` (permanent exceptions) and `tiff-decode-tiff` /
//!    `webp-decode-libwebp` / `webp-encode-libwebp` (blocked upstream
//!    migrations).
//! 5. **Infrastructure** — `ifd-parser`, `exif`, `serde`, and the verified
//!    hardware decode flags `hw` / `hw-videotoolbox` / `hw-vaapi` /
//!    `hw-mediacodec` (see `docs/SUPPORT.md`).
//!
//! See the "Feature Flags" section of the README for the full hierarchy.

pub(crate) mod codecs;
pub mod core;
pub mod data;
pub mod error;
pub mod formats;
pub(crate) mod metadata;
pub mod processing;
pub mod transforms;

pub mod prelude;
