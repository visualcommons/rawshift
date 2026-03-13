//! Core types and traits for image processing.

pub mod image;
pub mod metadata;
pub mod pixel;

pub use image::XTransPattern;
pub use metadata::{ImageMetadata, MetadataExtractor};
pub use pixel::{FromF32, Rgb, Rgb8, Rgb16, RgbF32, Rgba, Rgba8, Rgba16, RgbaF32, Sample};

// Re-export IccProfile from internal metadata module so it's publicly accessible
pub use crate::metadata::icc::IccProfile;
