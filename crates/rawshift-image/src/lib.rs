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
//! - AVIF decode + encode (requires `avif` feature)
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
//! Cargo features are organised in five tiers, high-level to low-level. Each
//! tier is defined in terms of the tier below; only tier-4 features (and RAW
//! tier-3 features) pull in an external crate.
//!
//! 1. **Bundles** — `default`, `full`, `experimental`, `raw-stabilizing`,
//!    `raw-incomplete`.
//! 2. **Formats** — `jpeg`, `png`, `webp`, `jxl`, `avif`, `dng`, `gif`, `tiff`,
//!    `heic`, `svg`, `arw`, `cr2`, `cr3`, `crw`, `nef`, `raf` (decode + encode
//!    for that format).
//! 3. **Directions** — `jpeg-decode`, `jpeg-encode`, `arw-decode`, … For
//!    compressed formats a direction feature aliases the **default**
//!    implementation; RAW formats (and the gamut-backed JPEG/PNG halves) have
//!    a single implementation.
//! 4. **Implementations** — compressed formats only, named
//!    `format-direction-impl` (e.g. `ppm-decode-zune`). Multiple may be enabled
//!    at once; the active backend is chosen via [`formats::DecodeOptions`] and
//!    [`formats::export::EncodeOptions`].
//! 5. **Infrastructure** — `ifd-parser`, `serde`, and the verified hardware
//!    decode flags `hw` / `hw-videotoolbox` / `hw-vaapi` / `hw-mediacodec`
//!    (see `docs/SUPPORT.md`).
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
