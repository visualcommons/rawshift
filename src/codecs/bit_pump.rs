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
    fn fill(&mut self) {
        while self.bits_left <= 56 && self.pos < self.data.len() {
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
        while self.bits_left < n && self.pos < self.data.len() {
            self.fill_one();
        }
        if self.bits_left < n {
            // Not enough data - return what we have padded with zeros
            return (self.bits as u32) << (n - self.bits_left);
        }
        ((self.bits >> (self.bits_left - n)) & ((1u64 << n) - 1)) as u32
    }

    /// Fill one byte into the bit buffer.
    fn fill_one(&mut self) {
        if self.pos >= self.data.len() {
            return;
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
                return;
            } else if next == 0xD9 {
                // EOI - stop filling
                return;
            }
        }

        self.bits = (self.bits << 8) | byte;
        self.bits_left += 8;
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
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Get the number of bits remaining in the buffer.
    #[inline]
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
}
