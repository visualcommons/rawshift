//! Metadata serialization for image export.
//!
//! This module provides utilities for converting `ImageMetadata` into
//! format-specific representations (EXIF, ICC, XMP) for embedding in output
//! images, and the bridge to gamut's unified `Metadata` model.

#[cfg(feature = "exif")]
pub mod bridge;
#[cfg(feature = "exif")]
pub mod exif;
pub mod icc;
pub(crate) mod isobmff;
// XMP box splicing is only needed by the AVIF encode path — JPEG, PNG, and
// JXL embed XMP through their gamut encoders.
#[cfg(feature = "avif-encode")]
pub mod xmp;
