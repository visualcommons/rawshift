//! Optimized bit reader for entropy-coded data.
//!
//! This module provides a high-performance bit pump for reading bit streams,
//! particularly designed for JPEG entropy-coded data with byte stuffing support.

/// Bit reader for entropy-coded data.
///
/// Reads bits from a byte stream, handling JPEG byte stuffing (0xFF 0x00 → 0xFF)
/// and restart markers automatically.
///
/// # Example
///
/// ```ignore
/// let mut pump = BitPump::new(&data);
/// let value = pump.get_bits(8); // Read 8 bits
/// ```
pub struct BitPump<'a> {
    data: &'a [u8],
    pos: usize,
    bits: u64,      // Bit buffer
    bits_left: u32, // Bits remaining in buffer
}

impl<'a> BitPump<'a> {
    /// Create a new BitPump from a byte slice.
    ///
    /// The pump will immediately fill its internal buffer.
    pub fn new(data: &'a [u8]) -> Self {
        let mut pump = Self {
            data,
            pos: 0,
            bits: 0,
            bits_left: 0,
        };
        pump.fill();
        pump
    }

    /// Fill the bit buffer, handling JPEG byte stuffing (FF 00 -> FF).
    /// Optimized to read chunks of 8 bytes when possible.
    fn fill(&mut self) {
        while self.bits_left <= 56 {
            if self.pos >= self.data.len() {
                break;
            }

            // Optimization: Try to read multiple bytes at once
            // Only use fast path if we have enough data (8 bytes)
            // and we can consume at least 4 bytes to justify the overhead
            let remaining = self.data.len() - self.pos;
            if remaining >= 8 {
                let bytes_to_add = ((64 - self.bits_left) / 8) as usize;

                if bytes_to_add >= 4 {
                    // Safe to read 8 bytes due to remaining >= 8
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(&self.data[self.pos..self.pos + 8]);
                    let chunk = u64::from_be_bytes(buf);

                    // Check for 0xFF in the 8-byte chunk
                    // Logic: !chunk has 0x00 where chunk has 0xFF
                    // has_zero_byte(v) = (v - 0x01..) & !v & 0x80..
                    let v = !chunk;
                    let has_zero = (v.wrapping_sub(0x0101010101010101)) & (!v) & 0x8080808080808080;

                    if has_zero == 0 {
                        // No 0xFF markers, safe this consumes 'bytes_to_add'
                        // Extract top 'bytes_to_add' bytes
                        let shift = (8 - bytes_to_add) * 8;
                        let val = chunk >> shift;

                        let bits_added = bytes_to_add * 8;
                        if bits_added == 64 {
                            self.bits = val;
                        } else {
                            self.bits = (self.bits << bits_added) | val;
                        }
                        self.bits_left += (bytes_to_add * 8) as u32;
                        self.pos += bytes_to_add;
                        continue;
                    }
                }
            }

            let byte = self.data[self.pos] as u64;
            self.pos += 1;

            // Handle byte stuffing: 0xFF followed by 0x00 means literal 0xFF
            if byte == 0xFF && self.pos < self.data.len() {
                let next = self.data[self.pos];
                if next == 0x00 {
                    self.pos += 1; // Skip the stuffed 0x00
                } else if (0xD0..=0xD7).contains(&next) {
                    // Restart marker - skip it
                    self.pos += 1;
                    continue;
                } else if next == 0xD9 {
                    // EOI - stop
                    break;
                }
            }

            self.bits = (self.bits << 8) | byte;
            self.bits_left += 8;
        }
    }

    /// Peek at the next n bits without consuming them.
    ///
    /// # Arguments
    /// * `n` - Number of bits to peek (0-32)
    ///
    /// # Returns
    /// The next `n` bits as a u32.
    #[inline]
    pub fn peek(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // Ensure we have enough bits
        if self.bits_left < n {
            self.fill();
        }
        if self.bits_left < n {
            // Not enough data - return what we have padded with zeros
            return (self.bits as u32) << (n - self.bits_left);
        }
        ((self.bits >> (self.bits_left - n)) & ((1u64 << n) - 1)) as u32
    }

    /// Consume n bits.
    ///
    /// Advances the read position by `n` bits.
    #[inline]
    pub fn consume(&mut self, n: u32) {
        if n <= self.bits_left {
            self.bits_left -= n;
        } else {
            self.bits_left = 0;
        }
        if self.bits_left < 32 {
            self.fill();
        }
    }

    /// Get n bits and consume them.
    ///
    /// Equivalent to `peek(n)` followed by `consume(n)`.
    ///
    /// # Arguments
    /// * `n` - Number of bits to read (0-32)
    ///
    /// # Returns
    /// The next `n` bits as a u32.
    #[inline]
    pub fn get_bits(&mut self, n: u32) -> u32 {
        let val = self.peek(n);
        self.consume(n);
        val
    }

    /// Get the current byte position in the input data.
    #[inline]
    #[allow(dead_code)]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Get the number of bits remaining in the buffer.
    #[inline]
    #[allow(dead_code)]
    pub fn bits_available(&self) -> u32 {
        self.bits_left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_pump_basic() {
        let data = [0xAB, 0xCD];
        let mut pump = BitPump::new(&data);

        // 0xAB = 1010_1011
        assert_eq!(pump.get_bits(4), 0b1010);
        assert_eq!(pump.get_bits(4), 0b1011);

        // 0xCD = 1100_1101
        assert_eq!(pump.get_bits(8), 0xCD);
    }

    #[test]
    fn test_bit_pump_byte_stuffing() {
        // 0xFF 0x00 should be read as just 0xFF
        let data = [0xFF, 0x00, 0xAB];
        let mut pump = BitPump::new(&data);

        assert_eq!(pump.get_bits(8), 0xFF);
        assert_eq!(pump.get_bits(8), 0xAB);
    }

    #[test]
    fn test_bit_pump_peek() {
        let data = [0xAB];
        let mut pump = BitPump::new(&data);

        // Peek should not consume bits
        assert_eq!(pump.peek(4), 0b1010);
        assert_eq!(pump.peek(4), 0b1010);
        assert_eq!(pump.get_bits(4), 0b1010);
        assert_eq!(pump.peek(4), 0b1011);
    }

    #[test]
    fn test_bit_pump_fast_path() {
        // 16 bytes of data, no 0xFF (uses fast path)
        let data: Vec<u8> = (0..16).map(|i| i as u8).collect();
        let mut pump = BitPump::new(&data);

        // consume 64 bits (8 bytes)
        // 00 01 02 03 04 05 06 07
        let val1 = pump.get_bits(32);
        let val2 = pump.get_bits(32);

        assert_eq!(val1, 0x00010203);
        assert_eq!(val2, 0x04050607);

        let val3 = pump.get_bits(32);
        let val4 = pump.get_bits(32);
        assert_eq!(val3, 0x08090A0B);
        assert_eq!(val4, 0x0C0D0E0F);
    }

    #[test]
    fn test_bit_pump_fast_path_with_stuffing() {
        // Fast path should detect 0xFF and fallback to slow path
        // 00 01 02 03 FF 00 05 06 ...
        // FF 00 -> FF literal
        let data = [
            0x00, 0x01, 0x02, 0x03, 0xFF, 0x00, 0x05, 0x06, 0x07, 0x08, 0x09,
        ];
        let mut pump = BitPump::new(&data);

        // read 32 bits: 00 01 02 03
        assert_eq!(pump.get_bits(32), 0x00010203);

        // read next 32 bits: FF 05 06 07
        // 0xFF 00 becomes just 0xFF
        assert_eq!(pump.get_bits(32), 0xFF050607);
    }
}
