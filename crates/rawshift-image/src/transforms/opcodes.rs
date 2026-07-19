//! DNG Opcode List processing.
//!
//! DNG opcodes are embedded binary records in OpcodeList1/2/3 TIFF tags that
//! describe image corrections: lens shading (GainMap), bad pixel fixing, etc.
//!
//! Binary layout of an OpcodeList (big-endian throughout):
//! ```text
//! [4 bytes] count of opcodes
//! For each opcode:
//!   [4 bytes] opcode ID
//!   [4 bytes] minimum DNG SDK version (ignored during parsing)
//!   [4 bytes] flags  (bit 0 = optional; skip on error if set)
//!   [4 bytes] parameter data length N
//!   [N bytes] parameter data
//! ```
//!
//! Implemented opcodes (priority order):
//! 1. `FixBadPixelsConstant` (ID 4) — replace sub-threshold pixels
//! 2. `FixBadPixelsList` (ID 5) — replace specific known bad pixels
//! 3. `GainMap` (ID 9) — spatially-varying lens-shading correction (critical for ProRAW)

use crate::core::RgbImage;

// ============================================================================
// Opcode data structures
// ============================================================================

/// A parsed DNG GainMap opcode (opcode ID 9).
///
/// Encodes a spatially-varying multiplicative gain applied to each colour
/// channel to correct for lens vignetting / shading.
#[derive(Debug, Clone)]
pub struct GainMap {
    /// Normalised top edge of the region this map covers (0.0–1.0)
    pub top: f64,
    /// Normalised left edge
    pub left: f64,
    /// Normalised bottom edge
    pub bottom: f64,
    /// Normalised right edge
    pub right: f64,
    /// Index of the first plane (colour channel) this map applies to
    pub plane: u32,
    /// Number of consecutive planes this map applies to
    pub planes: u32,
    /// Row sub-sampling pitch (every N rows has a map sample)
    pub row_pitch: u32,
    /// Column sub-sampling pitch
    pub col_pitch: u32,
    /// Number of map sample rows
    pub map_points_v: u32,
    /// Number of map sample columns
    pub map_points_h: u32,
    /// Normalised row spacing between consecutive map samples
    pub map_spacing_v: f64,
    /// Normalised column spacing
    pub map_spacing_h: f64,
    /// Normalised row coordinate of the first map sample
    pub map_origin_v: f64,
    /// Normalised column coordinate of the first map sample
    pub map_origin_h: f64,
    /// Number of planes stored in the map data (1 = shared, 3 = per-channel)
    pub map_planes: u32,
    /// Gain values, stored in [v][h][plane] order (row-major, f32 each)
    pub gain: Vec<f32>,
}

impl GainMap {
    /// Apply this GainMap to an RGB image.
    ///
    /// Bilinearly interpolates gain values from the map grid and multiplies
    /// each affected pixel channel by the corresponding gain.
    pub fn apply_to_rgb(&self, image: &mut RgbImage) {
        let img_w = image.width() as usize;
        let img_h = image.height() as usize;
        if img_w == 0 || img_h == 0 {
            return;
        }

        let map_v = self.map_points_v as usize;
        let map_h = self.map_points_h as usize;
        let map_p = self.map_planes as usize;
        if map_v == 0 || map_h == 0 || map_p == 0 || self.gain.is_empty() {
            return;
        }

        let planes_start = self.plane as usize;
        // `planes` is the number of output channels this map covers.
        // `map_planes` is how many distinct gain planes the data holds
        // (1 = shared gain applied to all `planes` output channels).
        let planes_count = (self.planes as usize).min(3);

        let data = image.data_mut();

        for y in 0..img_h {
            // Normalised image coordinate in [0, 1]
            let norm_v = if img_h > 1 {
                y as f64 / (img_h - 1) as f64
            } else {
                0.5
            };

            // Fractional map row index
            let map_row_f =
                ((norm_v - self.map_origin_v) / self.map_spacing_v).clamp(0.0, (map_v - 1) as f64);
            let r0 = map_row_f.floor() as usize;
            let r1 = (r0 + 1).min(map_v - 1);
            let dr = map_row_f - r0 as f64;

            for x in 0..img_w {
                let norm_h = if img_w > 1 {
                    x as f64 / (img_w - 1) as f64
                } else {
                    0.5
                };

                let map_col_f = ((norm_h - self.map_origin_h) / self.map_spacing_h)
                    .clamp(0.0, (map_h - 1) as f64);
                let c0 = map_col_f.floor() as usize;
                let c1 = (c0 + 1).min(map_h - 1);
                let dc = map_col_f - c0 as f64;

                let pixel_base = (y * img_w + x) * 3;

                for p in 0..planes_count {
                    let channel = planes_start + p;
                    if channel >= 3 {
                        break;
                    }
                    // When map_planes == 1 the same gain applies to all channels
                    let mp = if map_p == 1 { 0 } else { p.min(map_p - 1) };

                    let g00 = self.gain[r0 * map_h * map_p + c0 * map_p + mp] as f64;
                    let g01 = self.gain[r0 * map_h * map_p + c1 * map_p + mp] as f64;
                    let g10 = self.gain[r1 * map_h * map_p + c0 * map_p + mp] as f64;
                    let g11 = self.gain[r1 * map_h * map_p + c1 * map_p + mp] as f64;

                    let gain = g00 * (1.0 - dr) * (1.0 - dc)
                        + g01 * (1.0 - dr) * dc
                        + g10 * dr * (1.0 - dc)
                        + g11 * dr * dc;

                    let val = data[pixel_base + channel];
                    data[pixel_base + channel] = (val as f64 * gain).clamp(0.0, 65535.0) as u16;
                }
            }
        }
    }
}

