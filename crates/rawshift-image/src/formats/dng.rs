//! Adobe DNG (Digital Negative) format support.
//!
//! This module provides parsing for DNG files, particularly DNG 1.7+
//! with JPEG XL compression as used by iPhone ProRAW.

use std::io::{Read, Seek};

use crate::codecs::jxl::JxlDecoder;
use crate::core::image::{CfaPattern, RawImage, Rect, RgbImage, Size};
use crate::error::{ParseError, RawError, RawResult};
use crate::tiff::{ByteOrder, Ifd, TiffParser, TiffTag, TiffValue};

// TODO: Ensure this code ensures all tags are parsed exhaustively

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
    pub sensor_size: Size,
    /// Active/crop area (if different from sensor size)
    pub active_area: Option<Rect>,
    /// Default crop origin
    pub default_crop_origin: Option<(u32, u32)>,
    /// Default crop size
    pub default_crop_size: Option<(u32, u32)>,
    /// Bits per sample (typically 10, 12, 14, or 16)
    pub bit_depth: u8,
    /// Samples per pixel (1 for CFA, 3 for LinearRaw RGB)
    pub samples_per_pixel: u8,
    /// Compression type (52546 = JPEG XL)
    pub compression: u16,
    /// Photometric interpretation (32803=CFA, 34892=LinearRaw)
    pub photometric_interpretation: u16,
    /// True if LinearRaw (pre-demosaiced RGB)
    pub is_linear_raw: bool,
    /// Tile width (0 if strip-based)
    pub tile_width: u32,
    /// Tile height (0 if strip-based)
    pub tile_height: u32,
    /// Tile offsets
    pub tile_offsets: Vec<u64>,
    /// Tile byte counts
    pub tile_byte_counts: Vec<u64>,
    /// Rows per strip (0 if tile-based)
    pub rows_per_strip: u32,
    /// Strip offsets
    pub strip_offsets: Vec<u64>,
    /// Strip byte counts
    pub strip_byte_counts: Vec<u64>,
    /// Color matrix 1 (XYZ to camera native, illuminant 1)
    pub color_matrix1: Option<[f64; 9]>,
    /// Color matrix 2 (XYZ to camera native, illuminant 2)
    pub color_matrix2: Option<[f64; 9]>,
    /// As-shot neutral white balance
    pub as_shot_neutral: Option<[f64; 3]>,
    /// Analog balance
    pub analog_balance: Option<[f64; 3]>,
    /// Black level per channel
    pub black_levels: Vec<u32>,
    /// White level per channel
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

/// Type alias for raw IFD location: (index in main chain, (parent_index, sub_index) if subifd)
type RawIfdLocation = (Option<usize>, Option<(usize, usize)>);

/// Parsed Adobe DNG file.
pub struct DngFile<R> {
    parser: TiffParser<R>,
    /// The main IFD chain
    ifds: Vec<Ifd>,
    /// Index of the IFD containing the raw image data
    raw_ifd_index: Option<usize>,
    /// Whether raw IFD is a SubIFD (and which parent/child index)
    raw_is_subifd: Option<(usize, usize)>,
    /// Extracted metadata
    metadata: Option<DngMetadata>,
}

impl<R: Read + Seek> DngFile<R> {
    /// Parse a DNG file.
    pub fn parse(reader: R) -> RawResult<Self> {
        let mut parser = TiffParser::new(reader)?;

        // Walk the IFD chain
        let ifds = parser.walk_ifd_chain()?;

        // Find the raw IFD (need parser access to read values correctly)
        let (raw_ifd_index, raw_is_subifd) = Self::find_raw_ifd_with_parser(&ifds, &mut parser)?;

        let mut dng = DngFile {
            parser,
            ifds,
            raw_ifd_index,
            raw_is_subifd,
            metadata: None,
        };

        // Extract metadata
        dng.extract_metadata()?;

        Ok(dng)
    }

    /// Find the IFD containing the raw image data.
    ///
    /// For DNG, we look for:
    /// - PhotometricInterpretation = LinearRaw (34892) or CFA (32803)
    /// - Largest dimensions
    fn find_raw_ifd_with_parser(
        ifds: &[Ifd],
        parser: &mut TiffParser<R>,
    ) -> RawResult<RawIfdLocation> {
        let mut best_match: Option<(usize, Option<usize>, u64)> = None;

        for (ifd_idx, ifd) in ifds.iter().enumerate() {
            // Check main IFD
            if let Some(entry) = ifd.get(TiffTag::PhotometricInterpretation) {
                let value = parser.read_value(entry)?;
                let photometric = value.as_u32().unwrap_or(0) as u16;

                // LinearRaw (34892) or CFA (32803)
                if photometric == 34892 || photometric == 32803 {
                    let width = if let Some(entry) = ifd.get(TiffTag::ImageWidth) {
                        parser.read_value(entry)?.as_u32().unwrap_or(0)
                    } else {
                        0
                    };
                    let height = if let Some(entry) = ifd.get(TiffTag::ImageLength) {
                        parser.read_value(entry)?.as_u32().unwrap_or(0)
                    } else {
                        0
                    };
                    let pixel_count = width as u64 * height as u64;

                    if best_match.is_none() || best_match.as_ref().unwrap().2 < pixel_count {
                        best_match = Some((ifd_idx, None, pixel_count));
                    }
                }
            }

            // Check SubIFDs
            for (sub_idx, sub_ifd) in ifd.sub_ifds.iter().enumerate() {
                if let Some(entry) = sub_ifd.get(TiffTag::PhotometricInterpretation) {
                    let value = parser.read_value(entry)?;
                    let photometric = value.as_u32().unwrap_or(0) as u16;

                    if photometric == 34892 || photometric == 32803 {
                        let width = if let Some(entry) = sub_ifd.get(TiffTag::ImageWidth) {
                            parser.read_value(entry)?.as_u32().unwrap_or(0)
                        } else {
                            0
                        };
                        let height = if let Some(entry) = sub_ifd.get(TiffTag::ImageLength) {
                            parser.read_value(entry)?.as_u32().unwrap_or(0)
                        } else {
                            0
                        };
                        let pixel_count = width as u64 * height as u64;

                        if best_match.is_none() || best_match.as_ref().unwrap().2 < pixel_count {
                            best_match = Some((ifd_idx, Some(sub_idx), pixel_count));
                        }
                    }
                }
            }
        }

        Ok(match best_match {
            Some((ifd_idx, Some(sub_idx), _)) => (Some(ifd_idx), Some((ifd_idx, sub_idx))),
            Some((ifd_idx, None, _)) => (Some(ifd_idx), None),
            None => (None, None),
        })
    }

