//! Metadata infrastructure shared by rawshift's image format crates.
#![forbid(unsafe_code)]

#[cfg(feature = "exif")]
pub mod bridge;
#[cfg(feature = "exif")]
pub mod exif;
pub mod icc;
pub mod isobmff;
#[cfg(feature = "avif-encode")]
pub mod xmp;
