//! Lossless JPEG (LJPEG) Decoder
//!
//! This module implements a decoder for Lossless JPEG (ITU-T T.81 Annex H),
//! specifically optimized for Sony ARW raw files which use SOF3 (lossless, Huffman).
//!
//! Lossless JPEG uses spatial prediction rather than DCT, making it fundamentally
//! different from standard JPEG. The predictor modes are:
//! - Mode 1: Ra (left neighbor)
//! - Mode 2: Rb (above neighbor)
//! - Mode 3: Rc (upper-left neighbor)
//! - Mode 4: Ra + Rb - Rc
//! - Mode 5: Ra + (Rb - Rc) / 2
//! - Mode 6: Rb + (Ra - Rc) / 2
//! - Mode 7: (Ra + Rb) / 2

use crate::error::{FormatError, RawError, RawResult};

/// JPEG Markers
#[allow(dead_code)]
mod markers {
    pub const SOI: u16 = 0xFFD8; // Start of Image
    pub const EOI: u16 = 0xFFD9; // End of Image
    pub const SOF3: u16 = 0xFFC3; // Lossless, Huffman
    pub const DHT: u16 = 0xFFC4; // Define Huffman Table
    pub const SOS: u16 = 0xFFDA; // Start of Scan
    pub const DRI: u16 = 0xFFDD; // Define Restart Interval
    pub const RST0: u16 = 0xFFD0; // Restart marker 0
    pub const RST7: u16 = 0xFFD7; // Restart marker 7
    pub const APP0: u16 = 0xFFE0; // Application marker 0
    pub const APP15: u16 = 0xFFEF; // Application marker 15
    pub const COM: u16 = 0xFFFE; // Comment
}

/// Component information from SOF marker
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct Component {
    pub id: u8,
    pub h_samp: u8, // Horizontal sampling factor
    pub v_samp: u8, // Vertical sampling factor
    pub quant_table: u8,
    pub dc_table: u8, // DC Huffman table index (set from SOS)
}

/// Frame information from SOF3 marker
#[derive(Debug, Clone, Default)]
pub struct FrameInfo {
    pub precision: u8, // Bits per sample (typically 12-16 for raw)
    pub height: u16,   // May be dummy value, real from TIFF
    pub width: u16,    // May be dummy value, real from TIFF
    pub components: Vec<Component>,
}

/// Huffman table
#[derive(Debug, Clone)]
pub struct HuffmanTable {
    /// Number of codes of each length (1-16 bits)
    pub bits: [u8; 17],
    /// Symbol values
    pub huffval: Vec<u8>,
    /// Lookup table for fast decoding: (value, bits_consumed)
    /// Index is the next 16 bits of input
    pub lookup: Vec<(i32, u8)>,
    /// Maximum code value for each bit length
    pub maxcode: [i32; 18],
    /// Minimum code value for each bit length
    pub mincode: [i32; 17],
    /// Index into huffval for each bit length
    pub valptr: [i32; 17],
}

impl Default for HuffmanTable {
    fn default() -> Self {
        Self {
            bits: [0; 17],
            huffval: Vec::new(),
            lookup: Vec::new(),
            maxcode: [-1; 18],
            mincode: [0; 17],
            valptr: [0; 17],
        }
    }
}

impl HuffmanTable {
    /// Build the decoding tables from the bits and huffval arrays
    pub fn build_tables(&mut self) {
        // Build maxcode, mincode, valptr tables per JPEG spec
        let mut code = 0i32;
        let mut si = 1usize;
        let mut j = 0usize;

        while si <= 16 {
            if self.bits[si] == 0 {
                self.maxcode[si] = -1;
            } else {
                self.valptr[si] = j as i32;
                self.mincode[si] = code;
                code += self.bits[si] as i32;
                self.maxcode[si] = code - 1;
                j += self.bits[si] as usize;
            }
            code <<= 1;
            si += 1;
        }
        self.maxcode[17] = 0x7FFFFFFF; // Sentinel

        // Build fast lookup table (8-bit for speed)
        self.lookup = vec![(0, 0); 256];
        let mut code = 0u32;
        let mut idx = 0usize;

        for bits in 1..=8 {
            for _ in 0..self.bits[bits] {
                if idx < self.huffval.len() {
                    let symbol = self.huffval[idx] as i32;
                    // Fill all lookup entries that match this code
                    let fill_bits = 8 - bits;
                    let fill_count = 1 << fill_bits;
                    let base = (code << fill_bits) as usize;
                    for f in 0..fill_count {
                        if base + f < 256 {
                            self.lookup[base + f] = (symbol, bits as u8);
                        }
                    }
                    idx += 1;
                }
                code += 1;
            }
            code <<= 1;
        }
    }
}

