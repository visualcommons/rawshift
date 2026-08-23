//! Core types and traits for image processing.
//!
//! Most types live in the [`rawshift_core`] crate and are re-exported here so
//! that `rawshift`'s public path `rawshift::core::…` (and internal
//! `crate::core::…` paths) stay stable. [`RgbImage`] is defined here — it is a
//! stills-only container (see the rawshift-core charter) wrapping gamut's
//! validated `ImageBuf<Rgb16>`. [`IccProfile`] is additionally surfaced here
//! from the internal `metadata` module.

pub use rawshift_core::*;
pub use rawshift_image_core::RgbImage;
// The container-colour resolution used by the per-format decoders, surfaced
// so callers can classify a profile the same way a decode does.
pub use rawshift_image_core::color_resolve::{
    ContainerColor, is_convertible_to_srgb, resolve_color,
};

// Re-export IccProfile from the internal metadata module so it remains
// publicly accessible under `core` as before the workspace split.
// Its internals are built on `gamut_icc` (metadata-stack migration, #19);
// the wrapper type stays so the container-append API keeps its home.
pub use rawshift_image_metadata::icc::IccProfile;
