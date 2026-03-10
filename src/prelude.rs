//! Convenience re-exports of the most commonly used types and functions.
//!
//! Import everything at once with `use rawshift::prelude::*;`.
//!
//! # Contents
//!
//! - **`core`** — `RawImage`, `RgbImage`, `Size`, `Rect`, `Point`, `CfaPattern`,
//!   `ImageMetadata`, and related structs.
//! - **`data`** — Camera color-calibration database (`CameraCalibration`,
//!   `get_camera_calibration`, `all_cameras`).
//! - **`error`** — `RawError` and `RawResult`.
//! - **`formats`** — `RawFile`, `RawFormat`, `StandardFormat`,
//!   `decode_standard_image`, `detect_standard_format`, `DngExportConfig`.
//! - **`processing`** — `ProcessingOptions` and demosaicing types.
//! - **`tiff`** — `TiffParser`, `TiffTag`, `TiffValue`, and related TIFF types.
//! - **`transforms`** — `apply_black_level`, `apply_white_balance`,
//!   `apply_white_balance_raw`, `apply_color_matrix`, `apply_tone_reproduction`,
//!   `apply_tonemap`, `compute_camera_to_srgb`, `ColorSpaceTransform`, and more.

pub use crate::core::*;
pub use crate::data::*;
pub use crate::error::*;
pub use crate::formats::*;
pub use crate::processing::*;
pub use crate::tiff::*;
pub use crate::transforms::{
    BadPixelCorrectionMode, ColorSpaceTransform, ColorTemperature, apply_bad_pixel_correction,
    apply_bilateral_filter, apply_black_level, apply_ca_correction, apply_color_matrix,
    apply_gains_rgb, apply_gaussian_blur, apply_matrix_rgb, apply_tone_reproduction, apply_tonemap,
    apply_white_balance, apply_white_balance_raw, compute_camera_to_srgb, correct_bad_pixels,
    detect_bad_pixels, estimate_cct_from_as_shot_neutral, interpolate_color_matrix,
    subtract_black_level_uniform,
};
