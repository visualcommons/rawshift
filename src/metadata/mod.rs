//! Metadata serialization for image export.
//!
//! This module provides utilities for converting `ImageMetadata` into
//! format-specific representations (EXIF, ICC) for embedding in output images.

pub mod exif;
pub mod icc;
pub mod xmp;
