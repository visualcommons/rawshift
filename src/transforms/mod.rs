//! Image transformation and processing pipeline.
//!
//! This module provides the core building blocks for transforming raw sensor data
//! (like Bayer CFA) into viewable RGB images. These transforms are generic and
//! designed to support various RAW formats (ARW, DNG, CR2, NEF, etc.).
//!
//! # General RAW Processing Flow
//! Most RAW formats follow a similar processing pipeline:
//!
//! 1. **Sensor Normalization**:
//!    - **Black Level**: Subtracting the sensor noise floor (pedestal).
//!    - **White Level**: Normalizing signal to a standard range (0.0 - 1.0).
//!    - See [`black_level`].
//!
//! 2. **Demosaicing**:
//!    - Converting incomplete Color Filter Array (CFA) data (e.g., Bayer) into
//!      fully populated RGB pixels.
//!    - See [`cfa`].
//!
//! 3. **Color Processing**:
//!    - **White Balance**: Adjusting gains for Red/Blue channels to neutralize color casts.
//!    - **Color Transformation**: Converting from Camera Native RGB space to a standard
//!      connection space (like CIE XYZ) and then to an output space (like sRGB or ProPhoto).
//!    - See [`color`].
//!
//! 4. **Post-Processing & Correction**:
//!    - **Tone Mapping/Gamma**: Applying gamma curves for display or HDR compression.
//!    - **Corrections**: Lens distortion, vignetting, and other optical fixes (often handled
//!      via opcodes in formats like DNG).
//!    - See [`tonemap`] and [`opcodes`].

pub mod black_level;
pub mod cfa;
pub mod color;
pub mod opcodes;
pub mod tonemap;

pub use black_level::apply_black_level;
pub use color::{
    ColorSpaceTransform, apply_color_matrix, apply_white_balance, apply_white_balance_raw,
    compute_camera_to_srgb,
};
pub use tonemap::{apply_tone_reproduction, apply_tonemap};
