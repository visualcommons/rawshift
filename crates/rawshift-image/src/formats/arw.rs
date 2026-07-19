//! Sony ARW format support.
//!
//! This module provides parsing for Sony Alpha Raw (ARW) files,
//! which are based on the TIFF container format with Sony-specific extensions.
//!
//! IFD structure walking is backed by [`gamut_ifd`]; Sony tag semantics stay
//! here (see [`super::ifd::tags`]).

use std::io::{Read, Seek};
use std::marker::PhantomData;

use gamut_ifd::{ByteOrder, Ifd, IfdReader, Value, Variant};

use super::ifd::{self, tags};
use crate::core::image::{CfaPattern, Dimensions, RawImage, Rect, white_level_from_bit_depth};
use crate::error::{FormatError, ParseError, RawError, RawResult};

/// Metadata extracted from a Sony ARW file.
#[derive(Debug, Clone)]
pub struct ArwMetadata {
    /// Camera manufacturer (always "SONY" for ARW)
    pub make: String,
    /// Camera model (e.g., "ILCE-6700")
    pub model: String,
    /// Full sensor dimensions
    pub sensor_size: Dimensions,
    /// Active/crop area
    pub active_area: Rect,
    /// Bits per sample (typically 12 or 14)
    pub bit_depth: u8,
    /// CFA pattern (Bayer arrangement)
    pub cfa_pattern: CfaPattern,
    /// Compression type used
    pub compression: u16,
    /// Black level values (per CFA channel)
    pub black_levels: [u16; 4],
    /// White/saturation level
    pub white_level: u16,
    /// Offset to raw data (for strip-based storage)
    pub raw_data_offset: u64,
    /// Size of raw data in bytes
    pub raw_data_size: u64,
    /// Tile width (0 if strip-based)
    pub tile_width: u32,
    /// Tile height (0 if strip-based)
    pub tile_height: u32,
    /// Tile offsets (empty if strip-based)
    pub tile_offsets: Vec<u64>,
    /// Tile byte counts (empty if strip-based)
    pub tile_byte_counts: Vec<u64>,
    /// As Shot Neutral (converted from WB multipliers if found)
    pub as_shot_neutral: Option<[f64; 3]>,
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
}

/// Parsed Sony ARW file.
pub struct ArwFile<R> {
    /// The whole file, read into memory (IFD offsets are absolute).
    data: Vec<u8>,
    /// The container byte order.
    order: ByteOrder,
    /// Classic TIFF or BigTIFF.
    variant: Variant,
    /// The main IFD chain (with SubIFDs/Exif/GPS pointer groups resolved)
    ifds: Vec<Ifd>,
    /// The SubIFD containing the raw image (ifd_index, sub_ifd_index within
    /// the SubIFDs pointer group)
    raw_ifd_index: Option<(usize, usize)>,
    /// Extracted metadata
    metadata: Option<ArwMetadata>,
    /// The reader type this file was parsed from.
    _reader: PhantomData<R>,
}

impl<R: Read + Seek> ArwFile<R> {
    /// Parse a Sony ARW file.
    pub fn parse(reader: R) -> RawResult<Self> {
        let data = ifd::read_all(reader)?;
        let tree = ifd::parse_tree(&data, "ARW: TIFF structure")?;

        // Find the raw SubIFD
        let raw_ifd_index = Self::find_raw_ifd(&tree.ifds);

        let mut arw = ArwFile {
            data,
            order: tree.order,
            variant: tree.variant,
            ifds: tree.ifds,
            raw_ifd_index,
            metadata: None,
            _reader: PhantomData,
        };

        // Extract metadata
        arw.extract_metadata()?;

        Ok(arw)
    }