// Re-export BitPump for external use
pub use super::bit_pump::BitPump;

/// Lossless JPEG Decoder.
///
/// Decodes lossless JPEG data (ITU-T T.81 Annex H / SOF3),
/// commonly used in camera raw files like Sony ARW.
///
/// # Construction
///
/// Use the builder pattern for cleaner construction:
/// ```ignore
/// let pixels = LjpegDecoder::builder()
///     .dimensions(6000, 4000)
///     .build()
///     .decode(&data)?;
/// ```
///
/// Or the traditional methods for backward compatibility:
/// ```ignore
/// let mut decoder = LjpegDecoder::new();
/// decoder.set_dimensions(6000, 4000);
/// let pixels = decoder.decode(&data)?;
/// ```
pub struct LjpegDecoder {
    frame: FrameInfo,
    huffman_dc: [HuffmanTable; 4],
    restart_interval: u16,
    predictor: u8,
    point_transform: u8,
    /// Real dimensions (from TIFF metadata, overrides JPEG header)
    real_width: Option<u32>,
    real_height: Option<u32>,
}

/// Builder for [`LjpegDecoder`].
///
/// Provides a fluent API for constructing an LJPEG decoder.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct LjpegDecoderBuilder {
    width: Option<u32>,
    height: Option<u32>,
}

#[allow(dead_code)]
impl LjpegDecoderBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the output dimensions.
    ///
    /// These override any dimensions specified in the JPEG header.
    /// Required when decoding Sony ARW files that have placeholder dimensions.
    #[must_use]
    pub fn dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Build the decoder.
    #[must_use]
    pub fn build(self) -> LjpegDecoder {
        let mut decoder = LjpegDecoder::new();
        if let (Some(w), Some(h)) = (self.width, self.height) {
            decoder.set_dimensions(w, h);
        }
        decoder
    }

    /// Build the decoder and immediately decode the provided data.
    ///
    /// Convenience method equivalent to `builder.build().decode(data)`.
    pub fn decode(self, data: &[u8]) -> RawResult<Vec<u16>> {
        self.build().decode(data)
    }
}

impl LjpegDecoder {
    /// Create a new decoder with default settings.
    pub fn new() -> Self {
        Self {
            frame: FrameInfo::default(),
            huffman_dc: [
                HuffmanTable::default(),
                HuffmanTable::default(),
                HuffmanTable::default(),
                HuffmanTable::default(),
            ],
            restart_interval: 0,
            predictor: 1,
            point_transform: 0,
            real_width: None,
            real_height: None,
        }
    }

    /// Create a builder for configuring a new decoder.
    #[allow(dead_code)]
    pub fn builder() -> LjpegDecoderBuilder {
        LjpegDecoderBuilder::new()
    }

    /// Set real dimensions from TIFF metadata (Sony uses dummy values in JPEG header).
    pub fn set_dimensions(&mut self, width: u32, height: u32) {
        self.real_width = Some(width);
        self.real_height = Some(height);
    }

    /// Get the frame information (parsed from JPEG header).
    #[allow(dead_code)]
    pub fn frame_info(&self) -> &FrameInfo {
        &self.frame
    }

    /// Parse JPEG markers and decode the image
    pub fn decode(&mut self, data: &[u8]) -> RawResult<Vec<u16>> {
        let mut pos;
        let mut scan_data_start = 0;
        let mut scan_data_end = data.len();

        // Check SOI
        if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
            return Err(RawError::Format(FormatError::Decompression(
                "Missing JPEG SOI marker".into(),
            )));
        }
        pos = 2;

