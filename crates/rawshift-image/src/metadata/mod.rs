//! Metadata serialization for image export.
//!
//! This module provides utilities for converting `ImageMetadata` into
//! format-specific representations (EXIF, ICC) for embedding in output images.

#[cfg(feature = "exif")]
pub mod exif;
pub mod icc;
#[cfg(feature = "container-embed")]
pub mod xmp;
