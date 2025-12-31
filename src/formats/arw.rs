//! Sony ARW format support.
//!
//! This module provides parsing for Sony Alpha Raw (ARW) files,
//! which are based on the TIFF container format with Sony-specific extensions.

use std::io::{Read, Seek};

use crate::core::image::{CfaPattern, RawImage, Rect, Size};
use crate::error::{RawError, RawResult};
use crate::tiff::{Ifd, TiffParser, TiffTag, TiffValue};

/// Metadata extracted from a Sony ARW file.
#[derive(Debug, Clone)]
pub struct ArwMetadata {
    /// Camera manufacturer (always "SONY" for ARW)
    pub make: String,
    /// Camera model (e.g., "ILCE-6700")
    pub model: String,
    /// Full sensor dimensions
    pub sensor_size: Size,
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
}

/// Parsed Sony ARW file.
pub struct ArwFile<R> {
    parser: TiffParser<R>,
    /// The main IFD chain
    ifds: Vec<Ifd>,
    /// The SubIFD containing the raw image (ifd_index, sub_ifd_index)
    raw_ifd_index: Option<(usize, usize)>,
    /// Extracted metadata
    metadata: Option<ArwMetadata>,
}

impl<R: Read + Seek> ArwFile<R> {
    /// Parse a Sony ARW file.
    pub fn parse(reader: R) -> RawResult<Self> {
        let mut parser = TiffParser::new(reader)?;

        // Walk the IFD chain
        let ifds = parser.walk_ifd_chain()?;

        // Find the raw SubIFD
        let raw_ifd_index = Self::find_raw_ifd(&ifds);

        let mut arw = ArwFile {
            parser,
            ifds,
            raw_ifd_index,
            metadata: None,
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
            for (sub_idx, sub_ifd) in ifd.sub_ifds.iter().enumerate() {
                // Check for CFA photometric interpretation
                if let Some(entry) = sub_ifd.get(TiffTag::PhotometricInterpretation) {
                    // CFA is 32803
                    if entry.value_offset == 32803 {
                        // Get dimensions
                        let width = sub_ifd
                            .get(TiffTag::ImageWidth)
                            .map(|e| e.value_offset as u32)
                            .unwrap_or(0);
                        let height = sub_ifd
                            .get(TiffTag::ImageLength)
                            .map(|e| e.value_offset as u32)
                            .unwrap_or(0);

                        let pixel_count = width as u64 * height as u64;

                        // Keep the largest one
                        if best_match.is_none() || best_match.as_ref().unwrap().2 < pixel_count {
                            best_match = Some((ifd_idx, sub_idx, pixel_count));
                        }
                    }
                }
            }
        }

        best_match.map(|(ifd_idx, sub_idx, _)| (ifd_idx, sub_idx))
    }

    /// Get the raw SubIFD.
    pub fn raw_ifd(&self) -> Option<&Ifd> {
        self.raw_ifd_index
            .map(|(ifd_idx, sub_idx)| &self.ifds[ifd_idx].sub_ifds[sub_idx])
    }

    /// Get the main IFD (IFD0).
    pub fn ifd0(&self) -> Option<&Ifd> {
        self.ifds.first()
    }

    /// Get the extracted metadata.
    pub fn metadata(&self) -> Option<&ArwMetadata> {
        self.metadata.as_ref()
    }