        // Parse markers
        while pos + 2 <= data.len() {
            if data[pos] != 0xFF {
                pos += 1;
                continue;
            }

            let marker = u16::from_be_bytes([data[pos], data[pos + 1]]);
            pos += 2;

            match marker {
                markers::SOF3 => {
                    self.parse_sof3(&data[pos..])?;
                    let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                    pos += len;
                }
                markers::DHT => {
                    let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                    self.parse_dht(&data[pos..pos + len])?;
                    pos += len;
                }
                markers::DRI => {
                    let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                    if len >= 4 {
                        self.restart_interval = u16::from_be_bytes([data[pos + 2], data[pos + 3]]);
                    }
                    pos += len;
                }
                markers::SOS => {
                    let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                    self.parse_sos(&data[pos..pos + len])?;
                    pos += len;
                    scan_data_start = pos;
                    // Find end of scan data (EOI or next marker)
                    while pos + 1 < data.len() {
                        if data[pos] == 0xFF
                            && data[pos + 1] != 0x00
                            && !(data[pos + 1] >= 0xD0 && data[pos + 1] <= 0xD7)
                        {
                            scan_data_end = pos;
                            break;
                        }
                        pos += 1;
                    }
                    break; // Start decoding after SOS
                }
                markers::EOI => {
                    break;
                }
                m if (markers::APP0..=markers::APP15).contains(&m) => {
                    // Skip APP markers
                    let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                    pos += len;
                }
                markers::COM => {
                    // Skip comment
                    let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                    pos += len;
                }
                _ => {
                    // Skip unknown markers with length
                    if pos + 2 <= data.len() {
                        let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                        pos += len;
                    }
                }
            }
        }

