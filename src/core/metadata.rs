//! Unified metadata types for image formats.
//!
//! This module provides format-agnostic metadata structures that represent
//! a superset of all metadata that can exist in any supported RAW format.
//! Extend as new formats reveal additional fields.

/// Unsigned rational (numerator, denominator).
pub use crate::tiff::Rational as URational;

/// Signed rational (numerator, denominator).
pub use crate::tiff::SRational;

/// Camera identification information.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CameraInfo {
    /// Camera manufacturer (e.g., "SONY", "Apple")
    pub make: String,
    /// Camera model (e.g., "ILCE-6700", "iPhone 17 Pro Max")
    pub model: String,
    /// DNG UniqueCameraModel identifier
    pub unique_camera_model: Option<String>,
    /// Lens manufacturer
    pub lens_make: Option<String>,
    /// Lens model name
    pub lens_model: Option<String>,
    /// Lens info: [MinFL, MaxFL, MinFNum, MaxFNum]
    pub lens_info: Option<[f64; 4]>,
    /// Camera serial number
    pub serial_number: Option<String>,
}

/// EXIF exposure and capture settings.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExifInfo {
    /// ISO sensitivity
    pub iso: Option<u32>,
    /// Exposure time in seconds (num/denom)
    pub exposure_time: Option<URational>,
    /// F-number (num/denom)
    pub f_number: Option<URational>,
    /// Focal length in mm (num/denom)
    pub focal_length: Option<URational>,
    /// 35mm equivalent focal length
    pub focal_length_35mm: Option<u16>,
    /// Exposure program (EXIF enum)
    pub exposure_program: Option<u16>,
    /// Metering mode (EXIF enum)
    pub metering_mode: Option<u16>,
    /// Flash status (EXIF enum)
    pub flash: Option<u16>,
    /// Exposure compensation in EV (num/denom)
    pub exposure_compensation: Option<SRational>,
    /// Maximum aperture value (num/denom)
    pub max_aperture: Option<URational>,
    /// Brightness value (num/denom)
    pub brightness_value: Option<SRational>,
}

/// Date/time information.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DateTimeInfo {
    /// Original capture date/time (EXIF format: "YYYY:MM:DD HH:MM:SS")
    pub datetime_original: Option<String>,
    /// Digitization date/time
    pub create_date: Option<String>,
    /// Last modification date/time
    pub modify_date: Option<String>,
    /// Timezone offset (e.g., "-05:00")
    pub offset_time: Option<String>,
    /// Sub-second time precision
    pub subsec_time: Option<String>,
}

/// GPS geolocation data.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GpsInfo {
    /// Latitude as [degrees, minutes, seconds] rationals
    pub latitude: Option<[URational; 3]>,
    /// Latitude reference: 'N' or 'S'
    pub latitude_ref: Option<char>,
    /// Longitude as [degrees, minutes, seconds] rationals
    pub longitude: Option<[URational; 3]>,
    /// Longitude reference: 'E' or 'W'
    pub longitude_ref: Option<char>,
    /// Altitude (num/denom) in meters
    pub altitude: Option<URational>,
    /// Altitude reference: 0 = above sea level, 1 = below
    pub altitude_ref: Option<u8>,
    /// GPS timestamp [hour, minute, second] rationals
    pub timestamp: Option<[URational; 3]>,
    /// GPS datestamp "YYYY:MM:DD"
    pub datestamp: Option<String>,
    /// Speed (num/denom)
    pub speed: Option<URational>,
    /// Image direction (num/denom) in degrees
    pub img_direction: Option<URational>,
}

/// DNG color calibration data.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DngColorInfo {
    /// Color matrix 1 (3x3, row-major) - XYZ to camera native under illuminant 1
    pub color_matrix_1: Option<[f64; 9]>,
    /// Color matrix 2 (3x3, row-major) - XYZ to camera native under illuminant 2
    pub color_matrix_2: Option<[f64; 9]>,
    /// Standard light type for ColorMatrix1 (EXIF LightSource enum)
    pub calibration_illuminant_1: Option<u16>,
    /// Standard light type for ColorMatrix2 (EXIF LightSource enum)
    pub calibration_illuminant_2: Option<u16>,
    /// As-shot neutral white balance [R, G, B] multipliers
    pub as_shot_neutral: Option<[f64; 3]>,
    /// Analog balance [R, G, B]
    pub analog_balance: Option<[f64; 3]>,
    /// White balance setting name (e.g., "Cloudy", "Auto")
    pub white_balance: Option<String>,
    /// Color temperature in Kelvin
    pub color_temperature: Option<u32>,
}

/// DNG calibration and noise data.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DngCalibrationInfo {
    /// Baseline exposure offset in EV
    pub baseline_exposure: Option<f64>,
    /// Baseline noise level
    pub baseline_noise: Option<f64>,
    /// Baseline sharpness
    pub baseline_sharpness: Option<f64>,
    /// Noise profile coefficients
    pub noise_profile: Option<Vec<f64>>,
    /// Amount of noise reduction applied (0.0-1.0)
    pub noise_reduction_applied: Option<f64>,
}

/// DNG profile data.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DngProfileInfo {
    /// Embedded profile name
    pub profile_name: Option<String>,
    /// Profile tone curve (pairs of input/output values)
    pub profile_tone_curve: Option<Vec<f32>>,
}

/// Image-level metadata.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageInfo {
    /// EXIF orientation (1-8)
    pub orientation: Option<u16>,
    /// Bits per sample
    pub bit_depth: u8,
    /// Black level per channel
    pub black_levels: Vec<u32>,
    /// White/saturation level
    pub white_level: Option<u32>,
    /// Default crop origin (x, y)
    pub default_crop_origin: Option<(u32, u32)>,
    /// Default crop size (width, height)
    pub default_crop_size: Option<(u32, u32)>,
}

/// Complete image metadata — unified superset of all supported formats.
///
/// This struct is designed for extension. When adding support for new RAW
/// formats, add fields as needed to capture format-specific metadata.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageMetadata {
    /// Camera identification
    pub camera: CameraInfo,
    /// EXIF exposure settings
    pub exif: ExifInfo,
    /// Date/time information
    pub datetime: DateTimeInfo,
    /// GPS location data
    pub gps: GpsInfo,
    /// DNG color science
    pub dng_color: DngColorInfo,
    /// DNG calibration/noise
    pub dng_calibration: DngCalibrationInfo,
    /// DNG profile data
    pub dng_profile: DngProfileInfo,
    /// Image-level metadata
    pub image: ImageInfo,
}

/// Trait for extracting unified metadata from format-specific structures.
///
/// Implementors MUST provide metadata extraction for their format.
/// The compiler enforces implementation; incomplete data is handled via Option.
pub trait MetadataExtractor {
    /// Extract unified metadata from the format-specific representation.
    fn extract_metadata(&self) -> ImageMetadata;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_metadata_default() {
        let meta = ImageMetadata::default();
        assert!(meta.camera.make.is_empty());
        assert!(meta.exif.iso.is_none());
        assert!(meta.gps.latitude.is_none());
    }

    #[test]
    fn test_rational_types() {
        let ur = URational::new(1, 100);
        assert_eq!(ur.numerator, 1);
        assert_eq!(ur.denominator, 100);

        let sr = SRational::new(-1, 3);
        assert_eq!(sr.numerator, -1);
        assert_eq!(sr.denominator, 3);
    }
}