    /// Extract metadata from the parsed IFDs.
    fn extract_metadata(&mut self) -> RawResult<()> {
        // Clone the IFDs we need to avoid borrow issues
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

        // Validate this is a Sony file
        if !make.to_uppercase().contains("SONY") {
            return Err(RawError::UnsupportedFormat(format!(
                "Not a Sony file (Make: {})",
                make
            )));
        }

        // Extract Model
        let model = if let Some(entry) = ifd0.get(TiffTag::Model) {
            let value = self.parser.read_value(entry)?;
            value.as_str().unwrap_or("").trim().to_string()
        } else {
            String::new()
        };

        // Get the raw SubIFD
        let raw_ifd = self
            .raw_ifd()
            .cloned()
            .ok_or_else(|| RawError::UnsupportedFormat("Could not find raw SubIFD".to_string()))?;

        // Extract dimensions from raw SubIFD
        let width = raw_ifd
            .get(TiffTag::ImageWidth)
            .map(|e| e.value_offset as u32)
            .ok_or(RawError::TagNotFound(TiffTag::ImageWidth))?;

        let height = raw_ifd
            .get(TiffTag::ImageLength)
            .map(|e| e.value_offset as u32)
            .ok_or(RawError::TagNotFound(TiffTag::ImageLength))?;

        let sensor_size = Size::new(width, height);

        // Extract bit depth
        let bit_depth = if let Some(entry) = raw_ifd.get(TiffTag::BitsPerSample) {
            let value = self.parser.read_value(entry)?;
            value.as_u32().unwrap_or(14) as u8
        } else {
            14 // Default for modern Sony cameras
        };

        // Extract compression
        let compression = raw_ifd
            .get(TiffTag::Compression)
            .map(|e| e.value_offset as u16)
            .unwrap_or(1);

        // Extract CFA pattern
        let cfa_pattern = if let Some(entry) = raw_ifd.get(TiffTag::CFAPattern) {
            let value = self.parser.read_value(entry)?;
            if let TiffValue::Bytes(bytes) = value {
                if bytes.len() >= 4 {
                    let arr = [bytes[0], bytes[1], bytes[2], bytes[3]];
                    CfaPattern::from_array(arr).unwrap_or(CfaPattern::Rggb)
                } else {
                    CfaPattern::Rggb
                }
            } else {
                CfaPattern::Rggb
            }
        } else {
            // Sony typically uses RGGB
            CfaPattern::Rggb
        };

        // Extract crop/active area
        let active_area = if let (Some(origin_entry), Some(size_entry)) = (
            raw_ifd.get(TiffTag::DefaultCropOrigin),
            raw_ifd.get(TiffTag::DefaultCropSize),
        ) {
            let origin = self.parser.read_value(origin_entry)?;
            let size = self.parser.read_value(size_entry)?;

            if let (Some(origin_vec), Some(size_vec)) = (origin.as_u32_vec(), size.as_u32_vec()) {
                if origin_vec.len() >= 2 && size_vec.len() >= 2 {
                    Rect::from_coords(origin_vec[0], origin_vec[1], size_vec[0], size_vec[1])
                } else {
                    Rect::from_coords(0, 0, width, height)
                }
            } else {
                Rect::from_coords(0, 0, width, height)
            }
        } else {
            Rect::from_coords(0, 0, width, height)
        };

        // Extract black levels
        let black_levels = if let Some(entry) = raw_ifd.get(TiffTag::BlackLevel) {
            let value = self.parser.read_value(entry)?;
            if let Some(vec) = value.as_u32_vec() {
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
        let white_level = if let Some(entry) = raw_ifd.get(TiffTag::WhiteLevel) {
            let value = self.parser.read_value(entry)?;
            value.as_u32().unwrap_or((1 << bit_depth) - 1) as u16
        } else {
            (1u16 << bit_depth) - 1
        };

        // Get raw data location from strips
        let (raw_data_offset, raw_data_size) = if let (Some(offset_entry), Some(count_entry)) = (
            raw_ifd.get(TiffTag::StripOffsets),
            raw_ifd.get(TiffTag::StripByteCounts),
        ) {
            let offsets = self.parser.read_value(offset_entry)?;
            let counts = self.parser.read_value(count_entry)?;

            // For Sony, typically single strip
            let offset = offsets.as_u64().unwrap_or(0);
            let size = counts.as_u64().unwrap_or(0);
            (offset, size)
        } else {
            (0, 0)
        };

        // Get tile dimensions
        let tile_width = if let Some(entry) = raw_ifd.get(TiffTag::TileWidth) {
            entry.value_offset as u32
        } else {
            0
        };

        let tile_height = if let Some(entry) = raw_ifd.get(TiffTag::TileLength) {
            entry.value_offset as u32
        } else {
            0
        };

        // Get tile offsets and byte counts
        let tile_offsets = if let Some(entry) = raw_ifd.get(TiffTag::TileOffsets) {
            let value = self.parser.read_value(entry)?;
            value
                .as_u32_vec()
                .map(|v| v.into_iter().map(|x| x as u64).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let tile_byte_counts = if let Some(entry) = raw_ifd.get(TiffTag::TileByteCounts) {
            let value = self.parser.read_value(entry)?;
            value
                .as_u32_vec()
                .map(|v| v.into_iter().map(|x| x as u64).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

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
        });

        Ok(())
    }

    /// Validate that this is a Sony ARW file.
    pub fn validate(&self) -> RawResult<()> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| RawError::UnsupportedFormat("Metadata not extracted".to_string()))?;

        // Check for Sony
        if !metadata.make.to_uppercase().contains("SONY") {
            return Err(RawError::UnsupportedFormat(format!(
                "Not a Sony camera: {}",
                metadata.make
            )));
        }

        // Check for valid dimensions
        if metadata.sensor_size.width == 0 || metadata.sensor_size.height == 0 {
            return Err(RawError::InvalidDimensions {
                width: metadata.sensor_size.width,
                height: metadata.sensor_size.height,
            });
        }

        // Check for raw data
        if metadata.raw_data_offset == 0 || metadata.raw_data_size == 0 {
            return Err(RawError::UnsupportedFormat("No raw data found".to_string()));
        }

        Ok(())
    }

    pub fn read_raw_data(&mut self) -> RawResult<Vec<u8>> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| RawError::UnsupportedFormat("Metadata not extracted".to_string()))?;

        let offset = metadata.raw_data_offset;
        let size = metadata.raw_data_size as usize;

        // Seek to the raw data
        self.parser.seek_to(offset)?;

        // Read the data
        let data = self.parser.read_bytes(size)?;

        Ok(data)
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
                    self.parser.seek_to(tile_offset)?;
                    let tile_data = self.parser.read_bytes(tile_size as usize)?;

                    // Decode this tile
                    let mut decoder = LjpegDecoder::new();
                    // Set tile dimensions - Sony LJPEG header says 256x256 but with 4 components
                    // that produces a 512x512 tile
                    decoder.set_dimensions(tile_w as u32, tile_h as u32);

                    let tile_pixels = match decoder.decode(&tile_data) {
                        Ok(pixels) => pixels,
                        Err(e) => {
                            log::warn!("Failed to decode tile {}: {}", tile_idx, e);
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

            let expected_pixels = metadata.sensor_size.pixel_count() as usize;
            if output.len() != expected_pixels {
                return Err(RawError::DecompressionError(format!(
                    "Decoded {} pixels, expected {}",
                    output.len(),
                    expected_pixels
                )));
            }

            return Ok(RawImage {
                size: metadata.sensor_size,
                active_area: metadata.active_area,
                bit_depth: metadata.bit_depth,
                cfa_pattern: metadata.cfa_pattern,
                black_levels: metadata.black_levels,
                white_level: metadata.white_level,
                data: output,
            });
        }

        Err(RawError::UnsupportedFormat(format!(
            "Compression type {} not yet supported (only JPEG type 7 is supported)",
            metadata.compression
        )))
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
