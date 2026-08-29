//! # rawshift
//!
//! `rawshift` is a facade crate. It re-exports the workspace's image and
//! video libraries behind coarse feature flags so most consumers can depend on
//! a single crate:
//!
//! - **`image`** (default) — re-exports [`rawshift-image`] **at the crate
//!   root**: RAW decoding for Sony/Canon/Nikon/Fujifilm/Adobe, standard
//!   formats (JPEG, PNG, WebP, JXL, GIF, TIFF, AVIF, HEIC, SVG), the full RAW
//!   processing pipeline, and encoding. Everything appears at the root, e.g.
//!   [`formats`], [`core`], [`processing`], [`transforms`], [`prelude`].
//! - **`video`** — re-exports [`rawshift-video`] under [`video`]: MP4, M4V,
//!   QuickTime, Matroska and WebM containers, tracks, timelines and metadata,
//!   plus hardware-backed frame decode.
//!
//! Video lives under a module rather than at the root because the two
//! libraries share names — `prelude`, `Track`, and their error types — and
//! flattening both would collide. Image stays at the root for source
//! compatibility.
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
//! ```no_run
//! # #[cfg(feature = "video")] {
//! use rawshift::video::prelude::*;
//!
//! let file = VideoFile::open(std::fs::File::open("clip.mp4").expect("open")).expect("read");
//! for track in file.video_tracks() {
//!     println!("{} {}x{}", track.codec, track.dimensions.width, track.dimensions.height);
//! }
//! # }
//! ```
//!
//! ## What works without a hardware decoder
//!
//! Container parsing, track enumeration, timelines and metadata work on every
//! target in `docs/SUPPORT.md`, including those with no hardware decode API at
//! all. Only frame decode needs a backend, and its absence is reported as a
//! matchable capability gap rather than a failure. See `docs/SUPPORT.md`.
//!
//! ## Feature flags
//!
//! This facade exposes only `image`, `video`, `serde`, the hardware-decode
//! flags (`hw`, `hw-videotoolbox`, `hw-vaapi`, `hw-mediacodec`), and `full`.
//! It does **not** surface per-format flags — Cargo cannot auto-forward a
//! child crate's features, so re-listing them here would be duplicated,
//! rot-prone state. For fine-grained control (individual formats, containers,
//! codecs, or directions) depend on [`rawshift-image`] or [`rawshift-video`]
//! directly; their own feature trees are documented on those crates.
//!
//! [`rawshift-image`]: https://docs.rs/rawshift-image
//! [`rawshift-video`]: https://docs.rs/rawshift-video
#![forbid(unsafe_code)]

#[cfg(feature = "image")]
pub use rawshift_image::*;

/// Video containers, tracks, metadata and decoding.
///
/// The whole of [`rawshift-video`](https://docs.rs/rawshift-video); see that
/// crate for the full API.
#[cfg(feature = "video")]
pub mod video {
    pub use rawshift_video::*;
}
