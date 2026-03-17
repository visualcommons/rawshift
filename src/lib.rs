//! # rawshift
//!
//! A high-performance RAW image processing library with support for multiple
//! camera formats and a full processing pipeline.
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
//! - HEIC (detection only; decode requires a licensed H.265 library)
//! - APV (detection only; no Rust decoder exists yet)
//!
//! ## Quick Start
//!
//! ```no_run
//! use rawshift::formats::RawFile;
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
//! - `serde`: Enable serialization for metadata types
//! - `avif`: Enable AVIF encode (via `ravif`) and decode (via `dav1d`)
//! - `jxl-encode`: Enable JXL encoding
//! - `svg`: Enable SVG decoding (requires `resvg`)

pub(crate) mod codecs;
pub mod core;
pub mod data;
pub mod error;
pub mod formats;
pub(crate) mod metadata;
pub mod processing;
pub mod tiff;
pub mod transforms;

pub mod prelude;
