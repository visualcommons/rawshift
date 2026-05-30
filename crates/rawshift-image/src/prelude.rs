//! Convenience re-exports of the most commonly used types and functions.
//!
//! Import everything at once with `use rawshift::prelude::*;`.
//!
//! # Contents
//!
//! - **`core`** — `RawImage`, `RgbImage`, `Size`, `Rect`, `Point`, `CfaPattern`,
//!   `ImageMetadata`, `ColorSpace`, `BitDepth`, `CodecInfo`, the generic metadata
//!   model (`MetadataValue`, `MetadataKey`, `MetadataNamespace`, `MetadataEntry`),
//!   and related structs.
//! - **`data`** — Camera color-calibration database (`CameraCalibration`,
//!   `get_camera_calibration`, `all_cameras`).
//! - **`error`** — `RawError`, `ParseError`, `FormatError`, `ProcessingError`,
//!   `EncodeError`, and `RawResult`.
//! - **`formats`** — `RawFile`, `RawFormat`, `StandardFormat`,
//!   `decode_standard_image`, `detect_standard_format`,
//!   `read_standard_image_metadata`, `DngExportConfig`, `EncodeOptions`,
//!   `CommonEncodeOptions`, `OutputFormat`, and the per-backend encode configs.
//! - **`processing`** — `ProcessingOptions` and demosaicing types.
//! - **`tiff`** — `TiffParser`, `TiffTag`, `TiffValue`, and related TIFF types.
//! - **`transforms`** — `apply_black_level`, `apply_white_balance`,
//!   `apply_white_balance_raw`, `apply_color_matrix`, `apply_tone_reproduction`,
//!   `apply_tonemap`, `compute_camera_to_srgb`, `ColorSpaceTransform`, and more.

// core
pub use crate::core::image::{CfaPattern, RawImage, Rect, RgbImage, Size, XTransPattern};
pub use crate::core::metadata::{
    CameraInfo, DateTimeInfo, DngCalibrationInfo, DngColorInfo, DngProfileInfo, ExifInfo, GpsInfo,
    ImageInfo, ImageMetadata, MetadataEntry, MetadataExtractor, MetadataKey, MetadataNamespace,
    MetadataValue,
};
pub use crate::core::pixel::{
    FromF32, Rgb, Rgb8, Rgb16, RgbF32, Rgba, Rgba8, Rgba16, RgbaF32, Sample,
};
pub use crate::core::{CodecDirection, CodecId, CodecInfo, ColorSpace, IccProfile};

// data
pub use crate::data::cameras::find_camera_calibration;
#[allow(deprecated)]
pub use crate::data::cameras::{CameraCalibration, all_cameras, get_camera_calibration};

// error
pub use crate::error::{
    EncodeError, FormatError, ParseError, ProcessingError, RawError, RawResult,
};

// formats — encode option system
pub use crate::formats::export::{
    BitDepth, CommonEncodeOptions, EncodeOptions, JpegEncEncodeConfig, JpegSubsampling,
    JpegliEncodeConfig, LibjxlColorTransform, LibjxlEncodeConfig, LibjxlModular,
    LibwebpEncodeConfig, MetadataEmbedOptions, OutputFormat, RavifEncodeConfig, WebPMode,
    ZuneJxlEncodeConfig, ZunePngEncodeConfig,
};
// formats — decoders, format detection, encode/decode entry points
pub use crate::formats::{
    DecodeOptions, GifDecodeConfig, ImageAvifDecodeConfig, ImageProbe, JxlOxideDecodeConfig,
    LibheifDecodeConfig, LibwebpDecodeConfig, ResvgDecodeConfig, StandardFormat, TiffDecodeConfig,
    ZuneJpegDecodeConfig, ZunePngDecodeConfig, available_decoders, available_encoders,
    decode_standard_image, decode_standard_image_with, detect_standard_format, encode_rgb_image,
    encode_rgb_image_to_vec, encode_rgb_image_to_writer, probe_standard_image,
    read_standard_image_metadata,
};

#[cfg(feature = "jxl-decode")]
pub use crate::formats::decode_jxl_partial;

#[cfg(any_raw)]
pub use crate::formats::{RawFile, RawFormat};

#[cfg(feature = "heic-decode")]
pub use crate::formats::{HeicAuxImage, HeicAuxKind, HeicFile};

#[cfg(feature = "dng-encode")]
pub use crate::formats::{DngExportConfig, export_dng};

// processing
pub use crate::processing::{
    BayerAlgorithm, Demosaic, DemosaicMethod, ProcessingOptions, XTransAlgorithm,
};

// tiff
#[cfg(feature = "tiff-parser")]
pub use crate::tiff::{
    ByteOrder, Ifd, IfdEntry, Rational, SRational, TiffParser, TiffTag, TiffValue, TiffWriter,
};

// transforms
pub use crate::transforms::{
    BadPixelCorrectionMode, ColorSpaceTransform, ColorTemperature, apply_bad_pixel_correction,
    apply_bilateral_filter, apply_black_level, apply_ca_correction, apply_color_matrix, apply_crop,
    apply_gains_rgb, apply_gaussian_blur, apply_matrix_rgb, apply_orientation,
    apply_tone_reproduction, apply_tonemap, apply_white_balance, apply_white_balance_raw,
    compute_camera_to_srgb, convert_to_srgb, correct_bad_pixels, detect_bad_pixels,
    estimate_cct_from_as_shot_neutral, flip_horizontal, flip_vertical, interpolate_color_matrix,
    rotate_90_ccw, rotate_90_cw, rotate_180, subtract_black_level_uniform,
};
