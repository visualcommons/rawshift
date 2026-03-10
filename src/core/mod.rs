//! Core types and traits for image processing.

pub mod image;
pub mod metadata;
pub mod pixel;

pub use metadata::{ImageMetadata, MetadataExtractor};
pub use pixel::{FromF32, Rgb, Rgb8, Rgb16, RgbF32, Rgba, Rgba8, Rgba16, RgbaF32, Sample};
