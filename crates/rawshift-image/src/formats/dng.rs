//! Adobe DNG (Digital Negative) format support.
//!
//! Raw-image decoding (uncompressed, Deflate, lossless JPEG, and DNG 1.7
//! JPEG XL — the iPhone ProRAW scheme) is backed by [`gamut_dng::DngDecoder`];
//! the EXIF/GPS/thumbnail tree walk reuses the shared [`gamut_ifd`] helpers in
//! [`super::ifd`]. DNG tag semantics that gamut-dng does not type yet
//! (NoiseProfile, ProfileToneCurve) are read from the decoder's verbatim
//! `RawTag` extras.

use std::io::{Read, Seek};
use std::marker::PhantomData;

use gamut_dng::{DecodedDng, DngDecoder, RawLevels, RawPhotometry, RawTag};
use gamut_ifd::{Ifd, Value};

use super::ifd::{self, tags};
use crate::core::RgbImage;
use crate::core::image::{CfaPattern, Dimensions, RawImage, Rect};
use crate::error::{RawError, RawResult};

/// Metadata extracted from a DNG file.
#[derive(Debug, Clone)]
pub struct DngMetadata {
    /// Camera manufacturer
    pub make: String,
    /// Camera model
    pub model: String,
    /// Unique camera model identifier
    pub unique_camera_model: String,
    /// Full sensor dimensions
    pub sensor_size: Dimensions,
    /// Active/crop area (if different from sensor size)
    pub active_area: Option<Rect>,
    /// Default crop origin
    pub default_crop_origin: Option<(u32, u32)>,
    /// Default crop size
    pub default_crop_size: Option<(u32, u32)>,
    /// Bits per sample of the *decoded* samples. JPEG XL raw data decodes to
    /// full-range 16-bit (the reference SDK's semantics), so a ProRAW DNG that
    /// stores a 10-bit codestream reports 16 here.
    pub bit_depth: u8,
    /// Samples per pixel (1 for CFA, 3 for LinearRaw RGB)
    pub samples_per_pixel: u8,
    /// Compression type (1 = uncompressed, 7 = lossless JPEG, 52546 = JPEG XL)
    pub compression: u16,
    /// Photometric interpretation (32803=CFA, 34892=LinearRaw)
    pub photometric_interpretation: u16,
    /// True if LinearRaw (pre-demosaiced RGB)
    pub is_linear_raw: bool,
    /// Color matrix 1 (XYZ to camera native, illuminant 1)
    pub color_matrix1: Option<[f64; 9]>,
    /// Color matrix 2 (XYZ to camera native, illuminant 2)
    pub color_matrix2: Option<[f64; 9]>,
    /// As-shot neutral white balance
    pub as_shot_neutral: Option<[f64; 3]>,
    /// Analog balance
    pub analog_balance: Option<[f64; 3]>,
    /// Black level pattern values (rounded; `BlackLevelRepeatDim` cells in
    /// row-column-sample order)
    pub black_levels: Vec<u32>,
    /// White level per sample plane
    pub white_levels: Vec<u32>,
    /// DNG version (major, minor, patch, patch2)
    pub dng_version: [u8; 4],
    /// CFA pattern (only valid if not LinearRaw)
    pub cfa_pattern: Option<CfaPattern>,
    /// Linearization table (if present)
    pub linearization_table: Option<Vec<u16>>,
    /// Baseline exposure in EV (positive = brighten, negative = darken)
    pub baseline_exposure: Option<f32>,
    /// Calibration illuminant 1
    pub calibration_illuminant_1: Option<u16>,
    /// Calibration illuminant 2
    pub calibration_illuminant_2: Option<u16>,
    /// Noise profile coefficients
    pub noise_profile: Option<Vec<f64>>,
    /// Profile name
    pub profile_name: Option<String>,
    /// Profile tone curve
    pub profile_tone_curve: Option<Vec<f32>>,
    /// EXIF exposure/capture settings
    pub exif: crate::core::metadata::ExifInfo,
    /// Date/time information
    pub datetime: crate::core::metadata::DateTimeInfo,
    /// GPS location data
    pub gps: crate::core::metadata::GpsInfo,
    /// Lens make
    pub lens_make: Option<String>,
    /// Lens model
    pub lens_model: Option<String>,
    /// EXIF orientation tag (1-8)
    pub orientation: Option<u16>,
    /// Raw bytes of OpcodeList1 (applied to raw CFA data before demosaic)
    pub opcode_list1: Vec<u8>,
    /// Raw bytes of OpcodeList2 (applied to linear/demosaiced data)
    pub opcode_list2: Vec<u8>,
    /// Raw bytes of OpcodeList3 (applied after colour processing)
    pub opcode_list3: Vec<u8>,
}