    /// Get the raw IFD.
    fn raw_ifd(&self) -> Option<&Ifd> {
        if let Some((parent_idx, sub_idx)) = self.raw_is_subifd {
            Some(&self.ifds[parent_idx].sub_ifds[sub_idx])
        } else if let Some(idx) = self.raw_ifd_index {
            Some(&self.ifds[idx])
        } else {
            None
        }
    }

    /// Get IFD0 (main IFD).
    fn ifd0(&self) -> Option<&Ifd> {
        self.ifds.first()
    }

    /// Get the extracted metadata.
    pub fn metadata(&self) -> Option<&DngMetadata> {
        self.metadata.as_ref()
    }

    /// Extract metadata from the parsed IFDs.
    fn extract_metadata(&mut self) -> RawResult<()> {
        let ifd0 = self.ifd0().cloned().ok_or_else(|| {
            RawError::Parse(ParseError::InvalidIfd {
                offset: 0,
                reason: "No IFD0 found".to_string(),
            })
        })?;

        // Extract Make
        let make = if let Some(entry) = ifd0.get(TiffTag::Make) {
            let value = self.parser.read_value(entry)?;
            value.as_str().unwrap_or("").trim().to_string()
        } else {
            String::new()
        };

        // Extract Model
        let model = if let Some(entry) = ifd0.get(TiffTag::Model) {
            let value = self.parser.read_value(entry)?;
            value.as_str().unwrap_or("").trim().to_string()
        } else {
            String::new()
        };

        // Extract DNG Version
        let dng_version = if let Some(entry) = ifd0.get(TiffTag::DNGVersion) {
            let value = self.parser.read_value(entry)?;
            if let TiffValue::Bytes(bytes) = value {
                if bytes.len() >= 4 {
                    [bytes[0], bytes[1], bytes[2], bytes[3]]
                } else {
                    [1, 0, 0, 0]
                }
            } else {
                [1, 0, 0, 0]
            }
        } else {
            [1, 0, 0, 0]
        };

        // Extract Unique Camera Model
        let unique_camera_model = if let Some(entry) = ifd0.get(TiffTag::UniqueCameraModel) {
            let value = self.parser.read_value(entry)?;
            value.as_str().unwrap_or("").trim().to_string()
        } else {
            String::new()
        };

        // Get the raw IFD for dimension and compression info
        let raw_ifd = self
            .raw_ifd()
            .cloned()
            .ok_or_else(|| RawError::Unsupported("Could not find raw IFD".to_string()))?;

        // Extract dimensions
        let width = raw_ifd
            .get(TiffTag::ImageWidth)
            .map(|e| e.value_offset as u32)
            .ok_or(RawError::Parse(ParseError::TagNotFound(
                TiffTag::ImageWidth,
            )))?;

        let height = raw_ifd
            .get(TiffTag::ImageLength)
            .map(|e| e.value_offset as u32)
            .ok_or(RawError::Parse(ParseError::TagNotFound(
                TiffTag::ImageLength,
            )))?;

        let sensor_size = Size::new(width, height);

        // Extract bit depth (BitsPerSample may be array for LinearRaw)
        let bit_depth = if let Some(entry) = raw_ifd.get(TiffTag::BitsPerSample) {
            let value = self.parser.read_value(entry)?;
            value.first_u32().unwrap_or(16) as u8
        } else {
            16
        };

        // Extract samples per pixel
        let samples_per_pixel = if let Some(entry) = raw_ifd.get(TiffTag::SamplesPerPixel) {
            let value = self.parser.read_value(entry)?;
            value.as_u32().unwrap_or(1) as u8
        } else {
            1
        };

        // Extract compression
        let compression = if let Some(entry) = raw_ifd.get(TiffTag::Compression) {
            let value = self.parser.read_value(entry)?;
            value.as_u32().unwrap_or(1) as u16
        } else {
            1
        };

        // Extract photometric interpretation
        let photometric_interpretation =
            if let Some(entry) = raw_ifd.get(TiffTag::PhotometricInterpretation) {
                let value = self.parser.read_value(entry)?;
                value.as_u32().unwrap_or(32803) as u16
            } else {
                32803
            };

        let is_linear_raw = photometric_interpretation == 34892;

        // Extract tile dimensions
        let tile_width = if let Some(entry) = raw_ifd.get(TiffTag::TileWidth) {
            let value = self.parser.read_value(entry)?;
            value.as_u32().unwrap_or(0)
        } else {
            0
        };

        let tile_height = if let Some(entry) = raw_ifd.get(TiffTag::TileLength) {
            let value = self.parser.read_value(entry)?;
            value.as_u32().unwrap_or(0)
        } else {
            0
        };

        // Extract tile offsets
        let tile_offsets = if let Some(entry) = raw_ifd.get(TiffTag::TileOffsets) {
            let value = self.parser.read_value(entry)?;
            value
                .as_u32_vec()
                .map(|v| v.into_iter().map(|x| x as u64).collect())
                .or_else(|| value.as_u64_vec())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Extract tile byte counts
        let tile_byte_counts = if let Some(entry) = raw_ifd.get(TiffTag::TileByteCounts) {
            let value = self.parser.read_value(entry)?;
            value
                .as_u32_vec()
                .map(|v| v.into_iter().map(|x| x as u64).collect())
                .or_else(|| value.as_u64_vec())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Extract strip dimensions (for strip-based layout)
        let rows_per_strip = if let Some(entry) = raw_ifd.get(TiffTag::RowsPerStrip) {
            let value = self.parser.read_value(entry)?;
            value.as_u32().unwrap_or(0)
        } else {
            0
        };

        let strip_offsets = if let Some(entry) = raw_ifd.get(TiffTag::StripOffsets) {
            let value = self.parser.read_value(entry)?;
            value
                .as_u32_vec()
                .map(|v| v.into_iter().map(|x| x as u64).collect())
                .or_else(|| value.as_u64_vec())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let strip_byte_counts = if let Some(entry) = raw_ifd.get(TiffTag::StripByteCounts) {
            let value = self.parser.read_value(entry)?;
            value
                .as_u32_vec()
                .map(|v| v.into_iter().map(|x| x as u64).collect())
                .or_else(|| value.as_u64_vec())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Extract color matrices
        let color_matrix1 = self.extract_matrix(&ifd0, TiffTag::ColorMatrix1)?;
        let color_matrix2 = self.extract_matrix(&ifd0, TiffTag::ColorMatrix2)?;

        // Extract as-shot neutral (check IFD0 first, then Raw IFD)
        let as_shot_neutral = self
            .extract_triplet(&ifd0, TiffTag::AsShotNeutral)?
            .or(self.extract_triplet(&raw_ifd, TiffTag::AsShotNeutral)?);

        // Extract analog balance (check IFD0 first, then Raw IFD)
        let analog_balance = self
            .extract_triplet(&ifd0, TiffTag::AnalogBalance)?
            .or(self.extract_triplet(&raw_ifd, TiffTag::AnalogBalance)?);

        // Extract black levels
        let black_levels = if let Some(entry) = raw_ifd.get(TiffTag::BlackLevel) {
            let value = self.parser.read_value(entry)?;
            value.as_u32_vec().unwrap_or_default()
        } else {
            vec![0; samples_per_pixel as usize]
        };

        // Extract white levels
        let white_levels = if let Some(entry) = raw_ifd.get(TiffTag::WhiteLevel) {
            let value = self.parser.read_value(entry)?;
            value.as_u32_vec().unwrap_or_default()
        } else {
            vec![
                1u32.checked_shl(bit_depth as u32)
                    .unwrap_or(0)
                    .wrapping_sub(1);
                samples_per_pixel as usize
            ]
        };

        // Extract CFA pattern (only for non-LinearRaw)
        let cfa_pattern = if !is_linear_raw {
            if let Some(entry) = raw_ifd.get(TiffTag::CFAPattern) {
                let value = self.parser.read_value(entry)?;
                if let TiffValue::Bytes(bytes) = value {
                    if bytes.len() >= 4 {
                        CfaPattern::from_array([bytes[0], bytes[1], bytes[2], bytes[3]])
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Extract linearization table
        let linearization_table = if let Some(entry) = raw_ifd.get(TiffTag::LinearizationTable) {
            let value = self.parser.read_value(entry)?;
            if let TiffValue::Shorts(shorts) = value {
                Some(shorts)
            } else {
                None
            }
        } else {
            None
        };

        // Extract baseline exposure
        let baseline_exposure = if let Some(entry) = ifd0.get(TiffTag::BaselineExposure) {
            let value = self.parser.read_value(entry)?;
            value
                .as_f64_vec()
                .and_then(|v| v.first().copied().map(|x| x as f32))
        } else {
            None
        };

        // Extract active area
        let active_area = if let Some(entry) = raw_ifd.get(TiffTag::ActiveArea) {
            let value = self.parser.read_value(entry)?;
            if let Some(vec) = value.as_u32_vec() {
                if vec.len() >= 4 {
                    // ActiveArea is [top, left, bottom, right]
                    Some(Rect::from_coords(
                        vec[1],
                        vec[0],
                        vec[3] - vec[1],
                        vec[2] - vec[0],
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Extract default crop
        let default_crop_origin = if let Some(entry) = raw_ifd.get(TiffTag::DefaultCropOrigin) {
            let value = self.parser.read_value(entry)?;
            if let Some(vec) = value.as_u32_vec() {
                if vec.len() >= 2 {
                    Some((vec[0], vec[1]))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let default_crop_size = if let Some(entry) = raw_ifd.get(TiffTag::DefaultCropSize) {
            let value = self.parser.read_value(entry)?;
            if let Some(vec) = value.as_u32_vec() {
                if vec.len() >= 2 {
                    Some((vec[0], vec[1]))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Extract EXIF/GPS/DateTime/orientation from IFD0
        use crate::tiff::metadata_helper;
        let exif = metadata_helper::extract_exif(&mut self.parser, &ifd0);
        let datetime = metadata_helper::extract_datetime(&mut self.parser, &ifd0);
        let gps = metadata_helper::extract_gps(&mut self.parser, &ifd0);
        let (lens_make, lens_model) = metadata_helper::extract_lens_info(&mut self.parser, &ifd0);
        let orientation = metadata_helper::extract_orientation(&mut self.parser, &ifd0);

        // Extract DNG-specific calibration fields
        let calibration_illuminant_1 =
            if let Some(entry) = ifd0.get(TiffTag::CalibrationIlluminant1) {
                self.parser
                    .read_value(entry)
                    .ok()
                    .and_then(|v| v.as_u32().map(|x| x as u16))
            } else {
                raw_ifd
                    .get(TiffTag::CalibrationIlluminant1)
                    .and_then(|e| self.parser.read_value(e).ok())
                    .and_then(|v| v.as_u32().map(|x| x as u16))
            };
        let calibration_illuminant_2 =
            if let Some(entry) = ifd0.get(TiffTag::CalibrationIlluminant2) {
                self.parser
                    .read_value(entry)
                    .ok()
                    .and_then(|v| v.as_u32().map(|x| x as u16))
            } else {
                raw_ifd
                    .get(TiffTag::CalibrationIlluminant2)
                    .and_then(|e| self.parser.read_value(e).ok())
                    .and_then(|v| v.as_u32().map(|x| x as u16))
            };

        // Extract noise profile
        let noise_profile = raw_ifd
            .get(TiffTag::NoiseProfile)
            .or_else(|| ifd0.get(TiffTag::NoiseProfile))
            .and_then(|e| self.parser.read_value(e).ok())
            .and_then(|v| v.as_f64_vec());

        // Extract profile info
        let profile_name = raw_ifd
            .get(TiffTag::ProfileName)
            .or_else(|| ifd0.get(TiffTag::ProfileName))
            .and_then(|e| self.parser.read_value(e).ok())
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        let profile_tone_curve = raw_ifd
            .get(TiffTag::ProfileToneCurve)
            .or_else(|| ifd0.get(TiffTag::ProfileToneCurve))
            .and_then(|e| self.parser.read_value(e).ok())
            .and_then(|v| match v {
                TiffValue::Floats(f) => Some(f),
                _ => v
                    .as_f64_vec()
                    .map(|d| d.into_iter().map(|x| x as f32).collect()),
            });

        // Extract opcode lists (stored as UNDEFINED bytes, big-endian binary format)
        let opcode_list1 = raw_ifd
            .get(TiffTag::OpcodeList1)
            .or_else(|| ifd0.get(TiffTag::OpcodeList1))
            .and_then(|e| self.parser.read_value(e).ok())
            .and_then(|v| match v {
                TiffValue::Undefined(b) | TiffValue::Bytes(b) => Some(b),
                _ => None,
            })
            .unwrap_or_default();
        let opcode_list2 = raw_ifd
            .get(TiffTag::OpcodeList2)
            .or_else(|| ifd0.get(TiffTag::OpcodeList2))
            .and_then(|e| self.parser.read_value(e).ok())
            .and_then(|v| match v {
                TiffValue::Undefined(b) | TiffValue::Bytes(b) => Some(b),
                _ => None,
            })
            .unwrap_or_default();
        let opcode_list3 = raw_ifd
            .get(TiffTag::OpcodeList3)
            .or_else(|| ifd0.get(TiffTag::OpcodeList3))
            .and_then(|e| self.parser.read_value(e).ok())
            .and_then(|v| match v {
                TiffValue::Undefined(b) | TiffValue::Bytes(b) => Some(b),
                _ => None,
            })
            .unwrap_or_default();

        if !opcode_list2.is_empty() {
            tracing::debug!("DNG OpcodeList2: {} bytes present", opcode_list2.len());
        }

        self.metadata = Some(DngMetadata {
            make,
            model,
            unique_camera_model,
            sensor_size,
            active_area,
            default_crop_origin,
            default_crop_size,
            bit_depth,
            samples_per_pixel,
            compression,
            photometric_interpretation,
            is_linear_raw,
            tile_width,
            tile_height,
            tile_offsets,
            tile_byte_counts,
            rows_per_strip,
            strip_offsets,
            strip_byte_counts,
            color_matrix1,
            color_matrix2,
            as_shot_neutral,
            analog_balance,
            black_levels,
            white_levels,
            dng_version,
            cfa_pattern,
            linearization_table,
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
            opcode_list1,
            opcode_list2,
            opcode_list3,
        });

        // Warn about unknown tags
        for tag in ifd0.other_tags.keys() {
            tracing::warn!("Unknown/Unimplemented tag 0x{:04X} in IFD0", tag);
        }
        for tag in raw_ifd.other_tags.keys() {
            tracing::warn!("Unknown/Unimplemented tag 0x{:04X} in Raw IFD", tag);
        }

        Ok(())
    }

    /// Extract a 3x3 matrix from an IFD.
    fn extract_matrix(&mut self, ifd: &Ifd, tag: TiffTag) -> RawResult<Option<[f64; 9]>> {
        if let Some(entry) = ifd.get(tag) {
            let value = self.parser.read_value(entry)?;
            if let Some(f64_vec) = value.as_f64_vec() {
                if f64_vec.len() >= 9 {
                    return Ok(Some([
                        f64_vec[0], f64_vec[1], f64_vec[2], f64_vec[3], f64_vec[4], f64_vec[5],
                        f64_vec[6], f64_vec[7], f64_vec[8],
                    ]));
                }
            }
        }
        Ok(None)
    }

    /// Extract a 3-element triplet from an IFD.
    fn extract_triplet(&mut self, ifd: &Ifd, tag: TiffTag) -> RawResult<Option<[f64; 3]>> {
        if let Some(entry) = ifd.get(tag) {
            let value = self.parser.read_value(entry)?;
            if let Some(f64_vec) = value.as_f64_vec() {
                if f64_vec.len() >= 3 {
                    return Ok(Some([f64_vec[0], f64_vec[1], f64_vec[2]]));
                }
            }
        }
        Ok(None)
    }

    /// Copy a decoded block (tile or strip) into the output buffer.
    #[allow(clippy::too_many_arguments)]
    fn copy_block_to_output(
        src: &[u16],
        src_width: usize,
        src_channels: usize,
        dst: &mut [u16],
        dst_width: usize,
        dst_channels: usize,
        block_x: usize,
        block_y: usize,
        copy_w: usize,
        copy_h: usize,
        linearization_table: &Option<Vec<u16>>,
    ) {
        for ty in 0..copy_h {
            for tx in 0..copy_w {
                let src_idx = (ty * src_width + tx) * src_channels;
                let dst_x = block_x + tx;
                let dst_y = block_y + ty;
                let dst_idx = (dst_y * dst_width + dst_x) * dst_channels;

                for c in 0..dst_channels.min(src_channels) {
                    if src_idx + c < src.len() && dst_idx + c < dst.len() {
                        let mut val = src[src_idx + c];
                        if let Some(table) = linearization_table {
                            if !table.is_empty() {
                                let index = (val as usize).min(table.len() - 1);
                                val = table[index];
                            }
                        }
                        dst[dst_idx + c] = val;
                    }
                }
            }
        }
    }

    /// Decode strip-based image data into the output buffer.
    fn decode_strips(
        &mut self,
        metadata: &DngMetadata,
        width: usize,
        height: usize,
        output_channels: usize,
        output: &mut [u16],
    ) -> RawResult<()> {
        let rows_per_strip = if metadata.rows_per_strip > 0 {
            metadata.rows_per_strip as usize
        } else {
            // Default: entire image is one strip
            height
        };

        let byte_order = self.parser.byte_order();

        for (strip_idx, (&strip_offset, &strip_size)) in metadata
            .strip_offsets
            .iter()
            .zip(metadata.strip_byte_counts.iter())
            .enumerate()
        {
            let strip_y = strip_idx * rows_per_strip;
            let strip_rows = rows_per_strip.min(height - strip_y);

            self.parser.seek_to(strip_offset)?;
            let strip_data = self.parser.read_bytes(strip_size as usize)?;

            match metadata.compression {
                1 => {
                    // Uncompressed
                    let pixels = Self::decode_uncompressed_strip(
                        &strip_data,
                        width,
                        strip_rows,
                        output_channels,
                        metadata.bit_depth,
                        byte_order,
                    )?;
                    Self::copy_block_to_output(
                        &pixels,
                        width,
                        output_channels,
                        output,
                        width,
                        output_channels,
                        0,
                        strip_y,
                        width,
                        strip_rows,
                        &metadata.linearization_table,
                    );
                }
                52546 => {
                    // JPEG XL compressed strip
                    let (decoded_width, decoded_height, channels, pixels) =
                        JxlDecoder::decode_tile_with_depth(&strip_data, metadata.bit_depth)?;

                    let actual_w = decoded_width.min(width);
                    let actual_h = decoded_height.min(strip_rows);

                    Self::copy_block_to_output(
                        &pixels,
                        decoded_width,
                        channels,
                        output,
                        width,
                        output_channels,
                        0,
                        strip_y,
                        actual_w,
                        actual_h,
                        &metadata.linearization_table,
                    );
                }
                other => {
                    return Err(RawError::Unsupported(format!(
                        "Unsupported DNG strip compression: {} (supported: 1=uncompressed, 52546=JPEG XL)",
                        other
                    )));
                }
            }
        }

        Ok(())
    }

    /// Decode an uncompressed strip from raw bytes to u16 pixel values.
    fn decode_uncompressed_strip(
        data: &[u8],
        width: usize,
        rows: usize,
        channels: usize,
        bit_depth: u8,
        byte_order: ByteOrder,
    ) -> RawResult<Vec<u16>> {
        let total_samples = width * rows * channels;
        let mut pixels = Vec::with_capacity(total_samples);

        match bit_depth {
            8 => {
                for &b in data.iter().take(total_samples) {
                    pixels.push(b as u16);
                }
            }
            16 => {
                let bytes_needed = total_samples * 2;
                if data.len() < bytes_needed {
                    return Err(RawError::Unsupported(format!(
                        "Strip data too short: {} bytes for {} 16-bit samples",
                        data.len(),
                        total_samples
                    )));
                }
                for i in 0..total_samples {
                    let offset = i * 2;
                    let val = match byte_order {
                        ByteOrder::LittleEndian => {
                            u16::from_le_bytes([data[offset], data[offset + 1]])
                        }
                        ByteOrder::BigEndian => {
                            u16::from_be_bytes([data[offset], data[offset + 1]])
                        }
                    };
                    pixels.push(val);
                }
            }
            12 | 14 => {
                // Packed bit depths: read as 16-bit and mask
                let bytes_needed = total_samples * 2;
                if data.len() >= bytes_needed {
                    // Data stored as 16-bit values with upper bits zeroed
                    for i in 0..total_samples {
                        let offset = i * 2;
                        let val = match byte_order {
                            ByteOrder::LittleEndian => {
                                u16::from_le_bytes([data[offset], data[offset + 1]])
                            }
                            ByteOrder::BigEndian => {
                                u16::from_be_bytes([data[offset], data[offset + 1]])
                            }
                        };
                        pixels.push(val);
                    }
                } else {
                    // Tightly packed bits
                    let total_bits = total_samples * bit_depth as usize;
                    let bytes_needed = total_bits.div_ceil(8);
                    if data.len() < bytes_needed {
                        return Err(RawError::Unsupported(format!(
                            "Strip data too short: {} bytes for {} {}-bit samples",
                            data.len(),
                            total_samples,
                            bit_depth
                        )));
                    }
                    let mut bit_pos: usize = 0;
                    for _ in 0..total_samples {
                        let byte_idx = bit_pos / 8;
                        let bit_offset = bit_pos % 8;
                        let mut val: u32 = 0;
                        let mut bits_remaining = bit_depth as usize;
                        let mut current_bit_offset = bit_offset;
                        let mut current_byte = byte_idx;
                        while bits_remaining > 0 {
                            let bits_in_byte = (8 - current_bit_offset).min(bits_remaining);
                            let mask = ((1u32 << bits_in_byte) - 1)
                                << (8 - current_bit_offset - bits_in_byte);
                            let extracted = (data[current_byte] as u32 & mask)
                                >> (8 - current_bit_offset - bits_in_byte);
                            val = (val << bits_in_byte) | extracted;
                            bits_remaining -= bits_in_byte;
                            current_bit_offset = 0;
                            current_byte += 1;
                        }
                        pixels.push(val as u16);
                        bit_pos += bit_depth as usize;
                    }
                }
            }
            _ => {
                return Err(RawError::Unsupported(format!(
                    "Unsupported bit depth for uncompressed strip: {}",
                    bit_depth
                )));
            }
        }

        // Pad if needed
        pixels.resize(total_samples, 0);
        Ok(pixels)
    }

    /// Extract the embedded JPEG thumbnail, if present.
    ///
    /// Searches IFDs for a thumbnail (NewSubfileType=1) or falls back to
    /// JPEGInterchangeFormat in IFD 0.
    pub fn thumbnail(&mut self) -> RawResult<Option<Vec<u8>>> {
        // Try each IFD looking for a thumbnail (NewSubfileType=1 with JPEG data)
        for ifd in &self.ifds.clone() {
            if let Some(entry) = ifd.get(TiffTag::NewSubfileType) {
                if entry.value_offset == 1 {
                    // This is a thumbnail IFD
                    if let Some(offset_entry) = ifd.get(TiffTag::JPEGInterchangeFormat) {
                        if let Some(length_entry) = ifd.get(TiffTag::JPEGInterchangeFormatLength) {
                            let offset_entry = offset_entry.clone();
                            let length_entry = length_entry.clone();
                            let offset = match self.parser.read_value(&offset_entry)? {
                                crate::tiff::TiffValue::Longs(v) if !v.is_empty() => v[0] as u64,
                                crate::tiff::TiffValue::Shorts(v) if !v.is_empty() => v[0] as u64,
                                _ => continue,
                            };
                            let length = match self.parser.read_value(&length_entry)? {
                                crate::tiff::TiffValue::Longs(v) if !v.is_empty() => v[0] as usize,
                                crate::tiff::TiffValue::Shorts(v) if !v.is_empty() => v[0] as usize,
                                _ => continue,
                            };
                            if length > 0 {
                                self.parser.seek_to(offset)?;
                                let data = self.parser.read_bytes(length)?;
                                return Ok(Some(data));
                            }
                        }
                    }
                }
            }
        }

        // Fallback: try IFD 0 JPEG tags
        let ifd0 = match self.ifd0() {
            Some(ifd) => ifd,
            None => return Ok(None),
        };
        let offset_entry = match ifd0.get(TiffTag::JPEGInterchangeFormat) {
            Some(e) => e.clone(),
            None => return Ok(None),
        };
        let length_entry = match ifd0.get(TiffTag::JPEGInterchangeFormatLength) {
            Some(e) => e.clone(),
            None => return Ok(None),
        };
        let offset = match self.parser.read_value(&offset_entry)? {
            crate::tiff::TiffValue::Longs(v) if !v.is_empty() => v[0] as u64,
            crate::tiff::TiffValue::Shorts(v) if !v.is_empty() => v[0] as u64,
            _ => return Ok(None),
        };
        let length = match self.parser.read_value(&length_entry)? {
            crate::tiff::TiffValue::Longs(v) if !v.is_empty() => v[0] as usize,
            crate::tiff::TiffValue::Shorts(v) if !v.is_empty() => v[0] as usize,
            _ => return Ok(None),
        };
        if length == 0 {
            return Ok(None);
        }
        self.parser.seek_to(offset)?;
        let data = self.parser.read_bytes(length)?;
        Ok(Some(data))
    }

    /// Decode the raw image data.
    ///
    /// For LinearRaw (iPhone ProRAW), this returns an RGB image directly.
    /// For CFA (Bayer) data, this returns a RawImage that needs demosaicing.
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        let metadata = self
            .metadata
            .as_ref()
            .cloned()
            .ok_or_else(|| RawError::Unsupported("Metadata not extracted".to_string()))?;

        let width = metadata.sensor_size.width as usize;
        let height = metadata.sensor_size.height as usize;

        // For LinearRaw, output is RGB (3 channels)
        // For CFA, output is single channel
        let output_channels = if metadata.is_linear_raw { 3 } else { 1 };

        let is_tile_based = !metadata.tile_offsets.is_empty();
        let is_strip_based = !metadata.strip_offsets.is_empty();

        if !is_tile_based && !is_strip_based {
            return Err(RawError::Unsupported(
                "No tile or strip data found".to_string(),
            ));
        }

        // Allocate output buffer
        let mut output = vec![0u16; width * height * output_channels];

        if is_tile_based {
            // Check compression type for tile-based (only JXL supported)
            if metadata.compression != 52546 {
                return Err(RawError::Unsupported(format!(
                    "Unsupported DNG compression: {} (only JPEG XL 52546 supported for tiles)",
                    metadata.compression
                )));
            }

            let tile_w = metadata.tile_width as usize;
            let tile_h = metadata.tile_height as usize;
            let tiles_x = width.div_ceil(tile_w);

            for (tile_idx, (&tile_offset, &tile_size)) in metadata
                .tile_offsets
                .iter()
                .zip(metadata.tile_byte_counts.iter())
                .enumerate()
            {
                let tile_col = tile_idx % tiles_x;
                let tile_row = tile_idx / tiles_x;
                let tile_x = tile_col * tile_w;
                let tile_y = tile_row * tile_h;

                self.parser.seek_to(tile_offset)?;
                let tile_data = self.parser.read_bytes(tile_size as usize)?;

                let (decoded_width, decoded_height, channels, tile_pixels) =
                    JxlDecoder::decode_tile_with_depth(&tile_data, metadata.bit_depth)?;

                if channels != output_channels {
                    tracing::warn!(
                        "Tile {} has {} channels, expected {}",
                        tile_idx,
                        channels,
                        output_channels
                    );
                }

                let actual_tile_w = decoded_width.min(width - tile_x);
                let actual_tile_h = decoded_height.min(height - tile_y);

                Self::copy_block_to_output(
                    &tile_pixels,
                    decoded_width,
                    channels,
                    &mut output,
                    width,
                    output_channels,
                    tile_x,
                    tile_y,
                    actual_tile_w,
                    actual_tile_h,
                    &metadata.linearization_table,
                );
            }
        } else {
            // Strip-based layout
            self.decode_strips(&metadata, width, height, output_channels, &mut output)?;
        }

        // Create RawImage
        let active_area =
            metadata
                .active_area
                .unwrap_or(Rect::from_coords(0, 0, width as u32, height as u32));
        let cfa_pattern = metadata.cfa_pattern.unwrap_or(CfaPattern::Rggb);

        // If linearization table is applied, the effective bit depth is usually 16-bit
        let output_bit_depth = if metadata
            .linearization_table
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
        {
            16
        } else {
            metadata.bit_depth
        };

        let black_levels = [
            metadata.black_levels.first().copied().unwrap_or(0) as u16,
            metadata.black_levels.get(1).copied().unwrap_or(0) as u16,
            metadata.black_levels.get(2).copied().unwrap_or(0) as u16,
            metadata.black_levels.get(3).copied().unwrap_or(0) as u16,
        ];
        let white_level = metadata.white_levels.first().copied().unwrap_or(65535) as u16;
        let default_crop = if let (Some(origin), Some(size)) =
            (metadata.default_crop_origin, metadata.default_crop_size)
        {
            Some(Rect::from_coords(origin.0, origin.1, size.0, size.1))
        } else {
            None
        };

        // If LinearRaw, the data is already RGB interleaved
        let final_bit_depth = if metadata.is_linear_raw {
            metadata.bit_depth
        } else {
            output_bit_depth
        };

        let mut builder = RawImage::builder(
            metadata.sensor_size,
            active_area,
            final_bit_depth,
            cfa_pattern,
        )
        .black_levels(black_levels)
        .white_level(white_level)
        .data(output);
        if let Some(be) = metadata.baseline_exposure {
            builder = builder.baseline_exposure(be);
        }
        if let Some(crop) = default_crop {
            builder = builder.default_crop(crop);
        }
        let raw_image = builder.build();

        Ok(raw_image)
    }

    /// Decode LinearRaw data directly to an RGB image.
    ///
    /// This is the preferred method for iPhone ProRAW files
    /// which are already demosaiced.
    pub fn decode_linear_raw(&mut self) -> RawResult<RgbImage> {
        let metadata = self
            .metadata
            .as_ref()
            .cloned()
            .ok_or_else(|| RawError::Unsupported("Metadata not extracted".to_string()))?;

        if !metadata.is_linear_raw {
            return Err(RawError::Unsupported(
                "Not a LinearRaw DNG file".to_string(),
            ));
        }

        let width = metadata.sensor_size.width as usize;
        let height = metadata.sensor_size.height as usize;

        let is_tile_based = !metadata.tile_offsets.is_empty();
        let is_strip_based = !metadata.strip_offsets.is_empty();

        if !is_tile_based && !is_strip_based {
            return Err(RawError::Unsupported(
                "No tile or strip data found".to_string(),
            ));
        }

        let active_area =
            metadata
                .active_area
                .unwrap_or(Rect::from_coords(0, 0, width as u32, height as u32));
        let out_width = active_area.size.width as usize;
        let out_height = active_area.size.height as usize;
        let offset_x = active_area.origin.x as usize;
        let offset_y = active_area.origin.y as usize;

        // Allocate full sensor buffer, then crop to active area
        let mut sensor_buf = vec![0u16; width * height * 3];

        if is_tile_based {
            if metadata.compression != 52546 {
                return Err(RawError::Unsupported(format!(
                    "Unsupported DNG compression: {} (only JPEG XL 52546 supported for tiles)",
                    metadata.compression
                )));
            }

            let tile_w = metadata.tile_width as usize;
            let tile_h = metadata.tile_height as usize;
            let tiles_x = width.div_ceil(tile_w);

            for (tile_idx, (&tile_offset, &tile_size)) in metadata
                .tile_offsets
                .iter()
                .zip(metadata.tile_byte_counts.iter())
                .enumerate()
            {
                let tile_col = tile_idx % tiles_x;
                let tile_row = tile_idx / tiles_x;
                let tile_x = tile_col * tile_w;
                let tile_y = tile_row * tile_h;

                self.parser.seek_to(tile_offset)?;
                let tile_data = self.parser.read_bytes(tile_size as usize)?;

                let (decoded_width, decoded_height, channels, tile_pixels) =
                    JxlDecoder::decode_tile(&tile_data)?;

                let actual_tile_w = decoded_width.min(width - tile_x);
                let actual_tile_h = decoded_height.min(height - tile_y);

                Self::copy_block_to_output(
                    &tile_pixels,
                    decoded_width,
                    channels,
                    &mut sensor_buf,
                    width,
                    3,
                    tile_x,
                    tile_y,
                    actual_tile_w,
                    actual_tile_h,
                    &metadata.linearization_table,
                );
            }
        } else {
            self.decode_strips(&metadata, width, height, 3, &mut sensor_buf)?;
        }

        // Crop to active area
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
                output[dst_idx..dst_idx + 3].copy_from_slice(&sensor_buf[src_idx..src_idx + 3]);
            }
        }

        let mut image = RgbImage::new(out_width as u32, out_height as u32, output);
        image.set_baseline_exposure(metadata.baseline_exposure);
        image.set_default_crop(
            if let (Some(origin), Some(size)) =
                (metadata.default_crop_origin, metadata.default_crop_size)
            {
                Some(Rect::from_coords(origin.0, origin.1, size.0, size.1))
            } else {
                None
            },
        );

        // Apply OpcodeList2 — defined as corrections applied to linear raw (post-demosaic) data.
        // This is where GainMap (lens shading correction) lives for iPhone ProRAW.
        if !metadata.opcode_list2.is_empty() {
            let opcode_list = crate::transforms::opcodes::OpcodeList::parse(&metadata.opcode_list2);
            opcode_list.apply_to_rgb(&mut image);
        }

        Ok(image)
    }
}

impl<R: Read + Seek> crate::core::MetadataExtractor for DngFile<R> {
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

    use crate::tiff::writer::{IfdEntry, TiffWriter};

    fn test_data_path(filename: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(filename)
    }

    fn throw_if_no_test_data(path: &PathBuf) {
        if !path.exists() {
            panic!("Test data at {:?} not found", path);
        }
    }

    /// Build a minimal synthetic strip-based DNG (CFA, uncompressed, 16-bit) in memory.
    /// Returns the raw TIFF bytes.
    fn build_strip_dng(
        width: u32,
        height: u32,
        rows_per_strip: u32,
        pixel_data: &[u16],
    ) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        let mut writer = TiffWriter::new(&mut buf, ByteOrder::LittleEndian);

        // Write TIFF header
        writer.write_header().unwrap();

        // Write pixel data first so we know the strip offsets
        let num_strips = height.div_ceil(rows_per_strip) as usize;
        let mut offsets = Vec::with_capacity(num_strips);
        let mut byte_counts = Vec::with_capacity(num_strips);

        for strip_idx in 0..num_strips {
            let strip_y = strip_idx as u32 * rows_per_strip;
            let strip_rows = rows_per_strip.min(height - strip_y) as usize;
            let samples_in_strip = width as usize * strip_rows;

            let start_sample = strip_y as usize * width as usize;
            let end_sample = start_sample + samples_in_strip;
            let strip_slice = &pixel_data[start_sample..end_sample];

            let (offset, count) = writer.write_image_strip_rgb16(strip_slice).unwrap();
            offsets.push(offset as u32);
            byte_counts.push(count as u32);
        }

        // Record where the IFD will start, then update the header pointer
        let ifd_offset = writer.position();
        writer.update_ifd0_offset(ifd_offset as u32).unwrap();

        let dng_version: [u8; 4] = [1, 4, 0, 0];

        let mut entries = vec![
            IfdEntry::long(TiffTag::ImageWidth, width),
            IfdEntry::long(TiffTag::ImageLength, height),
            IfdEntry::short(TiffTag::BitsPerSample, 16),
            IfdEntry::short(TiffTag::Compression, 1),
            IfdEntry::short(TiffTag::PhotometricInterpretation, 32803),
            IfdEntry::ascii(TiffTag::Make, "TestCam"),
            IfdEntry::ascii(TiffTag::Model, "StripTest"),
            IfdEntry::longs(TiffTag::StripOffsets, &offsets),
            IfdEntry::short(TiffTag::SamplesPerPixel, 1),
            IfdEntry::long(TiffTag::RowsPerStrip, rows_per_strip),
            IfdEntry::longs(TiffTag::StripByteCounts, &byte_counts),
            IfdEntry::bytes(TiffTag::DNGVersion, &dng_version),
            IfdEntry::bytes(TiffTag::CFAPattern, &[0, 1, 1, 2]),
        ];

        writer.write_ifd(&mut entries, 0).unwrap();

        buf.into_inner()
    }

    #[test]
    fn test_strip_dng_parse_metadata() {
        let width = 8u32;
        let height = 6u32;
        let rows_per_strip = 2u32;
        let pixel_data = vec![1000u16; (width * height) as usize];
        let dng_bytes = build_strip_dng(width, height, rows_per_strip, &pixel_data);

        let reader = Cursor::new(dng_bytes);
        let dng = DngFile::parse(reader).unwrap();
        let meta = dng.metadata().unwrap();

        assert_eq!(meta.sensor_size.width, width);
        assert_eq!(meta.sensor_size.height, height);
        assert_eq!(meta.compression, 1);
        assert_eq!(meta.rows_per_strip, rows_per_strip);
        assert_eq!(meta.strip_offsets.len(), 3); // ceil(6/2) = 3 strips
        assert_eq!(meta.strip_byte_counts.len(), 3);
        assert!(meta.tile_offsets.is_empty());
        assert_eq!(meta.bit_depth, 16);
        assert!(!meta.is_linear_raw);
    }

    #[test]
    fn test_strip_dng_decode_raw() {
        let width = 8u32;
        let height = 6u32;
        let rows_per_strip = 2u32;
        // Fill with a known pattern: pixel value = row * width + col + 100
        let mut pixel_data = vec![0u16; (width * height) as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                pixel_data[y * width as usize + x] = (y * width as usize + x + 100) as u16;
            }
        }

        let dng_bytes = build_strip_dng(width, height, rows_per_strip, &pixel_data);

        let reader = Cursor::new(dng_bytes);
        let mut dng = DngFile::parse(reader).unwrap();

        let raw_image = dng.decode_raw().unwrap();

        assert_eq!(raw_image.size().width, width);
        assert_eq!(raw_image.size().height, height);
        assert_eq!(raw_image.data.len(), (width * height) as usize);

        // Verify all pixel values match the input
        for y in 0..height as usize {
            for x in 0..width as usize {
                let idx = y * width as usize + x;
                let expected = (y * width as usize + x + 100) as u16;
                assert_eq!(
                    raw_image.data[idx], expected,
                    "Pixel mismatch at ({}, {}): got {}, expected {}",
                    x, y, raw_image.data[idx], expected
                );
            }
        }
    }

    #[test]
    fn test_strip_dng_single_strip() {
        // Entire image in one strip (rows_per_strip >= height)
        let width = 4u32;
        let height = 4u32;
        let rows_per_strip = 4u32;
        let pixel_data: Vec<u16> = (0..16).map(|i| i * 100 + 500).collect();

        let dng_bytes = build_strip_dng(width, height, rows_per_strip, &pixel_data);

        let reader = Cursor::new(dng_bytes);
        let mut dng = DngFile::parse(reader).unwrap();

        let meta = dng.metadata().unwrap();
        assert_eq!(meta.strip_offsets.len(), 1);

        let raw_image = dng.decode_raw().unwrap();
        assert_eq!(raw_image.data, pixel_data);
    }

    #[test]
    fn test_strip_dng_uneven_strips() {
        // Height not evenly divisible by rows_per_strip
        let width = 4u32;
        let height = 7u32;
        let rows_per_strip = 3u32;
        // 3 strips: rows 0-2, 3-5, 6 (last strip has 1 row)
        let pixel_data: Vec<u16> = (0..(width * height) as u16).map(|i| i + 200).collect();

        let dng_bytes = build_strip_dng(width, height, rows_per_strip, &pixel_data);

        let reader = Cursor::new(dng_bytes);
        let mut dng = DngFile::parse(reader).unwrap();

        let meta = dng.metadata().unwrap();
        assert_eq!(meta.strip_offsets.len(), 3); // ceil(7/3) = 3

        let raw_image = dng.decode_raw().unwrap();
        assert_eq!(raw_image.data, pixel_data);
    }

    #[test]
    fn test_dng_parse_iphone() {
        let path = test_data_path("Apple/iPhone_17_Pro_Max/IMG_1347.DNG");
        throw_if_no_test_data(&path);

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

        // Validate bit depth
        assert_eq!(metadata.bit_depth, 10, "Should be 10-bit");

        // Validate tiled
        assert!(metadata.tile_width > 0, "Should be tiled");
        assert!(metadata.tile_height > 0, "Should be tiled");
    }

    #[test]
    fn test_dng_decode_iphone() {
        let path = test_data_path("Apple/iPhone_17_Pro_Max/IMG_1347.DNG");
        throw_if_no_test_data(&path);

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
        assert_eq!(rgb_image.data.len(), expected_size);

        // Check that we got some non-zero pixel data
        let non_zero_count = rgb_image.data.iter().filter(|&&v| v > 0).count();
        assert!(non_zero_count > 0, "Should have non-zero pixel values");
    }
}
