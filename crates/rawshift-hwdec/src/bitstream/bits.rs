#![cfg_attr(not(hwdec_backend = "vaapi"), allow(dead_code))]
//! Safe bitstream readers shared by the HEVC and AV1 still-picture parsers.
//!
//! **Safe Rust only** — this module (like `hevc.rs` / `av1.rs`) contains no
//! `unsafe`; all FFI stays in `sys.rs` and the call sites in `mod.rs`.
//!
//! [`BitReader`] is a plain MSB-first bit cursor with the Exp-Golomb
//! (`ue`/`se`, H.265) and AV1 (`uvlc`/`su`/`ns`/`le`/`leb128`) integer
//! codings layered on top. [`rbsp_from_nal_payload`] strips H.264/H.265
//! emulation-prevention bytes and remembers where they were, so the HEVC
//! slice parser can report VAAPI's `slice_data_byte_offset` /
//! `slice_data_num_emu_prevn_bytes` pair.

/// Error type for all header parsing: a static description of what was
/// malformed or out of scope. Converted to `HwDecodeError::Decode` at the
/// backend boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError(pub &'static str);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

pub type PResult<T> = Result<T, ParseError>;

const OVERRUN: ParseError = ParseError("bitstream ended inside a syntax element");

/// An MSB-first bit reader over a byte slice.
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    /// Absolute bit position from the start of `data`.
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }

    /// Bits consumed so far.
    pub fn bit_pos(&self) -> usize {
        self.pos
    }

    /// Whether at least `n` more bits are available.
    pub fn has_bits(&self, n: usize) -> bool {
        self.data.len() * 8 - self.pos >= n
    }

    /// Read one bit.
    pub fn bit(&mut self) -> PResult<u32> {
        let byte = *self.data.get(self.pos / 8).ok_or(OVERRUN)?;
        let bit = (byte >> (7 - (self.pos % 8))) & 1;
        self.pos += 1;
        Ok(u32::from(bit))
    }

    /// Read one bit as a flag.
    pub fn flag(&mut self) -> PResult<bool> {
        Ok(self.bit()? == 1)
    }

    /// Read `n` bits (0..=32), MSB first — `u(n)` in both specs.
    pub fn bits(&mut self, n: u32) -> PResult<u32> {
        debug_assert!(n <= 32);
        let mut value: u64 = 0;
        for _ in 0..n {
            value = (value << 1) | u64::from(self.bit()?);
        }
        Ok(value as u32)
    }

    /// Skip `n` bits.
    pub fn skip(&mut self, n: usize) -> PResult<()> {
        if !self.has_bits(n) {
            return Err(OVERRUN);
        }
        self.pos += n;
        Ok(())
    }

    // ── H.265 Exp-Golomb ────────────────────────────────────────────────────

    /// `ue(v)`: unsigned Exp-Golomb (H.265 §9.2).
    pub fn ue(&mut self) -> PResult<u32> {
        let mut leading = 0u32;
        while self.bit()? == 0 {
            leading += 1;
            if leading > 31 {
                return Err(ParseError("Exp-Golomb code longer than 32 bits"));
            }
        }
        if leading == 0 {
            return Ok(0);
        }
        let rest = self.bits(leading)?;
        Ok((1u32 << leading) - 1 + rest)
    }

    /// `ue(v)` with an inclusive upper bound (conformance check).
    pub fn ue_max(&mut self, max: u32, what: &'static str) -> PResult<u32> {
        let v = self.ue()?;
        if v > max {
            return Err(ParseError(what));
        }
        Ok(v)
    }

    /// `se(v)`: signed Exp-Golomb (H.265 §9.2.2).
    pub fn se(&mut self) -> PResult<i32> {
        let k = self.ue()?;
        // Mapping: 0→0, 1→1, 2→-1, 3→2, 4→-2, …
        let magnitude = k.div_ceil(2) as i32;
        Ok(if k % 2 == 1 { magnitude } else { -magnitude })
    }

    // ── AV1 integer codings (AV1 spec §4.10) ───────────────────────────────

    /// `uvlc()`: variable-length unsigned.
    pub fn uvlc(&mut self) -> PResult<u32> {
        let mut leading = 0u32;
        while self.bit()? == 0 {
            leading += 1;
            if leading > 31 {
                return Err(ParseError("uvlc code longer than 32 bits"));
            }
        }
        if leading == 0 {
            return Ok(0);
        }
        let rest = self.bits(leading)?;
        Ok((1u32 << leading) - 1 + rest)
    }

    /// `su(1+n)`: sign-magnitude signed with `n` value bits.
    pub fn su(&mut self, n: u32) -> PResult<i32> {
        let value = self.bits(n)? as i32;
        let sign_mask = 1i32 << (n - 1);
        Ok(if value & sign_mask != 0 {
            value - 2 * sign_mask
        } else {
            value
        })
    }

    /// `ns(n)`: non-symmetric unsigned with maximum `n - 1` (AV1 §4.10.7).
    pub fn ns(&mut self, n: u32) -> PResult<u32> {
        if n == 0 {
            return Err(ParseError("ns(0) is undefined"));
        }
        let w = floor_log2(n) + 1;
        let m = (1u32 << w) - n;
        if w == 1 {
            return Ok(0);
        }
        let v = self.bits(w - 1)?;
        if v < m {
            return Ok(v);
        }
        let extra = self.bit()?;
        Ok((v << 1) - m + extra)
    }

    /// `leb128()` (AV1 §4.10.5), byte-aligned, at most 8 bytes, value must
    /// fit 32 bits per the spec's conformance requirement on obu_size.
    pub fn leb128(&mut self) -> PResult<u64> {
        let mut value: u64 = 0;
        for i in 0..8u32 {
            let byte = self.bits(8)?;
            value |= u64::from(byte & 0x7f) << (i * 7);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(ParseError("leb128 longer than 8 bytes"))
    }

    /// AV1 `byte_alignment()` / H.265 skip-to-alignment: advance to the next
    /// byte boundary (discarding the alignment bits without validating them).
    pub fn align_to_byte(&mut self) -> PResult<()> {
        let rem = self.pos % 8;
        if rem != 0 {
            self.skip(8 - rem)?;
        }
        Ok(())
    }
}