/// Parsed Adobe DNG file.
pub struct DngFile<R> {
    /// The whole file, read into memory (needed for thumbnail extraction).
    data: Vec<u8>,
    /// The main IFD chain (with SubIFDs/Exif/GPS pointer groups resolved),
    /// used for the metadata/thumbnail reads gamut-dng does not type.
    ifds: Vec<Ifd>,
    /// The gamut-dng decode: raw image, camera profile, version, and the
    /// verbatim extras.
    decoded: DecodedDng,
    /// Extracted metadata
    metadata: Option<DngMetadata>,
    /// The reader type this file was parsed from.
    _reader: PhantomData<R>,
}

impl<R: Read + Seek> DngFile<R> {
    /// Parse a DNG file.
    ///
    /// The raw image is decoded eagerly by [`gamut_dng::DngDecoder`];
    /// [`Self::decode_raw`] / [`Self::decode_linear_raw`] bridge the decoded
    /// samples into rawshift's image types.
    pub fn parse(reader: R) -> RawResult<Self> {
        let data = ifd::read_all(reader)?;
        let decoded = DngDecoder::new()
            .decode(&data)
            .map_err(|e| RawError::gamut("DNG: decode", e))?;
        let tree = ifd::parse_tree(&data, "DNG: TIFF structure")?;

        let mut dng = DngFile {
            data,
            ifds: tree.ifds,
            decoded,
            metadata: None,
            _reader: PhantomData,
        };
        dng.extract_metadata();
        Ok(dng)
    }

    /// Get IFD0 (main IFD).
    fn ifd0(&self) -> Option<&Ifd> {
        self.ifds.first()
    }

    /// Get the extracted metadata.
    pub fn metadata(&self) -> Option<&DngMetadata> {
        self.metadata.as_ref()
    }

    /// Extract metadata from the decoded DNG and the parsed IFD tree.
    fn extract_metadata(&mut self) {
        let raw = &self.decoded.raw;
        let profile = &self.decoded.profile;
        let levels = raw.levels();

        // Container-side metadata (tags gamut-dng consumes without exposing,
        // or that live outside its typed surface).
        let ifd0 = self.ifd0();
        let make = ifd0
            .and_then(|i| ifd::ascii_tag(i, tags::MAKE))
            .unwrap_or_default();
        let model = ifd0
            .and_then(|i| ifd::ascii_tag(i, tags::MODEL))
            .unwrap_or_default();
        let exif = ifd0.map(ifd::extract_exif).unwrap_or_default();
        let datetime = ifd0.map(ifd::extract_datetime).unwrap_or_default();
        let gps = ifd0.map(ifd::extract_gps).unwrap_or_default();
        let (lens_make, lens_model) = ifd0.map(ifd::extract_lens_info).unwrap_or_default();
        let orientation = ifd0.and_then(ifd::extract_orientation);
        let (compression, photometric_interpretation) = raw_container_codes(&self.ifds);

        // Typed camera-profile fields.
        let color_matrix1 = Some(*profile.color_matrix1());
        let (color_matrix2, calibration_illuminant_2) = match profile.second_illuminant() {
            Some((m, ill)) => (Some(*m), Some(ill.code())),
            None => (None, None),
        };
        let calibration_illuminant_1 = Some(profile.calibration_illuminant1().code());
        let as_shot_neutral = Some(*profile.as_shot_neutral());
        let analog_balance = profile.analog_balance().copied();
        let baseline_exposure = profile.baseline_exposure().map(|v| v as f32);

        // DNG tags without a typed gamut-dng surface yet: read them from the
        // decoder's verbatim extras (raw IFD first, matching the legacy
        // preference, then IFD0).
        let noise_profile = extra_value(&self.decoded, tags::NOISE_PROFILE).and_then(value_f64s);
        let profile_name = extra_value(&self.decoded, tags::PROFILE_NAME)
            .and_then(Value::as_str)
            .map(ifd::clean_ascii)
            .or_else(|| profile.profile_name().map(str::to_string));
        let profile_tone_curve =
            extra_value(&self.decoded, tags::PROFILE_TONE_CURVE).and_then(value_f32s);

        let photometry = raw.photometry();
        let is_linear_raw = matches!(photometry, RawPhotometry::LinearRaw { .. });
        let cfa_pattern = cfa_pattern_of(photometry);

        self.metadata = Some(DngMetadata {
            make,
            model,
            unique_camera_model: profile.unique_camera_model().to_string(),
            sensor_size: Dimensions {
                width: raw.dimensions().width,
                height: raw.dimensions().height,
            },
            active_area: raw.active_area().map(active_area_rect),
            default_crop_origin: raw.default_crop().map(|(o, _)| (o[0], o[1])),
            default_crop_size: raw.default_crop().map(|(_, s)| (s[0], s[1])),
            bit_depth: raw.bits_per_sample() as u8,
            samples_per_pixel: raw.samples_per_pixel() as u8,
            compression,
            photometric_interpretation,
            is_linear_raw,
            color_matrix1,
            color_matrix2,
            as_shot_neutral,
            analog_balance,
            black_levels: levels.black().iter().map(|&v| v.round() as u32).collect(),
            white_levels: levels.white().iter().map(|&v| v.round() as u32).collect(),
            dng_version: self.decoded.dng_version,
            cfa_pattern,
            linearization_table: levels.linearization_table().map(<[u16]>::to_vec),
            baseline_exposure,
            calibration_illuminant_1,
            calibration_illuminant_2,
            noise_profile,
            profile_name,
            profile_tone_curve,
            exif,
            datetime,
            gps,
            lens_make,
            lens_model,
            orientation,
            opcode_list1: opcode_bytes(raw.opcode_list1()),
            opcode_list2: opcode_bytes(raw.opcode_list2()),
            opcode_list3: opcode_bytes(raw.opcode_list3()),
        });
    }

