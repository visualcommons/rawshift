//! Metadata serialization for image export.
//!
//! This module provides utilities for converting `ImageMetadata` into
//! format-specific representations (EXIF, ICC, XMP) for embedding in output
//! images, and the bridge to gamut's unified `Metadata` model.

#[cfg(feature = "exif")]
#[allow(unused_imports)]
pub use rawshift_image_metadata::bridge;
#[cfg(feature = "exif")]
pub use rawshift_image_metadata::exif;
pub use rawshift_image_metadata::icc;
#[allow(unused_imports)]
pub(crate) use rawshift_image_metadata::isobmff;
// XMP box splicing is only needed by the AVIF encode path — JPEG, PNG, and
// JXL embed XMP through their gamut encoders.
#[cfg(feature = "avif-encode")]
pub use rawshift_image_metadata::xmp;
