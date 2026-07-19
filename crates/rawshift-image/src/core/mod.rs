//! Core types and traits for image processing.
//!
//! Most types live in the [`rawshift_core`] crate and are re-exported here so
//! that `rawshift`'s public path `rawshift::core::…` (and internal
//! `crate::core::…` paths) stay stable. [`RgbImage`] is defined here — it is a
//! stills-only container (see the rawshift-core charter) wrapping gamut's
//! validated `ImageBuf<Rgb16>`. [`IccProfile`] is additionally surfaced here
//! from the internal `metadata` module.

mod rgb_image;

pub use rawshift_core::*;
pub use rgb_image::RgbImage;

// Re-export IccProfile from the internal metadata module so it remains
// publicly accessible under `core` as before the workspace split.
// The type is replaced by `gamut_icc::IccProfile` in the metadata-stack
// migration (#19), which owns the icc.rs internals wholesale.
pub use crate::metadata::icc::IccProfile;