    /// Find the SubIFD containing the raw image data.
    ///
    /// The raw SubIFD typically has:
    /// - PhotometricInterpretation = CFA (32803)
    /// - Largest dimensions
    /// - BitsPerSample = 12 or 14
    fn find_raw_ifd(ifds: &[Ifd]) -> Option<(usize, usize)> {
        let mut best_match: Option<(usize, usize, u64)> = None;

        for (ifd_idx, ifd) in ifds.iter().enumerate() {
            for (sub_idx, sub_ifd) in ifd::sub_ifd_group(ifd, tags::SUB_IFDS).iter().enumerate() {
                // Check for CFA photometric interpretation (CFA is 32803)
                if sub_ifd.get_u32(tags::PHOTOMETRIC_INTERPRETATION) == Some(32803) {
                    // Get dimensions
                    let width = ifd::first_u32(sub_ifd, tags::IMAGE_WIDTH).unwrap_or(0);
                    let height = ifd::first_u32(sub_ifd, tags::IMAGE_LENGTH).unwrap_or(0);

                    let pixel_count = width as u64 * height as u64;

                    // Keep the largest one
                    if best_match.is_none() || best_match.as_ref().unwrap().2 < pixel_count {
                        best_match = Some((ifd_idx, sub_idx, pixel_count));
                    }
                }
            }
        }

        best_match.map(|(ifd_idx, sub_idx, _)| (ifd_idx, sub_idx))
    }

    /// Get the raw SubIFD.
    fn raw_ifd(&self) -> Option<&Ifd> {
        self.raw_ifd_index.map(|(ifd_idx, sub_idx)| {
            &ifd::sub_ifd_group(&self.ifds[ifd_idx], tags::SUB_IFDS)[sub_idx]
        })
    }

    /// Get the main IFD (IFD0).
    fn ifd0(&self) -> Option<&Ifd> {
        self.ifds.first()
    }

    /// Get the extracted metadata.
    pub fn metadata(&self) -> Option<&ArwMetadata> {
        self.metadata.as_ref()
    }

