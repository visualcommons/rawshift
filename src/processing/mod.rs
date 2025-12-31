//! Pixel processing and color space conversion primitives

pub mod color;
pub mod demosaic;
pub mod options;
// pub mod simd; // TODO: Skip for now, accelerate regular implementations with SIMD (AVX2, AVX512, NEON, etc.)

pub use options::{DemosaicAlgorithm, ProcessingOptions};
