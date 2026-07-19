//! # rawshift
//!
//! `rawshift` is a facade crate. It re-exports the workspace's image library
//! behind coarse feature flags so most consumers can depend on a single crate:
//!
//! - **`image`** (default) — re-exports [`rawshift-image`]: RAW decoding for
//!   Sony/Canon/Nikon/Fujifilm/Adobe, standard formats (JPEG, PNG, WebP, JXL,
//!   GIF, TIFF, AVIF, HEIC, SVG), the full RAW processing pipeline, and
//!   encoding. Everything appears at the crate root, e.g. [`formats`],
//!   [`core`], [`processing`], [`transforms`], [`prelude`].
//!
//! ## Video is parked for v1
//!
//! rawshift v1 ships **image only**. `rawshift-video` remains in the workspace
//! as an unpublished placeholder holding the roadmap for post-v1 work, but it
//! is not a dependency of this facade and there is no `video` feature. A
//! feature that gates zero code is a promise the workspace cannot keep, and a
//! published facade cannot depend on an unpublished crate. Both are re-added
//! when video has an implementation.
//!
//! ## Feature flags
//!
//! This facade exposes only `image`, `serde`, the hardware-decode flags
//! (`hw`, `hw-videotoolbox`, `hw-vaapi`, `hw-mediacodec`), and `full`. It does
//! **not** surface per-format flags — Cargo cannot auto-forward a child crate's
//! features, so re-listing them here would be duplicated, rot-prone state. For
//! fine-grained control (individual formats or directions) depend on
//! [`rawshift-image`] directly; its own feature tree is documented on that
//! crate.
//!
//! ```no_run
//! // Default `image` feature is enough for standard-format decoding.
//! use rawshift::formats::{decode_standard_image, detect_standard_format};
//!
//! let bytes = std::fs::read("photo.jpg").expect("read");
//! let format = detect_standard_format(&bytes).expect("detect");
//! let image = decode_standard_image(&bytes, format).expect("decode");
//! println!("{format:?}: {}x{}", image.width(), image.height());
//! ```
//!
//! [`rawshift-image`]: https://docs.rs/rawshift-image
#![forbid(unsafe_code)]

#[cfg(feature = "image")]
pub use rawshift_image::*;
