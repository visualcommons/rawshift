//! Shared core types for the rawshift image/video processing libraries.
//!
//! This crate holds pure, stateless data structures with no decoding logic:
//! geometry ([`image::Size`], [`image::Point`], [`image::Rect`]), pixel sample
//! types ([`pixel`]), CFA patterns, the raw/RGB image containers, and the
//! format-agnostic [`metadata`] model. It is depended on by both
//! `rawshift-image` and `rawshift-video` so they share one vocabulary of types
//! without either pulling in the other.
#![forbid(unsafe_code)]

pub mod image;
pub mod metadata;
pub mod pixel;

pub use image::XTransPattern;
pub use metadata::{
    ImageMetadata, MetadataEntry, MetadataExtractor, MetadataKey, MetadataNamespace, MetadataValue,
};
pub use pixel::{FromF32, Rgb, Rgb8, Rgb16, RgbF32, Rgba, Rgba8, Rgba16, RgbaF32, Sample};