    /// Extract metadata from the parsed IFDs.
    fn extract_metadata(&mut self) -> RawResult<()> {
        let ifd0 = self.ifd0().ok_or_else(|| {
            RawError::Parse(ParseError::InvalidIfd {
                offset: 0,
                reason: "No IFD0 found".to_string(),
            })
        })?;

        // Extract Make
        let make = ifd::ascii_tag(ifd0, tags::MAKE).unwrap_or_default();

        // Validate this is a Sony file
        if !make.to_uppercase().contains("SONY") {
            return Err(RawError::Unsupported(format!(
                "Not a Sony file (Make: {})",
                make
            )));
        }

        // Extract Model
        let model = ifd::ascii_tag(ifd0, tags::MODEL).unwrap_or_default();

        // Get the raw SubIFD
        let raw_ifd = self
            .raw_ifd()
            .ok_or_else(|| RawError::Unsupported("Could not find raw SubIFD".to_string()))?;

        // Extract dimensions from raw SubIFD
        let width = ifd::first_u32(raw_ifd, tags::IMAGE_WIDTH)
            .ok_or(RawError::Parse(ParseError::MissingTag(tags::IMAGE_WIDTH)))?;

        let height = ifd::first_u32(raw_ifd, tags::IMAGE_LENGTH)
            .ok_or(RawError::Parse(ParseError::MissingTag(tags::IMAGE_LENGTH)))?;

        let sensor_size = Dimensions { width, height };

        // Extract bit depth
        let bit_depth = raw_ifd
            .get(tags::BITS_PER_SAMPLE)
            .and_then(Value::as_u32)
            .unwrap_or(14) as u8; // Default for modern Sony cameras

        // Extract compression
        let compression = ifd::first_u32(raw_ifd, tags::COMPRESSION).unwrap_or(1) as u16;

        // Extract CFA pattern (Sony typically uses RGGB)
        let cfa_pattern = raw_ifd
            .get(tags::CFA_PATTERN)
            .and_then(Value::as_bytes)
            .filter(|bytes| bytes.len() >= 4)
            .and_then(|bytes| CfaPattern::from_array([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .unwrap_or(CfaPattern::Rggb);

        // Extract crop/active area
        let active_area = if let (Some(origin_vec), Some(size_vec)) = (
            raw_ifd.get_u32_vec(tags::DEFAULT_CROP_ORIGIN),
            raw_ifd.get_u32_vec(tags::DEFAULT_CROP_SIZE),
        ) {
            if origin_vec.len() >= 2 && size_vec.len() >= 2 {
                Rect::from_coords(origin_vec[0], origin_vec[1], size_vec[0], size_vec[1])
            } else {
                Rect::from_coords(0, 0, width, height)
            }
        } else {
            Rect::from_coords(0, 0, width, height)
        };

        // Extract black levels
        let black_levels = if let Some(entry) = raw_ifd.get(tags::BLACK_LEVEL) {
            if let Some(vec) = entry.as_u32_vec() {
                if vec.len() >= 4 {
                    [vec[0] as u16, vec[1] as u16, vec[2] as u16, vec[3] as u16]
                } else if vec.len() == 1 {
                    let v = vec[0] as u16;
                    [v, v, v, v]
                } else {
                    [0, 0, 0, 0]
                }
            } else {
                [0, 0, 0, 0]
            }
        } else {
            [512, 512, 512, 512] // Sony default
        };

        // Extract white level
        let white_level = raw_ifd
            .get(tags::WHITE_LEVEL)
            .and_then(Value::as_u32)
            .unwrap_or(white_level_from_bit_depth(bit_depth) as u32)
            as u16;

        // Get raw data location from strips (Sony typically uses a single strip)
        let (raw_data_offset, raw_data_size) = if let (Some(offsets), Some(counts)) = (
            raw_ifd.get(tags::STRIP_OFFSETS),
            raw_ifd.get(tags::STRIP_BYTE_COUNTS),
        ) {
            (offsets.as_u64().unwrap_or(0), counts.as_u64().unwrap_or(0))
        } else {
            (0, 0)
        };

        // Get tile dimensions
        let tile_width = ifd::first_u32(raw_ifd, tags::TILE_WIDTH).unwrap_or(0);
        let tile_height = ifd::first_u32(raw_ifd, tags::TILE_LENGTH).unwrap_or(0);

        // Get tile offsets and byte counts
        let tile_offsets: Vec<u64> = raw_ifd
            .get_u32_vec(tags::TILE_OFFSETS)
            .map(|v| v.into_iter().map(u64::from).collect())
            .unwrap_or_default();

        let tile_byte_counts: Vec<u64> = raw_ifd
            .get_u32_vec(tags::TILE_BYTE_COUNTS)
            .map(|v| v.into_iter().map(u64::from).collect())
            .unwrap_or_default();

        // Extract White Balance from raw SubIFD first (most reliable for newer Sony cameras).
        // Sony ILCE series cameras (e.g., ILCE-6700) store WB_RGGBLevels (0x7313) as
        // SSHORT values directly in the raw SubIFD, not in the MakerNote.
        let mut as_shot_neutral: Option<[f64; 3]> = None;

        if let Some(value) = raw_ifd.get(tags::SONY_WB_RGGB_LEVELS) {
            let vals_opt: Option<(f64, f64, f64, f64)> = match value {
                Value::SShort(vals) if vals.len() >= 4 => Some((
                    vals[0] as f64,
                    vals[1] as f64,
                    vals[2] as f64,
                    vals[3] as f64,
                )),
                Value::Short(vals) if vals.len() >= 4 => Some((
                    vals[0] as f64,
                    vals[1] as f64,
                    vals[2] as f64,
                    vals[3] as f64,
                )),
                _ => None,
            };
            if let Some((r, g1, g2, b)) = vals_opt {
                let g = (g1 + g2) / 2.0;
                if r > 0.0 && g > 0.0 && b > 0.0 {
                    as_shot_neutral = Some([g / r, 1.0, g / b]);
                    tracing::debug!(
                        "Found WB_RGGBLevels in raw SubIFD (0x7313): RGGB=[{},{},{},{}] -> AsShotNeutral={:?}",
                        r,
                        g1,
                        g2,
                        b,
                        as_shot_neutral
                    );
                }
            }
        }

        // Fallback: extract White Balance from MakerNote if not found in raw SubIFD.
        // MakerNote is usually in the EXIF IFD
        let makernote_value = if let Some(exif_ifd) = ifd::exif_ifd(ifd0) {
            tracing::debug!("Found Exif IFD");
            exif_ifd.get(tags::MAKER_NOTE)
        } else {
            // Sometimes directly in IFD0?
            ifd0.get(tags::MAKER_NOTE)
        };

        if as_shot_neutral.is_none()
            && let Some(Value::Undefined(bytes)) = makernote_value
        {
            tracing::debug!("Found Sony MakerNote ({} bytes).", bytes.len());

            use std::io::Cursor;

            let offset = if bytes.starts_with(b"SONY DSC ") || bytes.starts_with(b"SONY CAM ") {
                12
            } else {
                0
            };

            if bytes.len() > offset {
                let mut cursor = Cursor::new(&bytes[offset..]);
                let mut buf2 = [0u8; 2];
                if cursor.read_exact(&mut buf2).is_ok() {
                    let count = u16::from_le_bytes(buf2);
                    tracing::debug!("Scanning {} MakerNote entries...", count);

                    for _ in 0..count {
                        // 12 bytes per entry
                        let mut entry_buf = [0u8; 12];
                        if cursor.read_exact(&mut entry_buf).is_ok() {
                            let tag_id = u16::from_le_bytes([entry_buf[0], entry_buf[1]]);
                            let _type_code = u16::from_le_bytes([entry_buf[2], entry_buf[3]]);
                            let _count = u32::from_le_bytes([
                                entry_buf[4],
                                entry_buf[5],
                                entry_buf[6],
                                entry_buf[7],
                            ]);
                            let value_offset = u32::from_le_bytes([
                                entry_buf[8],
                                entry_buf[9],
                                entry_buf[10],
                                entry_buf[11],
                            ]);

                            tracing::debug!(
                                "MakerNote Tag: 0x{:04X} Offset: {}",
                                tag_id,
                                value_offset
                            );

                            // Extract data from Tag 0x7313 (WB_RGGBLevels)
                            // This is the standard Sony WB tag. Values are Multipliers/Gains.
                            if tag_id == 0x7313 {
                                let mut v1 = 0;
                                let mut v2 = 0;
                                let mut v3 = 0;
                                let mut v4 = 0;

                                // Try Absolute Offset (into the whole file)
                                let mut found_abs = false;
                                let abs = value_offset as usize;
                                if let Some(v) =
                                    abs.checked_add(8).and_then(|end| self.data.get(abs..end))
                                {
                                    v1 = u16::from_le_bytes([v[0], v[1]]);
                                    v2 = u16::from_le_bytes([v[2], v[3]]);
                                    v3 = u16::from_le_bytes([v[4], v[5]]);
                                    v4 = u16::from_le_bytes([v[6], v[7]]);
                                    if v1 > 0 || v2 > 0 {
                                        found_abs = true;
                                    }
                                }

                                if !found_abs {
                                    let off = value_offset as usize;
                                    if off + 8 <= bytes.len() {
                                        let v = &bytes[off..off + 8];
                                        v1 = u16::from_le_bytes([v[0], v[1]]);
                                        v2 = u16::from_le_bytes([v[2], v[3]]);
                                        v3 = u16::from_le_bytes([v[4], v[5]]);
                                        v4 = u16::from_le_bytes([v[6], v[7]]);
                                    }
                                }

                                if v1 > 0 && v2 > 0 && v3 > 0 && v4 > 0 {
                                    // RGGB Layout for this tag: R, G, G, B
                                    // These are GAINS (Multipliers).
                                    // To convert to AsShotNeutral (Scene Levels), we invert them.
                                    // AsShotNeutral = [1/Gain_R, 1/Gain_G, 1/Gain_B]

                                    let r_gain = v1 as f64;
                                    let g_gain = (v2 as f64 + v3 as f64) / 2.0;
                                    let b_gain = v4 as f64;

                                    // Normalize so Green Neutral = 1.0.
                                    // Neutral_R = (1/R_Gain) / (1/G_Gain) = G_Gain / R_Gain.

                                    as_shot_neutral = Some([g_gain / r_gain, 1.0, g_gain / b_gain]);

                                    tracing::debug!(
                                        "Found WB_RGGBLevels (0x7313): Gains=[{}, {}, {}] -> AsShotNeutral={:?}",
                                        r_gain,
                                        g_gain,
                                        b_gain,
                                        as_shot_neutral
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Check for Sony SR2 SubIFD (Tag 0x02BC) which often contains the WB data
        if as_shot_neutral.is_none()
            && let Some(val) = ifd0.get(tags::SONY_SR2_PRIVATE)
        {
            tracing::debug!("Found Tag 0x02BC (SR2 Offset Candidate): {:?}", val);

            let offset_opt = match val {
                Value::Long(v) | Value::Ifd(v) if !v.is_empty() => Some(v[0]),
                Value::Short(v) if !v.is_empty() => Some(v[0] as u32),
                _ => None,
            };

            if let Some(offset) = offset_opt {
                tracing::debug!("Found Sony SR2 SubIFD at offset {}", offset);
                // The SR2 block is a directory at an absolute file offset; read
                // it lazily so a garbled (encrypted) entry only fails its own
                // value fetch, not the whole directory.
                let mut sr2_reader =
                    IfdReader::with_layout(&self.data[..], self.order, self.variant);
                match sr2_reader.read_ifd(u64::from(offset)) {
                    Ok(sr2_ifd) => {
                        // Check for WB_RGGBLevels (0x7313)
                        if let Some(wb_entry) = sr2_ifd.entry(tags::SONY_WB_RGGB_LEVELS) {
                            match sr2_reader.value(wb_entry) {
                                Ok(Value::Short(vals)) if vals.len() >= 4 => {
                                    let v1 = vals[0];
                                    let v2 = vals[1];
                                    let v3 = vals[2];
                                    let v4 = vals[3];

                                    tracing::debug!("Found WB Levels: {:?}", vals);

                                    if v1 > 0 && v2 > 0 && v3 > 0 && v4 > 0 {
                                        let r_gain = v1 as f64;
                                        let g_gain = (v2 as f64 + v3 as f64) / 2.0;
                                        let b_gain = v4 as f64;

                                        as_shot_neutral =
                                            Some([g_gain / r_gain, 1.0, g_gain / b_gain]);
                                        tracing::debug!(
                                            "Found WB_RGGBLevels in SR2: Gains=[{}, {}, {}] -> AsShotNeutral={:?}",
                                            r_gain,
                                            g_gain,
                                            b_gain,
                                            as_shot_neutral
                                        );
                                    }
                                }
                                Ok(other_val) => tracing::warn!(
                                    "WB_RGGBLevels (0x7313) has unexpected value: {:?}",
                                    other_val
                                ),
                                Err(e) => {
                                    tracing::warn!("Failed to read WB_RGGBLevels (0x7313): {}", e)
                                }
                            }
                        } else {
                            tracing::debug!(
                                "SR2 SubIFD parsed but WB_RGGBLevels (0x7313) not found"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse SR2 SubIFD at {}: {}", offset, e)
                    }
                }
            } else {
                tracing::warn!(
                    "Tag 0x02BC found but value not a valid offset (found {:?})",
                    val
                );
            }
        }

        // Extract EXIF/GPS/DateTime/orientation from IFD0
        let exif = ifd::extract_exif(ifd0);
        let datetime = ifd::extract_datetime(ifd0);
        let gps = ifd::extract_gps(ifd0);
        let (lens_make, lens_model) = ifd::extract_lens_info(ifd0);
        let orientation = ifd::extract_orientation(ifd0);

        self.metadata = Some(ArwMetadata {
            make,
            model,
            sensor_size,
            active_area,
            bit_depth,
            cfa_pattern,
            compression,
            black_levels,
            white_level,
            raw_data_offset,
            raw_data_size,
            tile_width,
            tile_height,
            tile_offsets,
            tile_byte_counts,
            as_shot_neutral,
            exif,
            datetime,
            gps,
            lens_make,
            lens_model,
            orientation,
        });

        Ok(())
    }

    /// Validate that this is a Sony ARW file.
    pub fn validate(&self) -> RawResult<()> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| RawError::Unsupported("Metadata not extracted".to_string()))?;

        // Check for Sony
        if !metadata.make.to_uppercase().contains("SONY") {
            return Err(RawError::Unsupported(format!(
                "Not a Sony camera: {}",
                metadata.make
            )));
        }

        // Check for valid dimensions
        if metadata.sensor_size.width == 0 || metadata.sensor_size.height == 0 {
            return Err(RawError::Parse(ParseError::InvalidDimensions {
                width: metadata.sensor_size.width,
                height: metadata.sensor_size.height,
            }));
        }

        // Verify Model name (per Sony ARW specs)
        let model = metadata.model.to_uppercase();
        if !model.contains("ILCE")
            && !model.contains("ILCA")
            && !model.contains("NEX")
            && !model.contains("SLT")
            && !model.contains("DSC")
            && !model.contains("ALPHA")
        {
            tracing::warn!(
                "Model '{}' does not contain standard Sony naming (ILCE, ILCA, etc.)",
                metadata.model
            );
        }

        // Check for raw data
        if metadata.raw_data_offset == 0 || metadata.raw_data_size == 0 {
            return Err(RawError::Unsupported("No raw data found".to_string()));
        }

        Ok(())
    }

    /// Read raw data as a byte vector.
    ///
    /// This retrieves the compressed raw data stream from the file.
    pub fn read_raw_data(&mut self) -> RawResult<Vec<u8>> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| RawError::Unsupported("Metadata not extracted".to_string()))?;

        let offset = metadata.raw_data_offset;
        let size = metadata.raw_data_size as usize;

        Ok(ifd::read_range(&self.data, offset, size)?.to_vec())
    }

    /// Extract the embedded JPEG thumbnail from IFD 0, if present.
    pub fn thumbnail(&mut self) -> RawResult<Option<Vec<u8>>> {
        let ifd0 = match self.ifd0() {
            Some(ifd) => ifd,
            None => return Ok(None),
        };
        ifd::jpeg_thumbnail(&self.data, ifd0)
    }

    /// Decode the raw image data into a RawImage.
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        let metadata = self.metadata.as_ref().unwrap().clone();

        // 7 = JPEG (Lossless for Sony ARW)
        if metadata.compression == 7 {
            use crate::codecs::ljpeg::LjpegDecoder;

            let width = metadata.sensor_size.width as usize;
            let height = metadata.sensor_size.height as usize;
            let mut output = vec![0u16; width * height];

            // Check if tiled or strip-based
            if !metadata.tile_offsets.is_empty()
                && metadata.tile_width > 0
                && metadata.tile_height > 0
            {
                // Tiled storage - decode each tile
                let tile_w = metadata.tile_width as usize;
                let tile_h = metadata.tile_height as usize;
                let tiles_x = width.div_ceil(tile_w);
                let _tiles_y = height.div_ceil(tile_h);

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
                    let tile_data = ifd::read_range(&self.data, tile_offset, tile_size as usize)?;

                    // Decode this tile
                    let mut decoder = LjpegDecoder::new();
                    // Set tile dimensions - Sony LJPEG header says 256x256 but with 4 components
                    // that produces a 512x512 tile
                    decoder.set_dimensions(tile_w as u32, tile_h as u32);

                    let tile_pixels = match decoder.decode(tile_data) {
                        Ok(pixels) => pixels,
                        Err(e) => {
                            tracing::warn!("Failed to decode tile {}: {}", tile_idx, e);
                            // Fill with zeros and continue
                            vec![0u16; tile_w * tile_h]
                        }
                    };

                    // Copy tile pixels to output at correct position
                    // The tile may contain 4-component super-pixels
                    // LJPEG frame claims 256x256 per tile, but with 4 components that's actually 512x512
                    let actual_tile_w = tile_w.min(width - tile_x);
                    let actual_tile_h = tile_h.min(height - tile_y);

                    for ty in 0..actual_tile_h {
                        for tx in 0..actual_tile_w {
                            let src_idx = ty * tile_w + tx;
                            if src_idx < tile_pixels.len() {
                                let dst_x = tile_x + tx;
                                let dst_y = tile_y + ty;
                                if dst_x < width && dst_y < height {
                                    output[dst_y * width + dst_x] = tile_pixels[src_idx];
                                }
                            }
                        }
                    }
                }
            } else {
                // Strip-based - single LJPEG stream
                let data = self.read_raw_data()?;
                let mut decoder = LjpegDecoder::new();
                decoder.set_dimensions(metadata.sensor_size.width, metadata.sensor_size.height);
                output = decoder.decode(&data)?;
            }

            let expected_pixels = metadata
                .sensor_size
                .num_pixels()
                .expect("sensor pixel count overflows usize");
            if output.len() != expected_pixels {
                return Err(RawError::Format(FormatError::Decompression(format!(
                    "Decoded {} pixels, expected {}",
                    output.len(),
                    expected_pixels
                ))));
            }

            return Ok(RawImage::builder(
                metadata.sensor_size,
                metadata.active_area,
                metadata.bit_depth,
                metadata.cfa_pattern,
            )
            .black_levels(metadata.black_levels)
            .white_level(metadata.white_level)
            .data(output)
            .build());
        }

        // Handle specific compression types
        match metadata.compression {
            8 => Err(RawError::Unsupported(
                "Sony Compressed (Type 8) not yet supported. Only Uncompressed/LJPEG (Type 7) is supported.".to_string()
            )),
            _ => Err(RawError::Unsupported(format!(
                "Compression type {} not yet supported (only JPEG type 7 is supported)",
                metadata.compression
            ))),
        }
    }
}

impl<R: Read + Seek> crate::core::ExtractMetadata for ArwFile<R> {
    fn extract_metadata(&self) -> crate::core::ImageMetadata {
        use crate::core::metadata::*;

        let m = self.metadata.as_ref();
        let as_shot_neutral = m.and_then(|x| x.as_shot_neutral);

        ImageMetadata {
            camera: CameraInfo {
                make: m.map(|x| x.make.clone()).unwrap_or_default(),
                model: m.map(|x| x.model.clone()).unwrap_or_default(),
                unique_camera_model: None, // ARW doesn't have this DNG tag
                lens_make: m.and_then(|x| x.lens_make.clone()),
                lens_model: m.and_then(|x| x.lens_model.clone()),
                lens_info: None,
                serial_number: None,
            },
            exif: m.map(|x| x.exif.clone()).unwrap_or_default(),
            datetime: m.map(|x| x.datetime.clone()).unwrap_or_default(),
            gps: m.map(|x| x.gps.clone()).unwrap_or_default(),
            dng_color: DngColorInfo {
                as_shot_neutral,
                ..DngColorInfo::default()
            },
            dng_calibration: DngCalibrationInfo::default(),
            dng_profile: DngProfileInfo::default(),
            image: ImageInfo {
                orientation: m.and_then(|x| x.orientation),
                bit_depth: m.map(|x| x.bit_depth).unwrap_or(14),
                black_levels: m
                    .map(|x| x.black_levels.iter().map(|&v| v as u32).collect())
                    .unwrap_or_default(),
                white_level: m.map(|x| x.white_level as u32),
                default_crop_origin: m.map(|x| (x.active_area.origin.x, x.active_area.origin.y)),
                default_crop_size: m.map(|x| (x.active_area.size.width, x.active_area.size.height)),
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
    use std::io::BufReader;
    use std::path::PathBuf;

    fn test_data_path(filename: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(filename)
    }

    fn skip_if_no_test_data(filename: &str) -> bool {
        !test_data_path(filename).exists()
    }

    #[test]
    fn test_arw_parse() {
        if skip_if_no_test_data("_JIC7790.ARW") {
            eprintln!("Skipping test: test data not found");
            return;
        }

        let file = File::open(test_data_path("_JIC7790.ARW")).unwrap();
        let reader = BufReader::new(file);
        let arw = ArwFile::parse(reader).unwrap();

        let metadata = arw.metadata().unwrap();

        // Validate Sony camera
        assert!(metadata.make.to_uppercase().contains("SONY"));
        assert!(metadata.model.contains("ILCE"));

        // Validate dimensions from ground truth
        assert_eq!(metadata.sensor_size.width, 6656);
        assert_eq!(metadata.sensor_size.height, 4608);

        // Validate bit depth
        assert_eq!(metadata.bit_depth, 14);

        // Validate CFA pattern (Sony uses RGGB)
        assert_eq!(metadata.cfa_pattern, CfaPattern::Rggb);
    }

    #[test]
    fn test_arw_validate() {
        if skip_if_no_test_data("_JIC7790.ARW") {
            return;
        }

        let file = File::open(test_data_path("_JIC7790.ARW")).unwrap();
        let reader = BufReader::new(file);
        let arw = ArwFile::parse(reader).unwrap();

        assert!(arw.validate().is_ok());
    }

    #[test]
    fn test_arw_read_raw_data() {
        if skip_if_no_test_data("_JIC7790.ARW") {
            return;
        }

        let file = File::open(test_data_path("_JIC7790.ARW")).unwrap();
        let reader = BufReader::new(file);
        let mut arw = ArwFile::parse(reader).unwrap();

        let raw_data = arw.read_raw_data().unwrap();

        // Verify we got some data
        assert!(!raw_data.is_empty());

        // Verify the size matches metadata
        let metadata = arw.metadata().unwrap();
        assert_eq!(raw_data.len(), metadata.raw_data_size as usize);
    }
}