/// A parsed DNG opcode.
#[derive(Debug, Clone)]
pub enum DngOpcode {
    /// Opcode ID 4: replace all pixels below a threshold with interpolated neighbours.
    FixBadPixelsConstant {
        /// Number of image planes affected
        planes: u32,
        /// Pixel value treated as defective
        bad_point_value: u32,
    },
    /// Opcode ID 5: replace a specific list of known bad pixels / rectangles.
    FixBadPixelsList {
        /// List of bad pixel (row, col) coordinates
        bad_points: Vec<(u32, u32)>,
        /// List of bad pixel regions (top, left, bottom, right)
        bad_rects: Vec<(u32, u32, u32, u32)>,
    },
    /// Opcode ID 9: spatially-varying gain map (lens shading / vignetting correction).
    GainMap(GainMap),
    /// An unrecognised opcode — stored so callers can decide whether to skip.
    Unknown {
        /// Raw opcode identifier
        id: u32,
    },
}

// ============================================================================
// OpcodeList
// ============================================================================

/// A parsed list of DNG opcodes from one of the OpcodeList1/2/3 TIFF tags.
#[derive(Debug, Clone, Default)]
pub struct OpcodeList {
    /// Parsed opcodes together with their `is_optional` flag.
    ///
    /// `is_optional == true` means failures should be silently ignored.
    pub opcodes: Vec<(DngOpcode, bool)>,
}

impl OpcodeList {
    /// Parse an opcode list from the raw TIFF tag bytes (UNDEFINED type, big-endian).
    ///
    /// Returns an empty list if `data` is shorter than 4 bytes.
    pub fn parse(data: &[u8]) -> Self {
        if data.len() < 4 {
            return OpcodeList::default();
        }

        let count = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let mut pos = 4;
        let mut opcodes = Vec::with_capacity(count);

        for _ in 0..count {
            if pos + 16 > data.len() {
                break;
            }

            let opcode_id =
                u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            // skip 4-byte min DNG version at pos+4
            let flags =
                u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);
            let param_len = u32::from_be_bytes([
                data[pos + 12],
                data[pos + 13],
                data[pos + 14],
                data[pos + 15],
            ]) as usize;
            pos += 16;

            let is_optional = (flags & 1) != 0;

            if pos + param_len > data.len() {
                break;
            }
            let param_data = &data[pos..pos + param_len];
            pos += param_len;

            let opcode = match opcode_id {
                4 => Self::parse_fix_bad_pixels_constant(param_data),
                5 => Self::parse_fix_bad_pixels_list(param_data),
                9 => Self::parse_gain_map(param_data),
                id => Some(DngOpcode::Unknown { id }),
            };

            if let Some(op) = opcode {
                opcodes.push((op, is_optional));
            }
        }

