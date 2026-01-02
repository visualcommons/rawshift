//! Adobe DNG (Digital Negative) format support.
//!
//! This module provides parsing for DNG files, particularly DNG 1.7+
//! with JPEG XL compression as used by iPhone ProRAW.

use std::io::{Read, Seek};

use crate::codecs::jxl::JxlDecoder;
use crate::core::image::{CfaPattern, RawImage, Rect, RgbImage, Size};
use crate::error::{RawError, RawResult};
use crate::tiff::{Ifd, TiffParser, TiffTag, TiffValue};

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
        let ifd0 = self.ifd0().cloned().ok_or_else(|| RawError::InvalidIfd {
            offset: 0,
            reason: "No IFD0 found".to_string(),
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
            .ok_or_else(|| RawError::UnsupportedFormat("Could not find raw IFD".to_string()))?;

        // Extract dimensions
        let width = raw_ifd
            .get(TiffTag::ImageWidth)
            .map(|e| e.value_offset as u32)
            .ok_or(RawError::TagNotFound(TiffTag::ImageWidth))?;

        let height = raw_ifd
            .get(TiffTag::ImageLength)
            .map(|e| e.value_offset as u32)
            .ok_or(RawError::TagNotFound(TiffTag::ImageLength))?;

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
            vec![(1u32 << bit_depth) - 1; samples_per_pixel as usize]
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
        });

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

    /// Decode the raw image data.
    ///
    /// For LinearRaw (iPhone ProRAW), this returns an RGB image directly.
    /// For CFA (Bayer) data, this returns a RawImage that needs demosaicing.
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        let metadata = self
            .metadata
            .as_ref()
            .cloned()
            .ok_or_else(|| RawError::UnsupportedFormat("Metadata not extracted".to_string()))?;

        // Check compression type
        if metadata.compression != 52546 {
            return Err(RawError::UnsupportedFormat(format!(
                "Unsupported DNG compression: {} (only JPEG XL 52546 supported)",
                metadata.compression
            )));
        }

        let width = metadata.sensor_size.width as usize;
        let height = metadata.sensor_size.height as usize;

        // For LinearRaw, output is RGB (3 channels)
        // For CFA, output is single channel
        let output_channels = if metadata.is_linear_raw { 3 } else { 1 };

        // Decode tiles
        if metadata.tile_offsets.is_empty() {
            return Err(RawError::UnsupportedFormat(
                "No tile data found (strip-based not yet supported)".to_string(),
            ));
        }

        let tile_w = metadata.tile_width as usize;
        let tile_h = metadata.tile_height as usize;
        let tiles_x = width.div_ceil(tile_w);
        let _tiles_y = height.div_ceil(tile_h);

        // Allocate output buffer
        let mut output = vec![0u16; width * height * output_channels];

        for (tile_idx, (&tile_offset, &tile_size)) in metadata
            .tile_offsets
            .iter()
            .zip(metadata.tile_byte_counts.iter())
            .enumerate()
        {
            // Calculate tile position
            let tile_col = tile_idx % tiles_x;
            let tile_row = tile_idx / tiles_x;
            let tile_x = tile_col * tile_w;
            let tile_y = tile_row * tile_h;

            // Read tile data
            self.parser.seek_to(tile_offset)?;
            let tile_data = self.parser.read_bytes(tile_size as usize)?;

            // Decode JXL tile
            let (decoded_width, decoded_height, channels, tile_pixels) =
                JxlDecoder::decode_tile_with_depth(&tile_data, metadata.bit_depth)?;

            // Validate decoded dimensions
            if channels != output_channels {
                tracing::warn!(
                    "Tile {} has {} channels, expected {}",
                    tile_idx,
                    channels,
                    output_channels
                );
            }

            // Copy tile pixels to output at correct position
            let actual_tile_w = decoded_width.min(width - tile_x);
            let actual_tile_h = decoded_height.min(height - tile_y);

            for ty in 0..actual_tile_h {
                for tx in 0..actual_tile_w {
                    let src_idx = (ty * decoded_width + tx) * channels;
                    let dst_x = tile_x + tx;
                    let dst_y = tile_y + ty;
                    let dst_idx = (dst_y * width + dst_x) * output_channels;

                    for c in 0..output_channels.min(channels) {
                        if src_idx + c < tile_pixels.len() && dst_idx + c < output.len() {
                            let mut val = tile_pixels[src_idx + c];
                            // Apply linearization table if present
                            if let Some(table) = &metadata.linearization_table {
                                if !table.is_empty() {
                                    let index = (val as usize).min(table.len() - 1);
                                    val = table[index];
                                }
                            }
                            output[dst_idx + c] = val;
                        }
                    }
                }
            }
        }

        // Create RawImage
        // For LinearRaw, we still create a RawImage but mark it appropriately
        let active_area =
            metadata
                .active_area
                .unwrap_or(Rect::from_coords(0, 0, width as u32, height as u32));
        let cfa_pattern = metadata.cfa_pattern.unwrap_or(CfaPattern::Rggb);

        // If linearization table is applied, the effective bit depth is usually 16-bit
        // (or determined by the table). We'll assume 16-bit to avoid double scaling later.
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

        let mut raw_image = RawImage {
            size: metadata.sensor_size,
            active_area,
            bit_depth: output_bit_depth,
            cfa_pattern,
            black_levels: [
                metadata.black_levels.first().copied().unwrap_or(0) as u16,
                metadata.black_levels.get(1).copied().unwrap_or(0) as u16,
                metadata.black_levels.get(2).copied().unwrap_or(0) as u16,
                metadata.black_levels.get(3).copied().unwrap_or(0) as u16,
            ],
            white_level: metadata.white_levels.first().copied().unwrap_or(65535) as u16,
            data: output,
            baseline_exposure: metadata.baseline_exposure,
            default_crop: if let (Some(origin), Some(size)) =
                (metadata.default_crop_origin, metadata.default_crop_size)
            {
                Some(Rect::from_coords(origin.0, origin.1, size.0, size.1))
            } else {
                None
            },
        };

        // If LinearRaw, the data is already RGB interleaved
        // We need to handle this specially in the export pipeline
        if metadata.is_linear_raw {
            // For LinearRaw, the data layout is different - it's RGB interleaved
            // We'll need to update the RawFile export to handle this
            raw_image.bit_depth = metadata.bit_depth;
        }

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
            .ok_or_else(|| RawError::UnsupportedFormat("Metadata not extracted".to_string()))?;

        if !metadata.is_linear_raw {
            return Err(RawError::UnsupportedFormat(
                "Not a LinearRaw DNG file".to_string(),
            ));
        }

        // Check compression type
        if metadata.compression != 52546 {
            return Err(RawError::UnsupportedFormat(format!(
                "Unsupported DNG compression: {} (only JPEG XL 52546 supported)",
                metadata.compression
            )));
        }

        let width = metadata.sensor_size.width as usize;
        let height = metadata.sensor_size.height as usize;

        // Decode tiles
        if metadata.tile_offsets.is_empty() {
            return Err(RawError::UnsupportedFormat(
                "No tile data found".to_string(),
            ));
        }

        let tile_w = metadata.tile_width as usize;
        let tile_h = metadata.tile_height as usize;
        let tiles_x = width.div_ceil(tile_w);
        let active_area =
            metadata
                .active_area
                .unwrap_or(Rect::from_coords(0, 0, width as u32, height as u32));
        let out_width = active_area.size.width as usize;
        let out_height = active_area.size.height as usize;
        let offset_x = active_area.origin.x as usize;
        let offset_y = active_area.origin.y as usize;

        // Allocate output buffer (RGB interleaved)
        let mut output = vec![0u16; out_width * out_height * 3];

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

            for ty in 0..actual_tile_h {
                let y_in_sensor = tile_y + ty;
                if y_in_sensor < offset_y || y_in_sensor >= offset_y + out_height {
                    // Skip this tile row
                    continue;
                }
                let dst_y = y_in_sensor - offset_y;

                for tx in 0..actual_tile_w {
                    let x_in_sensor = tile_x + tx;
                    if x_in_sensor < offset_x || x_in_sensor >= offset_x + out_width {
                        // Skip this tile column
                        continue;
                    }
                    let dst_x = x_in_sensor - offset_x;

                    let src_idx = (ty * decoded_width + tx) * channels;
                    let dst_idx = (dst_y * out_width + dst_x) * 3;

                    for c in 0..3.min(channels) {
                        if src_idx + c < tile_pixels.len() && dst_idx + c < output.len() {
                            let mut val = tile_pixels[src_idx + c];
                            // Apply linearization table if present
                            if let Some(table) = &metadata.linearization_table {
                                if !table.is_empty() {
                                    let index = (val as usize).min(table.len() - 1);
                                    val = table[index];
                                }
                            }
                            output[dst_idx + c] = val;
                        }
                    }
                }
            }
        }

        let mut image = RgbImage::new(out_width as u32, out_height as u32, output);
        image.baseline_exposure = metadata.baseline_exposure;
        image.default_crop = if let (Some(origin), Some(size)) =
            (metadata.default_crop_origin, metadata.default_crop_size)
        {
            Some(Rect::from_coords(origin.0, origin.1, size.0, size.1))
        } else {
            None
        };
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
                lens_make: None,  // TODO: Extract from EXIF
                lens_model: None, // TODO: Extract from EXIF
                lens_info: None,
                serial_number: None,
            },
            exif: ExifInfo::default(),         // TODO: Parse EXIF IFD
            datetime: DateTimeInfo::default(), // TODO: Parse EXIF IFD
            gps: GpsInfo::default(),           // TODO: Parse GPS IFD
            dng_color: DngColorInfo {
                color_matrix_1: m.and_then(|x| x.color_matrix1),
                color_matrix_2: m.and_then(|x| x.color_matrix2),
                calibration_illuminant_1: None, // TODO: Extract from IFD
                calibration_illuminant_2: None, // TODO: Extract from IFD
                as_shot_neutral: m.and_then(|x| x.as_shot_neutral),
                analog_balance: m.and_then(|x| x.analog_balance),
                white_balance: None,
                color_temperature: None,
            },
            dng_calibration: DngCalibrationInfo {
                baseline_exposure: m.and_then(|x| x.baseline_exposure.map(|v| v as f64)),
                baseline_noise: None,
                baseline_sharpness: None,
                noise_profile: None, // TODO: Extract NoiseProfile tag
                noise_reduction_applied: None,
            },
            dng_profile: DngProfileInfo::default(), // TODO: Extract ProfileName, ProfileToneCurve
            image: ImageInfo {
                orientation: None, // TODO: Extract from IFD0
                bit_depth: m.map(|x| x.bit_depth).unwrap_or(16),
                black_levels: m.map(|x| x.black_levels.clone()).unwrap_or_default(),
                white_level: m.and_then(|x| x.white_levels.first().copied()),
                default_crop_origin: m.and_then(|x| x.default_crop_origin),
                default_crop_size: m.and_then(|x| x.default_crop_size),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::BufReader;
    use std::path::PathBuf;

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
        assert_eq!(rgb_image.width, 8064);
        assert_eq!(rgb_image.height, 6048);

        // Validate data size (width * height * 3 channels)
        let expected_size = 8064 * 6048 * 3;
        assert_eq!(rgb_image.data.len(), expected_size);

        // Check that we got some non-zero pixel data
        let non_zero_count = rgb_image.data.iter().filter(|&&v| v > 0).count();
        assert!(non_zero_count > 0, "Should have non-zero pixel values");
    }
}