    /// Extract the embedded JPEG thumbnail, if present.
    ///
    /// Searches IFDs for a thumbnail (NewSubfileType=1) or falls back to
    /// JPEGInterchangeFormat in IFD 0.
    pub fn thumbnail(&mut self) -> RawResult<Option<Vec<u8>>> {
        let mut all = Vec::new();
        flatten_ifds(&self.ifds, &mut all);
        for ifd in &all {
            if ifd.get(tags::NEW_SUBFILE_TYPE).and_then(Value::as_u32) == Some(1)
                && let Some(thumb) = ifd::jpeg_thumbnail(&self.data, ifd)?
            {
                return Ok(Some(thumb));
            }
        }
        match self.ifd0() {
            Some(ifd0) => ifd::jpeg_thumbnail(&self.data, ifd0),
            None => Ok(None),
        }
    }

    /// Decode the raw image data.
    ///
    /// For LinearRaw (iPhone ProRAW), this returns the already-demosaiced RGB
    /// samples interleaved in [`RawImage::data`]. For CFA (Bayer) data, this
    /// returns a RawImage that needs demosaicing.
    ///
    /// Matching the legacy decoder, the linearization table (if present) is
    /// applied as a lookup; black-level subtraction stays in `transforms/`.
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        let raw = &self.decoded.raw;
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| RawError::Unsupported("Metadata not extracted".to_string()))?;

        let mut samples = raw.samples().to_vec();
        if let Some(table) = raw.levels().linearization_table() {
            apply_linearization(&mut samples, table);
        }

        let width = raw.dimensions().width;
        let height = raw.dimensions().height;
        let active_area = metadata
            .active_area
            .unwrap_or(Rect::from_coords(0, 0, width, height));
        let cfa_pattern = metadata.cfa_pattern.unwrap_or(CfaPattern::Rggb);

        // If a linearization table is applied, the effective bit depth is
        // usually 16-bit (matching the legacy decoder).
        let bit_depth = if raw
            .levels()
            .linearization_table()
            .is_some_and(|t| !t.is_empty())
        {
            16
        } else {
            metadata.bit_depth
        };

        let mut builder =
            RawImage::builder(metadata.sensor_size, active_area, bit_depth, cfa_pattern)
                .black_levels(black_levels_array(raw.levels()))
                .white_level(white_level_u16(raw.levels()))
                .data(samples);
        if let Some(be) = metadata.baseline_exposure {
            builder = builder.baseline_exposure(be);
        }
        if let Some(crop) = raw.default_crop() {
            builder = builder.default_crop(default_crop_rect(crop));
        }
        Ok(builder.build())
    }

    /// Decode LinearRaw data directly to an RGB image.
    ///
    /// This is the preferred method for iPhone ProRAW files
    /// which are already demosaiced.
    pub fn decode_linear_raw(&mut self) -> RawResult<RgbImage> {
        let raw = &self.decoded.raw;
        let RawPhotometry::LinearRaw { planes } = *raw.photometry() else {
            return Err(RawError::Unsupported(
                "Not a LinearRaw DNG file".to_string(),
            ));
        };
        if planes != 3 {
            return Err(RawError::Unsupported(format!(
                "LinearRaw DNG with {} planes is not supported (expected 3)",
                planes
            )));
        }
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| RawError::Unsupported("Metadata not extracted".to_string()))?;

        let width = raw.dimensions().width as usize;
        let height = raw.dimensions().height as usize;
        let active_area =
            metadata
                .active_area
                .unwrap_or(Rect::from_coords(0, 0, width as u32, height as u32));
        let out_width = active_area.size.width as usize;
        let out_height = active_area.size.height as usize;
        let offset_x = active_area.origin.x as usize;
        let offset_y = active_area.origin.y as usize;

        // Crop to the active area, applying the linearization lookup (the
        // legacy decoder's per-pixel semantics).
        let table = raw.levels().linearization_table();
        let sensor = raw.samples();
        let mut output = vec![0u16; out_width * out_height * 3];
        for y in 0..out_height {
            let src_y = offset_y + y;
            if src_y >= height {
                break;
            }
            for x in 0..out_width {
                let src_x = offset_x + x;
                if src_x >= width {
                    break;
                }
                let src_idx = (src_y * width + src_x) * 3;
                let dst_idx = (y * out_width + x) * 3;
                for c in 0..3 {
                    let mut val = sensor[src_idx + c];
                    if let Some(table) = table
                        && !table.is_empty()
                    {
                        val = table[(val as usize).min(table.len() - 1)];
                    }
                    output[dst_idx + c] = val;
                }
            }
        }

        let mut image = RgbImage::new(out_width as u32, out_height as u32, output)?;
        image.set_baseline_exposure(metadata.baseline_exposure);
        image.set_default_crop(raw.default_crop().map(default_crop_rect));

        // Apply OpcodeList2 — defined as corrections applied to linear raw
        // (post-demosaic) data. This is where GainMap (lens shading
        // correction) lives for iPhone ProRAW.
        if !raw.opcode_list2().is_empty() {
            let opcode_list =
                crate::transforms::opcodes::OpcodeList::parse(&raw.opcode_list2().to_bytes());
            opcode_list.apply_to_rgb(&mut image);
        }

        Ok(image)
    }
}