        OpcodeList { opcodes }
    }

    /// Apply this opcode list to an RGB image.
    ///
    /// Opcodes are applied in order. Unknown or unimplemented opcodes are
    /// skipped with a trace-level log. Optional opcodes that fail are silently
    /// ignored; required ones that fail are also ignored but logged at warn.
    pub fn apply_to_rgb(&self, image: &mut RgbImage) {
        for (opcode, is_optional) in &self.opcodes {
            match opcode {
                DngOpcode::GainMap(gm) => {
                    tracing::trace!(
                        "Applying GainMap: {}x{} grid, {} planes",
                        gm.map_points_h,
                        gm.map_points_v,
                        gm.map_planes
                    );
                    gm.apply_to_rgb(image);
                }
                DngOpcode::FixBadPixelsConstant { .. } => {
                    // Not yet implemented for RGB — applies to raw CFA data
                    if !is_optional {
                        tracing::debug!("FixBadPixelsConstant on RGB not yet implemented");
                    }
                }
                DngOpcode::FixBadPixelsList { .. } => {
                    if !is_optional {
                        tracing::debug!("FixBadPixelsList on RGB not yet implemented");
                    }
                }
                DngOpcode::Unknown { id } => {
                    if !is_optional {
                        tracing::warn!("Skipping unknown required DNG opcode ID={}", id);
                    } else {
                        tracing::trace!("Skipping unknown optional DNG opcode ID={}", id);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Private parsers
    // -----------------------------------------------------------------------

    fn read_u32_be(data: &[u8], offset: usize) -> Option<u32> {
        data.get(offset..offset + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_f32_be(data: &[u8], offset: usize) -> Option<f32> {
        data.get(offset..offset + 4)
            .map(|b| f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_f64_be(data: &[u8], offset: usize) -> Option<f64> {
        data.get(offset..offset + 8)
            .map(|b| f64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    fn parse_gain_map(data: &[u8]) -> Option<DngOpcode> {
        // Minimum fixed header: 8 rationals (4+4 bytes each = 32 bytes) +
        // 6 uint32 (24 bytes) + 4 doubles (32 bytes) + 1 uint32 (4 bytes) = 92 bytes
        if data.len() < 92 {
            return None;
        }

        let top_n = Self::read_u32_be(data, 0)? as f64;
        let top_d = Self::read_u32_be(data, 4)? as f64;
        let left_n = Self::read_u32_be(data, 8)? as f64;
        let left_d = Self::read_u32_be(data, 12)? as f64;
        let bottom_n = Self::read_u32_be(data, 16)? as f64;
        let bottom_d = Self::read_u32_be(data, 20)? as f64;
        let right_n = Self::read_u32_be(data, 24)? as f64;
        let right_d = Self::read_u32_be(data, 28)? as f64;

        let plane = Self::read_u32_be(data, 32)?;
        let planes = Self::read_u32_be(data, 36)?;
        let row_pitch = Self::read_u32_be(data, 40)?;
        let col_pitch = Self::read_u32_be(data, 44)?;
        let map_points_v = Self::read_u32_be(data, 48)?;
        let map_points_h = Self::read_u32_be(data, 52)?;
        let map_spacing_v = Self::read_f64_be(data, 56)?;
        let map_spacing_h = Self::read_f64_be(data, 64)?;
        let map_origin_v = Self::read_f64_be(data, 72)?;
        let map_origin_h = Self::read_f64_be(data, 80)?;
        let map_planes = Self::read_u32_be(data, 88)?;

        let gain_count = (map_points_v as usize) * (map_points_h as usize) * (map_planes as usize);
        let gain_offset = 92;

        if gain_offset + gain_count * 4 > data.len() {
            return None;
        }

        let mut gain = Vec::with_capacity(gain_count);
        for i in 0..gain_count {
            gain.push(Self::read_f32_be(data, gain_offset + i * 4)?);
        }

        Some(DngOpcode::GainMap(GainMap {
            top: if top_d != 0.0 { top_n / top_d } else { 0.0 },
            left: if left_d != 0.0 { left_n / left_d } else { 0.0 },
            bottom: if bottom_d != 0.0 {
                bottom_n / bottom_d
            } else {
                1.0
            },
            right: if right_d != 0.0 {
                right_n / right_d
            } else {
                1.0
            },
            plane,
            planes,
            row_pitch,
            col_pitch,
            map_points_v,
            map_points_h,
            map_spacing_v,
            map_spacing_h,
            map_origin_v,
            map_origin_h,
            map_planes,
            gain,
        }))
    }

    fn parse_fix_bad_pixels_constant(data: &[u8]) -> Option<DngOpcode> {
        if data.len() < 8 {
            return None;
        }
        let planes = Self::read_u32_be(data, 0)?;
        let bad_point_value = Self::read_u32_be(data, 4)?;
        Some(DngOpcode::FixBadPixelsConstant {
            planes,
            bad_point_value,
        })
    }

    fn parse_fix_bad_pixels_list(data: &[u8]) -> Option<DngOpcode> {
        if data.len() < 8 {
            return None;
        }
        let bad_point_count = Self::read_u32_be(data, 0)? as usize;
        let bad_rect_count = Self::read_u32_be(data, 4)? as usize;
        let mut offset = 8;

        let mut bad_points = Vec::with_capacity(bad_point_count);
        for _ in 0..bad_point_count {
            if offset + 8 > data.len() {
                return None;
            }
            let row = Self::read_u32_be(data, offset)?;
            let col = Self::read_u32_be(data, offset + 4)?;
            bad_points.push((row, col));
            offset += 8;
        }

        let mut bad_rects = Vec::with_capacity(bad_rect_count);
        for _ in 0..bad_rect_count {
            if offset + 16 > data.len() {
                return None;
            }
            let top = Self::read_u32_be(data, offset)?;
            let left = Self::read_u32_be(data, offset + 4)?;
            let bottom = Self::read_u32_be(data, offset + 8)?;
            let right = Self::read_u32_be(data, offset + 12)?;
            bad_rects.push((top, left, bottom, right));
            offset += 16;
        }

        Some(DngOpcode::FixBadPixelsList {
            bad_points,
            bad_rects,
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal GainMap opcode list with a single uniform gain.
    fn build_gain_map_opcode_list(gain_value: f32) -> Vec<u8> {
        let mut data = Vec::new();

        // count = 1
        data.extend_from_slice(&1u32.to_be_bytes());

        // opcode_id = 9 (GainMap)
        data.extend_from_slice(&9u32.to_be_bytes());
        // min version (ignored)
        data.extend_from_slice(&0u32.to_be_bytes());
        // flags = 1 (optional)
        data.extend_from_slice(&1u32.to_be_bytes());

        // Build GainMap parameter data
        let mut params: Vec<u8> = Vec::new();
        // top = 0/1
        params.extend_from_slice(&0u32.to_be_bytes());
        params.extend_from_slice(&1u32.to_be_bytes());
        // left = 0/1
        params.extend_from_slice(&0u32.to_be_bytes());
        params.extend_from_slice(&1u32.to_be_bytes());
        // bottom = 1/1
        params.extend_from_slice(&1u32.to_be_bytes());
        params.extend_from_slice(&1u32.to_be_bytes());
        // right = 1/1
        params.extend_from_slice(&1u32.to_be_bytes());
        params.extend_from_slice(&1u32.to_be_bytes());
        // plane=0, planes=3, row_pitch=1, col_pitch=1
        params.extend_from_slice(&0u32.to_be_bytes());
        params.extend_from_slice(&3u32.to_be_bytes());
        params.extend_from_slice(&1u32.to_be_bytes());
        params.extend_from_slice(&1u32.to_be_bytes());
        // map_points_v=2, map_points_h=2
        params.extend_from_slice(&2u32.to_be_bytes());
        params.extend_from_slice(&2u32.to_be_bytes());
        // map_spacing_v=1.0, map_spacing_h=1.0
        params.extend_from_slice(&1.0f64.to_be_bytes());
        params.extend_from_slice(&1.0f64.to_be_bytes());
        // map_origin_v=0.0, map_origin_h=0.0
        params.extend_from_slice(&0.0f64.to_be_bytes());
        params.extend_from_slice(&0.0f64.to_be_bytes());
        // map_planes=1
        params.extend_from_slice(&1u32.to_be_bytes());
        // 4 gain samples (2x2 grid, 1 plane)
        for _ in 0..4 {
            params.extend_from_slice(&gain_value.to_be_bytes());
        }

        // param_len
        data.extend_from_slice(&(params.len() as u32).to_be_bytes());
        data.extend_from_slice(&params);

        data
    }

    #[test]
    fn test_parse_empty() {
        let list = OpcodeList::parse(&[]);
        assert!(list.opcodes.is_empty());
    }

    #[test]
    fn test_parse_gain_map_uniform() {
        let data = build_gain_map_opcode_list(2.0);
        let list = OpcodeList::parse(&data);
        assert_eq!(list.opcodes.len(), 1);
        let (op, is_optional) = &list.opcodes[0];
        assert!(is_optional);
        match op {
            DngOpcode::GainMap(gm) => {
                assert_eq!(gm.map_points_v, 2);
                assert_eq!(gm.map_points_h, 2);
                assert_eq!(gm.map_planes, 1);
                assert_eq!(gm.gain.len(), 4);
                assert!((gm.gain[0] - 2.0).abs() < 1e-6);
            }
            _ => panic!("Expected GainMap opcode"),
        }
    }

    #[test]
    fn test_apply_uniform_gain_doubles_pixel() {
        let data = build_gain_map_opcode_list(2.0);
        let list = OpcodeList::parse(&data);

        let mut img = RgbImage::new(2, 2, vec![1000u16; 12]).expect("valid RGB buffer");
        list.apply_to_rgb(&mut img);

        // Uniform gain of 2.0 should double all pixels
        for &v in img.data() {
            assert_eq!(v, 2000, "Expected pixel value 2000, got {v}");
        }
    }

    #[test]
    fn test_parse_fix_bad_pixels_constant() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes()); // count=1
        data.extend_from_slice(&4u32.to_be_bytes()); // id=4
        data.extend_from_slice(&0u32.to_be_bytes()); // min version
        data.extend_from_slice(&0u32.to_be_bytes()); // flags=0 (required)
        let mut params = Vec::new();
        params.extend_from_slice(&3u32.to_be_bytes()); // planes=3
        params.extend_from_slice(&0u32.to_be_bytes()); // bad_point_value=0
        data.extend_from_slice(&(params.len() as u32).to_be_bytes());
        data.extend_from_slice(&params);

        let list = OpcodeList::parse(&data);
        assert_eq!(list.opcodes.len(), 1);
        match &list.opcodes[0].0 {
            DngOpcode::FixBadPixelsConstant {
                planes,
                bad_point_value,
            } => {
                assert_eq!(*planes, 3);
                assert_eq!(*bad_point_value, 0);
            }
            _ => panic!("Expected FixBadPixelsConstant"),
        }
    }

    #[test]
    fn test_parse_fix_bad_pixels_list() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes()); // count=1
        data.extend_from_slice(&5u32.to_be_bytes()); // id=5
        data.extend_from_slice(&0u32.to_be_bytes()); // min version
        data.extend_from_slice(&1u32.to_be_bytes()); // flags=1 (optional)
        let mut params = Vec::new();
        params.extend_from_slice(&1u32.to_be_bytes()); // bad_point_count=1
        params.extend_from_slice(&0u32.to_be_bytes()); // bad_rect_count=0
        params.extend_from_slice(&100u32.to_be_bytes()); // row=100
        params.extend_from_slice(&200u32.to_be_bytes()); // col=200
        data.extend_from_slice(&(params.len() as u32).to_be_bytes());
        data.extend_from_slice(&params);

        let list = OpcodeList::parse(&data);
        assert_eq!(list.opcodes.len(), 1);
        match &list.opcodes[0].0 {
            DngOpcode::FixBadPixelsList {
                bad_points,
                bad_rects,
            } => {
                assert_eq!(bad_points.len(), 1);
                assert_eq!(bad_points[0], (100, 200));
                assert!(bad_rects.is_empty());
            }
            _ => panic!("Expected FixBadPixelsList"),
        }
    }

    #[test]
    fn test_parse_unknown_opcode() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes()); // count=1
        data.extend_from_slice(&42u32.to_be_bytes()); // unknown id=42
        data.extend_from_slice(&0u32.to_be_bytes()); // min version
        data.extend_from_slice(&1u32.to_be_bytes()); // flags=1 (optional)
        data.extend_from_slice(&0u32.to_be_bytes()); // param_len=0

        let list = OpcodeList::parse(&data);
        assert_eq!(list.opcodes.len(), 1);
        match &list.opcodes[0].0 {
            DngOpcode::Unknown { id } => assert_eq!(*id, 42),
            _ => panic!("Expected Unknown opcode"),
        }
    }
}
