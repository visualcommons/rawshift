//! Unified metadata types for image formats.
//!
//! This module provides format-agnostic metadata structures that represent
//! a superset of all metadata that can exist in any supported RAW format.
//! Extend as new formats reveal additional fields.

/// Unsigned rational (numerator, denominator).
///
/// The format-agnostic rational used throughout the metadata model. TIFF-based
/// decoders carry their own wire-level `tiff::Rational` and convert into this
/// type at the metadata boundary via `From<tiff::Rational>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct URational {
    /// Numerator
    pub numerator: u32,
    /// Denominator
    pub denominator: u32,
}

impl URational {
    /// Create a new URational.
    pub fn new(numerator: u32, denominator: u32) -> Self {
        Self {
            numerator,
            denominator,
        }
    }
    /// Convert to f64.
    pub fn to_f64(&self) -> f64 {
        if self.denominator == 0 {
            f64::NAN
        } else {
            self.numerator as f64 / self.denominator as f64
        }
    }
}

/// Signed rational (numerator, denominator).
///
/// The format-agnostic signed rational used throughout the metadata model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SRational {
    /// Numerator
    pub numerator: i32,
    /// Denominator
    pub denominator: i32,
}

impl SRational {
    /// Create a new SRational.
    pub fn new(numerator: i32, denominator: i32) -> Self {
        Self {
            numerator,
            denominator,
        }
    }
    /// Convert to f64.
    pub fn to_f64(&self) -> f64 {
        if self.denominator == 0 {
            f64::NAN
        } else {
            self.numerator as f64 / self.denominator as f64
        }
    }
}

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

/// A typed, format-agnostic metadata value.
///
/// Used by [`ImageMetadata::extra`] to represent any metadata tag that does not
/// have a dedicated typed field. Every EXIF/TIFF/XMP/IPTC value type maps onto
/// one of these variants, so the generic table can hold *anything* the library
/// does not (yet) model without requiring a schema change.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MetadataValue {
    /// Unsigned integer (EXIF BYTE / SHORT / LONG widen losslessly into `u64`).
    U64(u64),
    /// Signed integer (EXIF SBYTE / SSHORT / SLONG).
    I64(i64),
    /// Floating point (EXIF FLOAT / DOUBLE).
    F64(f64),
    /// Unsigned rational (EXIF RATIONAL).
    URational(URational),
    /// Signed rational (EXIF SRATIONAL).
    SRational(SRational),
    /// Text (EXIF ASCII, XMP/IPTC strings).
    Text(String),
    /// Opaque/undefined byte payload (EXIF UNDEFINED, MakerNote fragments).
    Bytes(Vec<u8>),
    /// Homogeneous or heterogeneous array of values (EXIF count > 1).
    Array(Vec<MetadataValue>),
}

/// Namespace identifying the origin of a generic metadata tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MetadataNamespace {
    /// Standard TIFF/EXIF IFD tags.
    Exif,
    /// EXIF GPS IFD tags.
    Gps,
    /// Manufacturer MakerNote (uninterpreted sub-tags).
    MakerNote,
    /// XMP (RDF/XML) properties.
    Xmp,
    /// IPTC IIM datasets.
    Iptc,
    /// HEIC/HEIF container-level facts.
    Heic,
    /// Vendor/format-specific, identified by the accompanying tag string.
    Other,
}

/// Fully-qualified key into [`ImageMetadata::extra`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MetadataKey {
    /// Origin namespace.
    pub namespace: MetadataNamespace,
    /// Tag identifier — a string for serde stability and human-readable dumps
    /// (e.g. `"0x9209"` for an EXIF tag, or an XMP property path).
    pub tag: String,
}

impl MetadataKey {
    /// Create a new metadata key.
    pub fn new(namespace: MetadataNamespace, tag: impl Into<String>) -> Self {
        Self {
            namespace,
            tag: tag.into(),
        }
    }
}

/// One entry in the generic metadata table ([`ImageMetadata::extra`]).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MetadataEntry {
    /// Namespaced tag identifier.
    pub key: MetadataKey,
    /// Typed value.
    pub value: MetadataValue,
}