/// `FloorLog2(x)` for `x >= 1` (AV1 §4.10.7 helper).
pub fn floor_log2(x: u32) -> u32 {
    31 - x.leading_zeros()
}

/// `Clip3(low, high, value)`.
pub fn clip3(low: i32, high: i32, value: i32) -> i32 {
    value.clamp(low, high)
}

/// A NAL payload with emulation-prevention bytes removed, remembering where
/// each `emulation_prevention_three_byte` sat so byte offsets in the RBSP
/// can be related back to the escaped stream.
#[derive(Debug)]
pub struct Rbsp {
    /// The de-escaped RBSP bytes.
    pub data: Vec<u8>,
    /// For each removed `0x03`, the RBSP byte offset it preceded (sorted).
    epb_before: Vec<usize>,
}

impl Rbsp {
    /// How many emulation-prevention bytes were removed within the first
    /// `rbsp_bytes` bytes of the RBSP.
    pub fn epb_count_within(&self, rbsp_bytes: usize) -> usize {
        self.epb_before.partition_point(|&p| p < rbsp_bytes)
    }
}

/// De-escape the payload of a NAL unit (the bytes **after** the two-byte NAL
/// header): every `00 00 03` with the `03` followed by `00`, `01`, `02` or
/// `03` drops the `03` (H.265 §7.4.2 / §7.3.1.1).
pub fn rbsp_from_nal_payload(payload: &[u8]) -> Rbsp {
    let mut data = Vec::with_capacity(payload.len());
    let mut epb_before = Vec::new();
    let mut zeros = 0usize;
    let mut i = 0usize;
    while i < payload.len() {
        let b = payload[i];
        if zeros >= 2 && b == 0x03 && payload.get(i + 1).is_none_or(|&n| n <= 0x03) {
            // Emulation prevention byte: drop it (it terminates the zero run).
            epb_before.push(data.len());
            zeros = 0;
            i += 1;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        data.push(b);
        i += 1;
    }
    Rbsp { data, epb_before }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_and_fixed_width_reads() {
        let mut r = BitReader::new(&[0b1011_0011, 0b1000_0000]);
        assert_eq!(r.bit().unwrap(), 1);
        assert_eq!(r.bits(3).unwrap(), 0b011);
        assert_eq!(r.bits(4).unwrap(), 0b0011);
        assert!(r.flag().unwrap());
        assert_eq!(r.bit_pos(), 9);
        assert!(r.bits(8).is_err(), "overrun must error");
    }

    #[test]
    fn exp_golomb_ue_se() {
        // ue: 1 → 0; 010 → 1; 011 → 2; 00100 → 3.
        let mut r = BitReader::new(&[0b1010_0110, 0b0100_0000]);
        assert_eq!(r.ue().unwrap(), 0);
        assert_eq!(r.ue().unwrap(), 1);
        assert_eq!(r.ue().unwrap(), 2);
        assert_eq!(r.ue().unwrap(), 3);

        // se mapping: k=1→+1, k=2→-1, k=3→+2, k=4→-2
        // (codes 010, 011, 00100, 00101 → 0100_1100 1000_0101).
        let mut r = BitReader::new(&[0x4C, 0x85]);
        assert_eq!(r.se().unwrap(), 1);
        assert_eq!(r.se().unwrap(), -1);
        assert_eq!(r.se().unwrap(), 2);
        assert_eq!(r.se().unwrap(), -2);
    }

    #[test]
    fn av1_su_ns_leb128_le() {
        // su(4): 0b0111 = 7; 0b1001 = 1 - 2*8 = -7.
        let mut r = BitReader::new(&[0b0111_1001]);
        assert_eq!(r.su(4).unwrap(), 7);
        assert_eq!(r.su(4).unwrap(), -7);

        // ns(3): w=2, m=1. "0" → 0; "10" → v=1 ≥ m → (2-1)+bit.
        let mut r = BitReader::new(&[0b0100_0000]);
        assert_eq!(r.ns(3).unwrap(), 0);
        assert_eq!(r.ns(3).unwrap(), 1);

        // leb128: 0xE5 0x8E 0x26 = 624485.
        let mut r = BitReader::new(&[0xE5, 0x8E, 0x26]);
        assert_eq!(r.leb128().unwrap(), 624_485);
    }

    #[test]
    fn rbsp_de_escaping_tracks_epbs() {
        // 00 00 03 01 | AA | 00 00 03 00 → RBSP 00 00 01 AA 00 00 00.
        let rbsp = rbsp_from_nal_payload(&[0x00, 0x00, 0x03, 0x01, 0xAA, 0x00, 0x00, 0x03, 0x00]);
        assert_eq!(rbsp.data, [0x00, 0x00, 0x01, 0xAA, 0x00, 0x00, 0x00]);
        assert_eq!(rbsp.epb_count_within(2), 0);
        assert_eq!(rbsp.epb_count_within(3), 1);
        assert_eq!(rbsp.epb_count_within(7), 2);

        // A 03 not preceded by two zeros is kept; 00 00 03 FF keeps the 03
        // (next byte > 0x03 means it was not an EPB).
        let rbsp = rbsp_from_nal_payload(&[0x00, 0x03, 0x00, 0x00, 0x03, 0xFF]);
        assert_eq!(rbsp.data, [0x00, 0x03, 0x00, 0x00, 0x03, 0xFF]);
        assert_eq!(rbsp.epb_count_within(6), 0);
    }

    #[test]
    fn alignment_and_clip() {
        let mut r = BitReader::new(&[0xFF, 0x0F]);
        r.bits(3).unwrap();
        r.align_to_byte().unwrap();
        assert_eq!(r.bit_pos(), 8);
        r.align_to_byte().unwrap();
        assert_eq!(r.bit_pos(), 8);
        assert_eq!(clip3(0, 255, -4), 0);
        assert_eq!(floor_log2(1), 0);
        assert_eq!(floor_log2(64), 6);
    }
}
