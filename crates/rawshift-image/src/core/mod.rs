//! Core types and traits for image processing.
//!
//! These types live in the [`rawshift_core`] crate and are re-exported here so
//! that `rawshift`'s public path `rawshift::core::…` (and internal
//! `crate::core::…` paths) stay stable. [`IccProfile`] is additionally surfaced
//! here from the internal `metadata` module.

pub use rawshift_core::*;

// Re-export IccProfile from the internal metadata module so it remains
// publicly accessible under `core` as before the workspace split.
pub use crate::metadata::icc::IccProfile;