/// Complete image metadata — unified superset of all supported formats.
///
/// This struct is designed for extension. Common metadata has dedicated typed
/// fields ([`camera`](Self::camera), [`exif`](Self::exif), …); anything the
/// library does not model is preserved losslessly via the raw-blob fields
/// ([`exif_raw`](Self::exif_raw), [`xmp`](Self::xmp), …) and the typed generic
/// table [`extra`](Self::extra), so new formats never force a schema change.
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
    /// Raw XMP (XML) metadata bytes, if present in source image
    pub xmp: Option<Vec<u8>>,
    /// Raw embedded ICC color profile bytes, if present
    pub icc_profile: Option<Vec<u8>>,
    /// Full raw EXIF block (TIFF byte stream) exactly as embedded, if present
    pub exif_raw: Option<Vec<u8>>,
    /// Raw manufacturer MakerNote blob, uninterpreted, if present
    pub makernote_raw: Option<Vec<u8>>,
    /// Raw IPTC IIM block, if present
    pub iptc_raw: Option<Vec<u8>>,
    /// Typed generic tag table.
    ///
    /// Holds metadata tags as a complete typed mirror of the source — including
    /// tags also surfaced as dedicated fields above, and anything the library
    /// does not model. Guarantees no metadata is silently dropped.
    ///
    /// Stored as a `Vec` (not a map) so it serializes cleanly in every serde
    /// format and preserves source ordering. Use [`get`](Self::get) /
    /// [`insert`](Self::insert) for map-like access.
    pub extra: Vec<MetadataEntry>,
}

impl ImageMetadata {
    /// Look up a generic tag in [`extra`](Self::extra).
    ///
    /// Returns the first entry matching `namespace` and `tag`.
    pub fn get(&self, namespace: MetadataNamespace, tag: &str) -> Option<&MetadataValue> {
        self.extra
            .iter()
            .find(|e| e.key.namespace == namespace && e.key.tag == tag)
            .map(|e| &e.value)
    }

    /// Insert or overwrite a generic tag in [`extra`](Self::extra).
    ///
    /// If an entry with the same key already exists, its value is replaced;
    /// otherwise the entry is appended.
    pub fn insert(&mut self, key: MetadataKey, value: MetadataValue) {
        if let Some(entry) = self.extra.iter_mut().find(|e| e.key == key) {
            entry.value = value;
        } else {
            self.extra.push(MetadataEntry { key, value });
        }
    }
}

/// Trait for extracting unified metadata from format-specific structures.
///
/// Implementors MUST provide metadata extraction for their format.
/// The compiler enforces implementation; incomplete data is handled via Option.
pub trait ExtractMetadata {
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

    #[test]
    fn test_metadata_value_variants() {
        // Each variant constructs and compares as expected.
        assert_eq!(MetadataValue::U64(42), MetadataValue::U64(42));
        assert_ne!(MetadataValue::U64(1), MetadataValue::I64(1));
        let nested = MetadataValue::Array(vec![
            MetadataValue::Text("a".into()),
            MetadataValue::Bytes(vec![1, 2, 3]),
            MetadataValue::URational(URational::new(1, 2)),
        ]);
        match nested {
            MetadataValue::Array(items) => assert_eq!(items.len(), 3),
            _ => panic!("expected Array"),
        }
    }

    #[test]
    fn test_extra_get_insert() {
        let mut md = ImageMetadata::default();
        assert!(md.get(MetadataNamespace::Exif, "0x9209").is_none());

        md.insert(
            MetadataKey::new(MetadataNamespace::Exif, "0x9209"),
            MetadataValue::U64(9),
        );
        assert_eq!(md.extra.len(), 1);
        assert_eq!(
            md.get(MetadataNamespace::Exif, "0x9209"),
            Some(&MetadataValue::U64(9))
        );

        // Same key overwrites in place rather than appending.
        md.insert(
            MetadataKey::new(MetadataNamespace::Exif, "0x9209"),
            MetadataValue::U64(16),
        );
        assert_eq!(md.extra.len(), 1);
        assert_eq!(
            md.get(MetadataNamespace::Exif, "0x9209"),
            Some(&MetadataValue::U64(16))
        );

        // A different namespace with the same tag is a distinct entry.
        md.insert(
            MetadataKey::new(MetadataNamespace::Heic, "0x9209"),
            MetadataValue::Text("hi".into()),
        );
        assert_eq!(md.extra.len(), 2);
        assert!(md.get(MetadataNamespace::Gps, "0x9209").is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_image_metadata_serde_roundtrip_with_extra() {
        let mut md = ImageMetadata {
            icc_profile: Some(vec![0xAA, 0xBB]),
            exif_raw: Some(vec![1, 2, 3, 4]),
            ..Default::default()
        };
        md.insert(
            MetadataKey::new(MetadataNamespace::Exif, "FlashEnergy"),
            MetadataValue::Array(vec![
                MetadataValue::URational(URational::new(3, 2)),
                MetadataValue::F64(1.5),
            ]),
        );
        md.insert(
            MetadataKey::new(MetadataNamespace::Heic, "aux_count"),
            MetadataValue::U64(2),
        );

        let json = serde_json::to_string(&md).expect("serialize");
        let back: ImageMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(md, back, "ImageMetadata must survive a JSON round-trip");
    }
}
