//! Low-level TIFF engine.
//!
//! This module provides the core TIFF parsing and writing functionality used by
//! all TIFF-based RAW formats (ARW, CR2, DNG, etc.).
//!
//! # Structure
//!
//! - [`parser`] - IFD parsing and navigation
//! - [`tags`] - Known TIFF tag definitions
//! - [`types`] - TIFF data types and values
//! - [`writer`] - TIFF/DNG file writing

pub mod metadata_helper;
mod parser;
mod tags;
mod types;
pub mod writer;

pub use parser::*;
pub use tags::*;
pub use types::*;
pub use writer::{IfdEntry, TiffWriter};
