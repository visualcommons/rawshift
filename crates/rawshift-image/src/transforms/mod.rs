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

pub mod bad_pixel;
pub mod black_level;
pub mod ca_correction;
pub mod cfa;
pub mod color;
pub mod denoise;
pub mod lens_correction;
pub mod opcodes;
pub mod orientation;
pub mod simd;
pub mod tonemap;

pub use bad_pixel::{
    BadPixelCorrectionMode, apply_bad_pixel_correction, correct_bad_pixels, detect_bad_pixels,
};
pub use black_level::apply_black_level;
pub use ca_correction::apply_ca_correction;
pub use color::{
    ColorSpaceTransform, ColorTemperature, apply_color_matrix, apply_white_balance,
    apply_white_balance_raw, compute_camera_to_srgb, estimate_cct_from_as_shot_neutral,
    interpolate_color_matrix,
};
pub use denoise::{apply_bilateral_filter, apply_gaussian_blur};
pub use lens_correction::{apply_warp_rectilinear, apply_warp_rectilinear_tangential};
pub use orientation::{
    apply_crop, apply_orientation, flip_horizontal, flip_vertical, rotate_90_ccw, rotate_90_cw,
    rotate_180,
};
pub use simd::{apply_gains_rgb, apply_matrix_rgb, subtract_black_level_uniform};
pub use tonemap::{apply_tone_reproduction, apply_tonemap};