// ============================================================================
// gamut-dng bridge helpers
// ============================================================================

/// Applies a DNG `LinearizationTable` lookup in place (inputs at or beyond the
/// table length map to the last entry, DNG 1.7.1 p. 99).
fn apply_linearization(samples: &mut [u16], table: &[u16]) {
    if table.is_empty() {
        return;
    }
    for v in samples {
        *v = table[(*v as usize).min(table.len() - 1)];
    }
}

/// Collapses the [`RawLevels`] black pattern to rawshift's `[u16; 4]` per-CFA-
/// channel model (rounded).
///
/// Policy: a full 2×2 single-plane pattern maps cell-for-cell; anything else
/// (uniform values included) fills the four slots by cycling through the
/// pattern cells, which reproduces a uniform level exactly and approximates
/// larger patterns by their leading cells.
fn black_levels_array(levels: &RawLevels) -> [u16; 4] {
    let cells = levels.black();
    let mut out = [0u16; 4];
    if cells.is_empty() {
        return out;
    }
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = cells[i % cells.len()].round().clamp(0.0, 65535.0) as u16;
    }
    out
}

/// The first plane's white level, rounded into rawshift's `u16` model.
fn white_level_u16(levels: &RawLevels) -> u16 {
    levels
        .white()
        .first()
        .map(|&w| w.round().clamp(0.0, 65535.0) as u16)
        .unwrap_or(u16::MAX)
}

