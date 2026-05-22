//! # rawshift
//!
//! `rawshift` is a facade crate. It re-exports the workspace's image and video
//! libraries behind two coarse feature flags so most consumers can depend on a
//! single crate:
//!
//! - **`image`** (default) — re-exports [`rawshift-image`]: RAW decoding for
//!   Sony/Canon/Nikon/Fujifilm/Adobe, standard formats (JPEG, PNG, WebP, JXL,
//!   GIF, TIFF, AVIF, HEIC, SVG), the full RAW processing pipeline, and
//!   encoding. Everything appears at the crate root, e.g. [`formats`],
//!   [`core`], [`processing`], [`transforms`], [`prelude`].
//! - **`video`** — re-exports `rawshift-video` as [`video`]. Video support is
//!   planned but not yet implemented.
//!
//! ## Feature flags
//!
//! This facade exposes only `image`, `video`, `serde`, and `full`. It does
//! **not** surface per-format flags — Cargo cannot auto-forward a child crate's
//! features, so re-listing them here would be duplicated, rot-prone state. For
//! fine-grained control (individual formats, alternative codec backends, the
//! `tiff-parser` API, `heic-vendored` linking) depend on [`rawshift-image`]
//! directly; its own five-tier feature system is documented on that crate.
//!
//! ```no_run,ignore
//! // Default `image` feature is enough for standard-format decoding.
//! use rawshift::formats::{decode_standard_image, detect_standard_format};
//!
//! let bytes = std::fs::read("photo.jpg").expect("read");
//! let format = detect_standard_format(&bytes).expect("detect");
//! let image = decode_standard_image(&bytes).expect("decode");
//! println!("{format:?}: {}x{}", image.width(), image.height());
//! ```
//!
//! [`rawshift-image`]: https://docs.rs/rawshift-image
#![forbid(unsafe_code)]

#[cfg(feature = "image")]
pub use rawshift_image::*;

#[cfg(feature = "video")]
pub use rawshift_video as video;