        // Decode the scan data
        let scan_data = &data[scan_data_start..scan_data_end];
        self.decode_scan(scan_data)
    }

    /// Parse SOF3 (Start of Frame, Lossless)
    fn parse_sof3(&mut self, data: &[u8]) -> RawResult<()> {
        if data.len() < 8 {
            return Err(RawError::Format(FormatError::Decompression(
                "SOF3 too short".into(),
            )));
        }

        let _len = u16::from_be_bytes([data[0], data[1]]);
        self.frame.precision = data[2];
        self.frame.height = u16::from_be_bytes([data[3], data[4]]);
        self.frame.width = u16::from_be_bytes([data[5], data[6]]);
        let num_components = data[7] as usize;

        if data.len() < 8 + num_components * 3 {
            return Err(RawError::Format(FormatError::Decompression(
                "SOF3 component data too short".into(),
            )));
        }

        self.frame.components.clear();
        for i in 0..num_components {
            let offset = 8 + i * 3;
            let comp = Component {
                id: data[offset],
                h_samp: (data[offset + 1] >> 4) & 0x0F,
                v_samp: data[offset + 1] & 0x0F,
                quant_table: data[offset + 2],
                dc_table: 0, // Set later from SOS
            };
            self.frame.components.push(comp);
        }

        Ok(())
    }

    /// Parse DHT (Define Huffman Table)
    fn parse_dht(&mut self, data: &[u8]) -> RawResult<()> {
        let len = u16::from_be_bytes([data[0], data[1]]) as usize;
        let mut pos = 2;

        while pos < len {
            let info = data[pos];
            let table_class = (info >> 4) & 0x0F; // 0 = DC, 1 = AC (not used in lossless)
            let table_id = (info & 0x0F) as usize;
            pos += 1;

            if table_id >= 4 {
                return Err(RawError::Format(FormatError::Decompression(format!(
                    "Invalid Huffman table ID: {}",
                    table_id
                ))));
            }

            // Read BITS (number of codes of each length 1-16)
            let mut table = HuffmanTable::default();
            let mut total_codes = 0usize;
            for i in 1..=16 {
                if pos >= len {
                    return Err(RawError::Format(FormatError::Decompression(
                        "DHT truncated".into(),
                    )));
                }
                table.bits[i] = data[pos];
                total_codes += data[pos] as usize;
                pos += 1;
            }

            // Read HUFFVAL (symbol values)
            if pos + total_codes > len {
                return Err(RawError::Format(FormatError::Decompression(
                    "DHT HUFFVAL truncated".into(),
                )));
            }
            table.huffval = data[pos..pos + total_codes].to_vec();
            pos += total_codes;

            // Build decoding tables
            table.build_tables();

            // Store in appropriate slot (only DC tables used in lossless JPEG)
            if table_class == 0 {
                self.huffman_dc[table_id] = table;
            }
        }

        Ok(())
    }

    /// Parse SOS (Start of Scan)
    fn parse_sos(&mut self, data: &[u8]) -> RawResult<()> {
        if data.len() < 3 {
            return Err(RawError::Format(FormatError::Decompression(
                "SOS too short".into(),
            )));
        }

        let _len = u16::from_be_bytes([data[0], data[1]]);
        let num_components = data[2] as usize;

        if data.len() < 3 + num_components * 2 + 3 {
            return Err(RawError::Format(FormatError::Decompression(
                "SOS data too short".into(),
            )));
        }

        // Parse component selectors
        for i in 0..num_components {
            let offset = 3 + i * 2;
            let comp_id = data[offset];
            let tables = data[offset + 1];
            let dc_table = (tables >> 4) & 0x0F;

            // Find and update the component
            for comp in &mut self.frame.components {
                if comp.id == comp_id {
                    comp.dc_table = dc_table;
                    break;
                }
            }
        }

        // Spectral selection (Ss = predictor, Se = 0, Ah/Al = point transform)
        let ss_offset = 3 + num_components * 2;
        self.predictor = data[ss_offset];
        // Se should be 0 for lossless
        self.point_transform = data[ss_offset + 2] & 0x0F;

        Ok(())
    }

    /// Decode a Huffman-coded value
    #[inline]
    fn decode_huffman(&self, pump: &mut BitPump, table: &HuffmanTable) -> i32 {
        // Fast path: use 8-bit lookup
        let peek8 = (pump.peek(8) & 0xFF) as usize;

        if peek8 < table.lookup.len() {
            let (symbol, bits) = table.lookup[peek8];
            if bits > 0 {
                pump.consume(bits as u32);
                return symbol;
            }
        }

        // Slow path: full decode
        let mut code = 0i32;
        for bits in 1..=16 {
            code = (code << 1) | (pump.get_bits(1) as i32);
            if code <= table.maxcode[bits] {
                let idx = (table.valptr[bits] + code - table.mincode[bits]) as usize;
                if idx < table.huffval.len() {
                    return table.huffval[idx] as i32;
                }
            }
        }

        0 // Error case
    }

    /// Extend a value to its full signed representation
    #[inline]
    fn extend(v: i32, t: i32) -> i32 {
        if t == 0 {
            return 0;
        }
        let vt = 1 << (t - 1);
        if v < vt { v + (-1 << t) + 1 } else { v }
    }

    /// Get the predictor value
    #[inline]
    fn predict(&self, ra: i32, rb: i32, rc: i32) -> i32 {
        match self.predictor {
            0 => 0,                     // No prediction (first row/col)
            1 => ra,                    // Left
            2 => rb,                    // Above
            3 => rc,                    // Upper-left
            4 => ra + rb - rc,          // Ra + Rb - Rc
            5 => ra + ((rb - rc) >> 1), // Ra + (Rb - Rc) / 2
            6 => rb + ((ra - rc) >> 1), // Rb + (Ra - Rc) / 2
            7 => (ra + rb) >> 1,        // (Ra + Rb) / 2
            _ => ra,                    // Default to left
        }
    }

    /// Decode the scan data
    fn decode_scan(&mut self, scan_data: &[u8]) -> RawResult<Vec<u16>> {
        let out_width = self.real_width.unwrap_or(self.frame.width as u32) as usize;
        let out_height = self.real_height.unwrap_or(self.frame.height as u32) as usize;
        let num_components = self.frame.components.len();

        if num_components == 0 {
            return Err(RawError::Format(FormatError::Decompression(
                "No components defined".into(),
            )));
        }

        let total_pixels = out_width * out_height;
        let mut output: Vec<u16> = vec![0; total_pixels];

        let mut pump = BitPump::new(scan_data);

        // Initial predictor value (1 << (precision - point_transform - 1))
        let initial = 1i32
            << (self
                .frame
                .precision
                .saturating_sub(self.point_transform)
                .saturating_sub(1));
        let max_val = (1u32 << self.frame.precision) - 1;

        // Sony ARW with 4 components:
        // The JPEG frame dimensions (e.g., 256x256) are placeholders.
        // The actual dimensions come from TIFF metadata (e.g., 6656x4608).
        // With 4 components, we decode super-pixels: each iteration produces 4 values
        // for a 2x2 block in the Bayer pattern.
        if num_components == 4 {
            // Decode dimensions: we need (out_width/2) * (out_height/2) super-pixels
            let super_width = out_width / 2;
            let super_height = out_height / 2;

            // Previous values for each component for prediction (DPCM)
            let mut prev = [initial; 4];
            // Previous row values for column 0 (for "Above" prediction at start of line)
            let mut col0_prev = [initial; 4];

            for sy in 0..super_height {
                for sx in 0..super_width {
                    for c in 0..4 {
                        let table_idx = self.frame.components[c].dc_table as usize;
                        let table = &self.huffman_dc[table_idx.min(3)];

                        // Decode the DPCM difference
                        let category = self.decode_huffman(&mut pump, table);
                        let diff = if category == 0 {
                            0
                        } else {
                            let bits = pump.get_bits(category as u32) as i32;
                            Self::extend(bits, category)
                        };

                        // DPCM prediction:
                        // First pixel of first line: initial
                        // First pixel of other lines: from above (col0_prev)
                        // Other pixels: from left (prev)
                        let predicted = if sx == 0 {
                            if sy == 0 { initial } else { col0_prev[c] }
                        } else {
                            prev[c]
                        };

                        let value = (predicted + diff).clamp(0, max_val as i32) as u16;
                        prev[c] = value as i32;
                        if sx == 0 {
                            col0_prev[c] = value as i32;
                        }

                        // Map component to output position in Bayer pattern
                        // Component 0: top-left (even x, even y)
                        // Component 1: top-right (odd x, even y)
                        // Component 2: bottom-left (even x, odd y)
                        // Component 3: bottom-right (odd x, odd y)
                        let (dx, dy) = match c {
                            0 => (0, 0),
                            1 => (1, 0),
                            2 => (0, 1),
                            3 => (1, 1),
                            _ => (0, 0),
                        };

                        let x = sx * 2 + dx;
                        let y = sy * 2 + dy;

                        if x < out_width && y < out_height {
                            output[y * out_width + x] = value;
                        }
                    }
                }
            }
        } else if num_components == 1 {
            // Single component mode
            let table = &self.huffman_dc[self.frame.components[0].dc_table as usize];

            for y in 0..out_height {
                for x in 0..out_width {
                    // Get prediction value
                    let ra = if x > 0 {
                        output[y * out_width + x - 1] as i32
                    } else {
                        initial
                    };
                    let rb = if y > 0 {
                        output[(y - 1) * out_width + x] as i32
                    } else {
                        initial
                    };
                    let rc = if x > 0 && y > 0 {
                        output[(y - 1) * out_width + x - 1] as i32
                    } else {
                        initial
                    };

                    let predicted = if x == 0 && y == 0 {
                        initial
                    } else if y == 0 {
                        ra
                    } else if x == 0 {
                        rb
                    } else {
                        self.predict(ra, rb, rc)
                    };

                    // Decode difference
                    let category = self.decode_huffman(&mut pump, table);
                    let diff = if category == 0 {
                        0
                    } else {
                        let bits = pump.get_bits(category as u32) as i32;
                        Self::extend(bits, category)
                    };

                    let value = ((predicted + diff) as u32).min(max_val) as u16;
                    output[y * out_width + x] = value;
                }
            }
        } else {
            return Err(RawError::Format(FormatError::Decompression(format!(
                "Unsupported component count: {}",
                num_components
            ))));
        }

        // Apply point transform
        if self.point_transform > 0 {
            for pixel in output.iter_mut() {
                *pixel <<= self.point_transform;
            }
        }

        Ok(output)
    }
}

impl Default for LjpegDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_huffman_extend() {
        // Test the extend function
        assert_eq!(LjpegDecoder::extend(0, 0), 0);
        assert_eq!(LjpegDecoder::extend(0, 1), -1);
        assert_eq!(LjpegDecoder::extend(1, 1), 1);
        assert_eq!(LjpegDecoder::extend(0, 2), -3);
        assert_eq!(LjpegDecoder::extend(1, 2), -2);
        assert_eq!(LjpegDecoder::extend(2, 2), 2);
        assert_eq!(LjpegDecoder::extend(3, 2), 3);
    }

    #[test]
    fn test_huffman_table_build() {
        // Simple Huffman table: 2 symbols with codes 0 and 1
        let mut table = HuffmanTable::default();
        table.bits[1] = 2; // Two 1-bit codes
        table.huffval = vec![0, 1];
        table.build_tables();

        assert_eq!(table.maxcode[1], 1);
        assert_eq!(table.mincode[1], 0);
    }
}