/// Maps a DNG `ActiveArea` `[top, left, bottom, right]` to a [`Rect`].
fn active_area_rect(area: [u32; 4]) -> Rect {
    let [top, left, bottom, right] = area;
    Rect::from_coords(
        left,
        top,
        right.saturating_sub(left),
        bottom.saturating_sub(top),
    )
}

/// Maps a DNG default crop `(origin, size)` to a [`Rect`].
fn default_crop_rect(crop: ([u32; 2], [u32; 2])) -> Rect {
    let (origin, size) = crop;
    Rect::from_coords(origin[0], origin[1], size[0], size[1])
}

/// The rawshift [`CfaPattern`] for a 2×2 CFA photometry, if representable.
fn cfa_pattern_of(photometry: &RawPhotometry) -> Option<CfaPattern> {
    match photometry {
        RawPhotometry::Cfa {
            repeat: (2, 2),
            pattern,
            ..
        } if pattern.len() == 4 => {
            CfaPattern::from_array([pattern[0], pattern[1], pattern[2], pattern[3]])
        }
        _ => None,
    }
}

/// An opcode list's DNG container bytes, or empty when the list is empty
/// (matching the legacy "absent tag" representation).
fn opcode_bytes(list: &gamut_dng::OpcodeList) -> Vec<u8> {
    if list.is_empty() {
        Vec::new()
    } else {
        list.to_bytes()
    }
}

/// Looks up `tag` in the decode's verbatim extras — the raw IFD's first
/// (matching the legacy raw-then-IFD0 read order), then IFD 0's.
fn extra_value(decoded: &DecodedDng, tag: u16) -> Option<&Value> {
    fn find(extras: &[RawTag], tag: u16) -> Option<&Value> {
        extras.iter().find(|t| t.tag == tag).map(|t| &t.value)
    }
    find(&decoded.raw_extra, tag).or_else(|| find(&decoded.ifd0_extra, tag))
}

/// Numeric values of a tag as `f64`s (`DOUBLE`/`FLOAT`/`RATIONAL`/`SRATIONAL`
/// or integer types).
fn value_f64s(value: &Value) -> Option<Vec<f64>> {
    let ratio = |n: f64, d: f64| if d == 0.0 { 0.0 } else { n / d };
    match value {
        Value::Double(v) => Some(v.clone()),
        Value::Float(v) => Some(v.iter().map(|&x| f64::from(x)).collect()),
        Value::Rational(v) => Some(
            v.iter()
                .map(|&(n, d)| ratio(f64::from(n), f64::from(d)))
                .collect(),
        ),
        Value::SRational(v) => Some(
            v.iter()
                .map(|&(n, d)| ratio(f64::from(n), f64::from(d)))
                .collect(),
        ),
        Value::Byte(v) => Some(v.iter().map(|&x| f64::from(x)).collect()),
        Value::Short(v) => Some(v.iter().map(|&x| f64::from(x)).collect()),
        Value::Long(v) => Some(v.iter().map(|&x| f64::from(x)).collect()),
        Value::SLong(v) => Some(v.iter().map(|&x| f64::from(x)).collect()),
        _ => None,
    }
}

/// Numeric values of a tag as `f32`s (the `ProfileToneCurve` shape).
fn value_f32s(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::Float(v) => Some(v.clone()),
        _ => value_f64s(value).map(|v| v.into_iter().map(|x| x as f32).collect()),
    }
}

/// Reads the raw IFD's `Compression` and `PhotometricInterpretation` codes
/// from the parsed container tree (gamut-dng consumes them during decode
/// without exposing them). Uses the legacy heuristic: the raw-photometry IFD
/// with the largest pixel count anywhere in the tree.
fn raw_container_codes(ifds: &[Ifd]) -> (u16, u16) {
    let mut all = Vec::new();
    flatten_ifds(ifds, &mut all);
    let mut best: Option<(&Ifd, u64)> = None;
    for ifd in all {
        let photometric = ifd
            .get(tags::PHOTOMETRIC_INTERPRETATION)
            .and_then(Value::as_u32)
            .unwrap_or(0) as u16;
        if photometric != 32803 && photometric != 34892 {
            continue;
        }
        let width = ifd::first_u32(ifd, tags::IMAGE_WIDTH).unwrap_or(0);
        let height = ifd::first_u32(ifd, tags::IMAGE_LENGTH).unwrap_or(0);
        let pixels = u64::from(width) * u64::from(height);
        if best.map(|(_, p)| p < pixels).unwrap_or(true) {
            best = Some((ifd, pixels));
        }
    }
    match best {
        Some((ifd, _)) => (
            ifd.get(tags::COMPRESSION)
                .and_then(Value::as_u32)
                .unwrap_or(1) as u16,
            ifd.get(tags::PHOTOMETRIC_INTERPRETATION)
                .and_then(Value::as_u32)
                .unwrap_or(32803) as u16,
        ),
        None => (1, 32803),
    }
}

/// Collects every IFD in the resolved tree — the top-level chain plus every
/// sub-IFD group's children, recursively.
fn flatten_ifds<'a>(ifds: &'a [Ifd], out: &mut Vec<&'a Ifd>) {
    for ifd in ifds {
        out.push(ifd);
        for group in ifd.sub_ifds() {
            flatten_ifds(&group.ifds, out);
        }
    }
}

impl<R: Read + Seek> crate::core::ExtractMetadata for DngFile<R> {
    fn extract_metadata(&self) -> crate::core::ImageMetadata {
        use crate::core::metadata::*;

        let m = self.metadata.as_ref();

        ImageMetadata {
            camera: CameraInfo {
                make: m.map(|x| x.make.clone()).unwrap_or_default(),
                model: m.map(|x| x.model.clone()).unwrap_or_default(),
                unique_camera_model: m.map(|x| x.unique_camera_model.clone()),
                lens_make: m.and_then(|x| x.lens_make.clone()),
                lens_model: m.and_then(|x| x.lens_model.clone()),
                lens_info: None,
                serial_number: None,
            },
            exif: m.map(|x| x.exif.clone()).unwrap_or_default(),
            datetime: m.map(|x| x.datetime.clone()).unwrap_or_default(),
            gps: m.map(|x| x.gps.clone()).unwrap_or_default(),
            dng_color: DngColorInfo {
                color_matrix_1: m.and_then(|x| x.color_matrix1),
                color_matrix_2: m.and_then(|x| x.color_matrix2),
                calibration_illuminant_1: m.and_then(|x| x.calibration_illuminant_1),
                calibration_illuminant_2: m.and_then(|x| x.calibration_illuminant_2),
                as_shot_neutral: m.and_then(|x| x.as_shot_neutral),
                analog_balance: m.and_then(|x| x.analog_balance),
                white_balance: None,
                color_temperature: None,
            },
            dng_calibration: DngCalibrationInfo {
                baseline_exposure: m.and_then(|x| x.baseline_exposure.map(|v| v as f64)),
                baseline_noise: None,
                baseline_sharpness: None,
                noise_profile: m.and_then(|x| x.noise_profile.clone()),
                noise_reduction_applied: None,
            },
            dng_profile: DngProfileInfo {
                profile_name: m.and_then(|x| x.profile_name.clone()),
                profile_tone_curve: m.and_then(|x| x.profile_tone_curve.clone()),
            },
            image: ImageInfo {
                orientation: m.and_then(|x| x.orientation),
                bit_depth: m.map(|x| x.bit_depth).unwrap_or(16),
                black_levels: m.map(|x| x.black_levels.clone()).unwrap_or_default(),
                white_level: m.and_then(|x| x.white_levels.first().copied()),
                default_crop_origin: m.and_then(|x| x.default_crop_origin),
                default_crop_size: m.and_then(|x| x.default_crop_size),
            },
            xmp: None,
            icc_profile: None,
            exif_raw: None,
            makernote_raw: None,
            iptc_raw: None,
            extra: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::{BufReader, Cursor};
    use std::path::PathBuf;

    use gamut_dng::values::CalibrationIlluminant;
    use gamut_dng::{CameraProfile, DngEncoder};

    fn test_data_path(filename: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(filename)
    }

    /// Returns `true` (and logs) when the fixture is absent, so the caller can
    /// skip gracefully. Real RAW fixtures require human sourcing and are not
    /// always present in CI — see TEST_FIXTURES.md.
    fn skip_if_no_test_data(path: &PathBuf) -> bool {
        if !path.exists() {
            eprintln!("Skipping test: test data file not found: {:?}", path);
            return true;
        }
        false
    }

    fn test_profile() -> CameraProfile {
        CameraProfile::new(
            "TestCam StripTest",
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            CalibrationIlluminant::D65,
            [0.5, 1.0, 0.6],
        )
        .expect("valid profile")
    }

    /// Encode a CFA (RGGB, 16-bit, uncompressed) DNG in memory via gamut-dng.
    fn build_cfa_dng(width: u32, height: u32, pixel_data: &[u16]) -> Vec<u8> {
        let raw = gamut_dng::RawImage::new_cfa(
            gamut_dng::Dimensions::new(width, height).unwrap(),
            16,
            (2, 2),
            vec![0, 1, 1, 2],
            pixel_data.to_vec(),
        )
        .unwrap()
        .with_black_level(128.0)
        .unwrap();
        let mut out = Vec::new();
        DngEncoder::new()
            .encode(&raw, &test_profile(), &mut out)
            .unwrap();
        out
    }

    #[test]
    fn test_cfa_dng_parse_metadata() {
        let width = 8u32;
        let height = 6u32;
        let pixel_data = vec![1000u16; (width * height) as usize];
        let dng_bytes = build_cfa_dng(width, height, &pixel_data);

        let dng = DngFile::parse(Cursor::new(dng_bytes)).unwrap();
        let meta = dng.metadata().unwrap();

        assert_eq!(meta.sensor_size.width, width);
        assert_eq!(meta.sensor_size.height, height);
        assert_eq!(meta.compression, 1);
        assert_eq!(meta.photometric_interpretation, 32803);
        assert!(!meta.is_linear_raw);
        assert_eq!(meta.bit_depth, 16);
        assert_eq!(meta.samples_per_pixel, 1);
        assert_eq!(meta.cfa_pattern, Some(CfaPattern::Rggb));
        assert_eq!(meta.unique_camera_model, "TestCam StripTest");
        assert_eq!(meta.black_levels, vec![128]);
        assert_eq!(meta.white_levels, vec![65535]);
        assert_eq!(meta.calibration_illuminant_1, Some(21)); // D65
        assert_eq!(meta.as_shot_neutral, Some([0.5, 1.0, 0.6]));
    }

    #[test]
    fn test_cfa_dng_decode_raw_round_trips_pixels() {
        let width = 8u32;
        let height = 6u32;
        // Fill with a known pattern: pixel value = row * width + col + 100
        let mut pixel_data = vec![0u16; (width * height) as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                pixel_data[y * width as usize + x] = (y * width as usize + x + 100) as u16;
            }
        }
        let dng_bytes = build_cfa_dng(width, height, &pixel_data);

        let mut dng = DngFile::parse(Cursor::new(dng_bytes)).unwrap();
        let raw_image = dng.decode_raw().unwrap();

        assert_eq!(raw_image.size().width, width);
        assert_eq!(raw_image.size().height, height);
        assert_eq!(raw_image.data, pixel_data);
        // The uniform black level replicates across the 4 CFA slots.
        assert_eq!(raw_image.black_levels(), &[128, 128, 128, 128]);
        assert_eq!(raw_image.white_level(), 65535);
    }

    #[test]
    fn test_linear_raw_dng_round_trips_rgb() {
        let width = 4u32;
        let height = 4u32;
        let pixel_data: Vec<u16> = (0..(width * height * 3) as u16)
            .map(|i| i * 7 + 500)
            .collect();
        let raw = gamut_dng::RawImage::new_linear_raw(
            gamut_dng::Dimensions::new(width, height).unwrap(),
            16,
            3,
            pixel_data.clone(),
        )
        .unwrap();
        let mut dng_bytes = Vec::new();
        DngEncoder::new()
            .encode(&raw, &test_profile(), &mut dng_bytes)
            .unwrap();

        let mut dng = DngFile::parse(Cursor::new(dng_bytes)).unwrap();
        let meta = dng.metadata().unwrap();
        assert!(meta.is_linear_raw);
        assert_eq!(meta.samples_per_pixel, 3);
        assert_eq!(meta.photometric_interpretation, 34892);

        let rgb = dng.decode_linear_raw().unwrap();
        assert_eq!(rgb.width(), width);
        assert_eq!(rgb.height(), height);
        assert_eq!(rgb.data(), pixel_data.as_slice());
    }

    #[test]
    fn test_black_levels_array_policies() {
        // Full 2x2 single-plane pattern: cell-for-cell.
        let l = RawLevels::new(1, (2, 2), vec![1.0, 2.0, 3.0, 4.4], vec![4095.0]).unwrap();
        assert_eq!(black_levels_array(&l), [1, 2, 3, 4]);
        // Uniform (1x1): replicated across the 4 slots.
        let l = RawLevels::uniform(1, 64.0, 4095.0).unwrap();
        assert_eq!(black_levels_array(&l), [64, 64, 64, 64]);
        // 3-plane linear: cycles through the plane values.
        let l = RawLevels::new(3, (1, 1), vec![10.0, 20.0, 30.0], vec![65535.0; 3]).unwrap();
        assert_eq!(black_levels_array(&l), [10, 20, 30, 10]);
        assert_eq!(white_level_u16(&l), 65535);
    }

    #[test]
    fn test_geometry_bridges() {
        // ActiveArea is [top, left, bottom, right].
        let r = active_area_rect([10, 20, 110, 220]);
        assert_eq!((r.origin.x, r.origin.y), (20, 10));
        assert_eq!((r.size.width, r.size.height), (200, 100));

        let r = default_crop_rect(([4, 8], [100, 50]));
        assert_eq!((r.origin.x, r.origin.y), (4, 8));
        assert_eq!((r.size.width, r.size.height), (100, 50));
    }

    #[test]
    fn test_apply_linearization_clamps_index() {
        let table = vec![0u16, 10, 20, 30];
        let mut samples = vec![0u16, 2, 3, 100];
        apply_linearization(&mut samples, &table);
        assert_eq!(samples, vec![0, 20, 30, 30]);
    }

    // TODO: get rid of all `skip_if_no_test_data()` once fixtures are properly
    // configured even in CI

    #[test]
    fn test_dng_parse_iphone() {
        let path = test_data_path("Apple/iPhone_17_Pro_Max/IMG_1347.DNG");
        if skip_if_no_test_data(&path) {
            return;
        }

        let file = File::open(&path).unwrap();
        let reader = BufReader::new(file);
        let dng = DngFile::parse(reader).unwrap();

        let metadata = dng.metadata().unwrap();

        // Validate Apple camera
        assert!(
            metadata.make.to_uppercase().contains("APPLE"),
            "Make should be Apple"
        );
        assert!(
            metadata.model.contains("iPhone"),
            "Model should contain iPhone"
        );

        // Validate DNG 1.7
        assert_eq!(metadata.dng_version[0], 1, "DNG major version should be 1");
        assert_eq!(metadata.dng_version[1], 7, "DNG minor version should be 7");

        // Validate dimensions (from exiftool: 8064x6048)
        assert_eq!(metadata.sensor_size.width, 8064);
        assert_eq!(metadata.sensor_size.height, 6048);

        // Validate compression (JPEG XL = 52546)
        assert_eq!(metadata.compression, 52546);

        // Validate LinearRaw
        assert!(metadata.is_linear_raw, "Should be LinearRaw");
        assert_eq!(
            metadata.samples_per_pixel, 3,
            "Should have 3 samples per pixel"
        );

        // JPEG XL raw data decodes to full-range 16-bit (the file stores a
        // 10-bit codestream with WhiteLevel 65535).
        assert_eq!(metadata.bit_depth, 16, "Decoded precision should be 16-bit");
    }

    #[test]
    fn test_dng_decode_iphone() {
        let path = test_data_path("Apple/iPhone_17_Pro_Max/IMG_1347.DNG");
        if skip_if_no_test_data(&path) {
            return;
        }

        let file = File::open(&path).unwrap();
        let reader = BufReader::new(file);
        let mut dng = DngFile::parse(reader).unwrap();

        // Decode as LinearRaw
        let rgb_image = dng.decode_linear_raw().unwrap();

        // Validate dimensions
        assert_eq!(rgb_image.width(), 8064);
        assert_eq!(rgb_image.height(), 6048);

        // Validate data size (width * height * 3 channels)
        let expected_size = 8064 * 6048 * 3;
        assert_eq!(rgb_image.data().len(), expected_size);

        // Check that we got some non-zero pixel data
        let non_zero_count = rgb_image.data().iter().filter(|&&v| v > 0).count();
        assert!(non_zero_count > 0, "Should have non-zero pixel values");
    }
}
