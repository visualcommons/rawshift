//! TIFF data types and value representations.
//!
//! This module defines the fundamental types used in TIFF files:
//! - Byte ordering (Little/Big Endian)
//! - The 12 standard TIFF data types
//! - Rational number types
//! - Value containers for parsed tag values

use binrw::{BinRead, BinWrite};
use std::fmt;

/// Byte order marker for TIFF files.
///
/// TIFF files begin with either "II" (Intel, little-endian) or "MM" (Motorola, big-endian).
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
pub enum ByteOrder {
    /// Little-endian byte order (Intel, "II")
    #[brw(magic = b"II")]
    LittleEndian,
    /// Big-endian byte order (Motorola, "MM")
    #[brw(magic = b"MM")]
    BigEndian,
}

impl ByteOrder {
    /// Parse byte order from the first two bytes of a TIFF file.
    pub fn from_bytes(bytes: [u8; 2]) -> Option<Self> {
        match &bytes {
            b"II" => Some(ByteOrder::LittleEndian),
            b"MM" => Some(ByteOrder::BigEndian),
            _ => None,
        }
    }

    /// Returns the two-byte marker for this byte order.
    pub fn to_bytes(self) -> [u8; 2] {
        match self {
            ByteOrder::LittleEndian => *b"II",
            ByteOrder::BigEndian => *b"MM",
        }
    }

    /// Returns the string representation ("LE" or "BE") for annotations.
    pub fn as_str(&self) -> &'static str {
        match self {
            ByteOrder::LittleEndian => "LE",
            ByteOrder::BigEndian => "BE",
        }
    }
}

impl fmt::Display for ByteOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ByteOrder::LittleEndian => write!(f, "Little-Endian (II)"),
            ByteOrder::BigEndian => write!(f, "Big-Endian (MM)"),
        }
    }
}

/// TIFF data type codes.
///
/// These are the 12 standard data types defined by the TIFF specification,
/// plus IFD/IFD8 for pointer types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(repr = u16)]
#[repr(u16)]
pub enum TiffType {
    /// 8-bit unsigned integer
    Byte = 1,
    /// 8-bit byte containing ASCII character (null-terminated)
    Ascii = 2,
    /// 16-bit unsigned integer
    Short = 3,
    /// 32-bit unsigned integer
    Long = 4,
    /// Two LONGs: numerator and denominator
    Rational = 5,
    /// 8-bit signed integer
    SByte = 6,
    /// 8-bit byte that may contain anything
    Undefined = 7,
    /// 16-bit signed integer
    SShort = 8,
    /// 32-bit signed integer
    SLong = 9,
    /// Two SLONGs: numerator and denominator
    SRational = 10,
    /// Single precision IEEE floating point (4 bytes)
    Float = 11,
    /// Double precision IEEE floating point (8 bytes)
    Double = 12,
    /// 32-bit unsigned integer offset to IFD
    Ifd = 13,
    /// 64-bit unsigned integer (BigTIFF)
    Long8 = 16,
    /// 64-bit signed integer (BigTIFF)
    SLong8 = 17,
    /// 64-bit unsigned integer offset to IFD (BigTIFF)
    Ifd8 = 18,
}

impl TiffType {
    /// Parse a type code from u16, returning None for unknown types.
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(TiffType::Byte),
            2 => Some(TiffType::Ascii),
            3 => Some(TiffType::Short),
            4 => Some(TiffType::Long),
            5 => Some(TiffType::Rational),
            6 => Some(TiffType::SByte),
            7 => Some(TiffType::Undefined),
            8 => Some(TiffType::SShort),
            9 => Some(TiffType::SLong),
            10 => Some(TiffType::SRational),
            11 => Some(TiffType::Float),
            12 => Some(TiffType::Double),
            13 => Some(TiffType::Ifd),
            16 => Some(TiffType::Long8),
            17 => Some(TiffType::SLong8),
            18 => Some(TiffType::Ifd8),
            _ => None,
        }
    }

    /// Returns the size in bytes of a single element of this type.
    pub fn size(&self) -> usize {
        match self {
            TiffType::Byte | TiffType::Ascii | TiffType::SByte | TiffType::Undefined => 1,
            TiffType::Short | TiffType::SShort => 2,
            TiffType::Long | TiffType::SLong | TiffType::Float | TiffType::Ifd => 4,
            TiffType::Rational
            | TiffType::SRational
            | TiffType::Double
            | TiffType::Long8
            | TiffType::SLong8
            | TiffType::Ifd8 => 8,
        }
    }

    /// Returns whether a value of this type with the given count fits inline in an IFD entry.
    ///
    /// Standard TIFF IFD entries have 4 bytes for the value/offset field.
    /// If the total data size fits in 4 bytes, it's stored inline.
    pub fn fits_inline(&self, count: u32) -> bool {
        self.size() * (count as usize) <= 4
    }

    /// Returns whether a value of this type with the given count fits inline in a BigTIFF entry.
    ///
    /// BigTIFF IFD entries have 8 bytes for the value/offset field.
    pub fn fits_inline_bigtiff(&self, count: u64) -> bool {
        self.size() * (count as usize) <= 8
    }
}

/// Unsigned rational number (two 32-bit unsigned integers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rational {
    /// Numerator
    pub numerator: u32,
    /// Denominator
    pub denominator: u32,
}

impl Rational {
    /// Create a new Rational.
    pub fn new(numerator: u32, denominator: u32) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    /// Convert to f64, returning infinity if denominator is zero.
    pub fn to_f64(&self) -> f64 {
        if self.denominator == 0 {
            if self.numerator == 0 {
                f64::NAN
            } else {
                f64::INFINITY
            }
        } else {
            self.numerator as f64 / self.denominator as f64
        }
    }
}

impl From<(u32, u32)> for Rational {
    fn from((numerator, denominator): (u32, u32)) -> Self {
        Self {
            numerator,
            denominator,
        }
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

/// Signed rational number (two 32-bit signed integers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SRational {
    /// Numerator
    pub numerator: i32,
    /// Denominator
    pub denominator: i32,
}

impl SRational {
    /// Create a new SRational.
    pub fn new(numerator: i32, denominator: i32) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    /// Convert to f64, returning infinity if denominator is zero.
    pub fn to_f64(&self) -> f64 {
        if self.denominator == 0 {
            if self.numerator == 0 {
                f64::NAN
            } else if self.numerator > 0 {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            }
        } else {
            self.numerator as f64 / self.denominator as f64
        }
    }
}

impl From<(i32, i32)> for SRational {
    fn from((numerator, denominator): (i32, i32)) -> Self {
        Self {
            numerator,
            denominator,
        }
    }
}

impl fmt::Display for SRational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

// ── Conversions to the format-agnostic metadata rationals ────────────────────
// The wire-level TIFF rationals carry binrw derives and cannot live in
// `rawshift-core`; these `From` impls bridge them to the core metadata model.

impl From<Rational> for rawshift_core::metadata::URational {
    fn from(r: Rational) -> Self {
        Self::new(r.numerator, r.denominator)
    }
}

impl From<SRational> for rawshift_core::metadata::SRational {
    fn from(r: SRational) -> Self {
        Self::new(r.numerator, r.denominator)
    }
}

/// Container for parsed TIFF tag values.
///
/// This enum holds the actual parsed data for a tag value,
/// with variants for each possible TIFF data type.
#[derive(Debug, Clone, PartialEq)]
pub enum TiffValue {
    /// BYTE values (u8)
    Bytes(Vec<u8>),
    /// ASCII string (without null terminator)
    Ascii(String),
    /// SHORT values (u16)
    Shorts(Vec<u16>),
    /// LONG values (u32)
    Longs(Vec<u32>),
    /// RATIONAL values
    Rationals(Vec<Rational>),
    /// SBYTE values (i8)
    SBytes(Vec<i8>),
    /// UNDEFINED values (raw bytes)
    Undefined(Vec<u8>),
    /// SSHORT values (i16)
    SShorts(Vec<i16>),
    /// SLONG values (i32)
    SLongs(Vec<i32>),
    /// SRATIONAL values
    SRationals(Vec<SRational>),
    /// FLOAT values (f32)
    Floats(Vec<f32>),
    /// DOUBLE values (f64)
    Doubles(Vec<f64>),
    /// LONG8 values (u64, BigTIFF)
    Long8s(Vec<u64>),
    /// SLONG8 values (i64, BigTIFF)
    SLong8s(Vec<i64>),
}

impl TiffValue {
    /// Get as a single u32 value, if applicable.
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            TiffValue::Bytes(v) if v.len() == 1 => Some(v[0] as u32),
            TiffValue::Shorts(v) if v.len() == 1 => Some(v[0] as u32),
            TiffValue::Longs(v) if v.len() == 1 => Some(v[0]),
            _ => None,
        }
    }

    /// Get the first element as u32, useful for array tags like BitsPerSample.
    pub fn first_u32(&self) -> Option<u32> {
        match self {
            TiffValue::Bytes(v) if !v.is_empty() => Some(v[0] as u32),
            TiffValue::Shorts(v) if !v.is_empty() => Some(v[0] as u32),
            TiffValue::Longs(v) if !v.is_empty() => Some(v[0]),
            _ => None,
        }
    }

    /// Get as a Vec<u32>, coercing numeric types if possible.
    pub fn as_u32_vec(&self) -> Option<Vec<u32>> {
        match self {
            TiffValue::Bytes(v) => Some(v.iter().map(|&x| x as u32).collect()),
            TiffValue::Shorts(v) => Some(v.iter().map(|&x| x as u32).collect()),
            TiffValue::Longs(v) => Some(v.clone()),
            _ => None,
        }
    }

    /// Get as a single u64 value, if applicable.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            TiffValue::Bytes(v) if v.len() == 1 => Some(v[0] as u64),
            TiffValue::Shorts(v) if v.len() == 1 => Some(v[0] as u64),
            TiffValue::Longs(v) if v.len() == 1 => Some(v[0] as u64),
            TiffValue::Long8s(v) if v.len() == 1 => Some(v[0]),
            _ => None,
        }
    }

    /// Get as a Vec<u64>, coercing numeric types if possible.
    pub fn as_u64_vec(&self) -> Option<Vec<u64>> {
        match self {
            TiffValue::Bytes(v) => Some(v.iter().map(|&x| x as u64).collect()),
            TiffValue::Shorts(v) => Some(v.iter().map(|&x| x as u64).collect()),
            TiffValue::Longs(v) => Some(v.iter().map(|&x| x as u64).collect()),
            TiffValue::Long8s(v) => Some(v.clone()),
            _ => None,
        }
    }

    /// Get as a Vec<f64>, coercing numeric types if possible.
    /// Supports Rationals, SRationals, Floats, Doubles, and integer types.
    pub fn as_f64_vec(&self) -> Option<Vec<f64>> {
        match self {
            TiffValue::Bytes(v) => Some(v.iter().map(|&x| x as f64).collect()),
            TiffValue::Shorts(v) => Some(v.iter().map(|&x| x as f64).collect()),
            TiffValue::Longs(v) => Some(v.iter().map(|&x| x as f64).collect()),
            TiffValue::Rationals(v) => Some(v.iter().map(|r| r.to_f64()).collect()),
            TiffValue::SRationals(v) => Some(v.iter().map(|r| r.to_f64()).collect()),
            TiffValue::Floats(v) => Some(v.iter().map(|&x| x as f64).collect()),
            TiffValue::Doubles(v) => Some(v.clone()),
            _ => None,
        }
    }

    /// Get as string, if this is an ASCII value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            TiffValue::Ascii(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Get the number of elements in this value.
    pub fn len(&self) -> usize {
        match self {
            TiffValue::Bytes(v) | TiffValue::Undefined(v) => v.len(),
            TiffValue::Ascii(s) => s.len() + 1, // Include null terminator
            TiffValue::Shorts(v) => v.len(),
            TiffValue::Longs(v) => v.len(),
            TiffValue::Rationals(v) => v.len(),
            TiffValue::SBytes(v) => v.len(),
            TiffValue::SShorts(v) => v.len(),
            TiffValue::SLongs(v) => v.len(),
            TiffValue::SRationals(v) => v.len(),
            TiffValue::Floats(v) => v.len(),
            TiffValue::Doubles(v) => v.len(),
            TiffValue::Long8s(v) => v.len(),
            TiffValue::SLong8s(v) => v.len(),
        }
    }

    /// Check if the value is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_order_from_bytes() {
        assert_eq!(ByteOrder::from_bytes(*b"II"), Some(ByteOrder::LittleEndian));
        assert_eq!(ByteOrder::from_bytes(*b"MM"), Some(ByteOrder::BigEndian));
        assert_eq!(ByteOrder::from_bytes(*b"XX"), None);
    }

    #[test]
    fn test_byte_order_to_bytes() {
        assert_eq!(ByteOrder::LittleEndian.to_bytes(), *b"II");
        assert_eq!(ByteOrder::BigEndian.to_bytes(), *b"MM");
    }

    #[test]
    fn test_byte_order_as_str() {
        assert_eq!(ByteOrder::LittleEndian.as_str(), "LE");
        assert_eq!(ByteOrder::BigEndian.as_str(), "BE");
    }

    #[test]
    fn test_tiff_type_size() {
        assert_eq!(TiffType::Byte.size(), 1);
        assert_eq!(TiffType::Ascii.size(), 1);
        assert_eq!(TiffType::Short.size(), 2);
        assert_eq!(TiffType::Long.size(), 4);
        assert_eq!(TiffType::Rational.size(), 8);
        assert_eq!(TiffType::Double.size(), 8);
        assert_eq!(TiffType::Long8.size(), 8);
    }

    #[test]
    fn test_tiff_type_fits_inline() {
        // 4 bytes or less fit inline
        assert!(TiffType::Byte.fits_inline(4));
        assert!(TiffType::Byte.fits_inline(1));
        assert!(!TiffType::Byte.fits_inline(5));

        assert!(TiffType::Short.fits_inline(2));
        assert!(!TiffType::Short.fits_inline(3));

        assert!(TiffType::Long.fits_inline(1));
        assert!(!TiffType::Long.fits_inline(2));

        // Rational (8 bytes) never fits in 4
        assert!(!TiffType::Rational.fits_inline(1));
    }

    #[test]
    fn test_rational_to_f64() {
        let r = Rational::new(1, 2);
        assert_eq!(r.to_f64(), 0.5);

        let r = Rational::new(0, 0);
        assert!(r.to_f64().is_nan());

        let r = Rational::new(1, 0);
        assert!(r.to_f64().is_infinite());
    }

    #[test]
    fn test_srational_to_f64() {
        let r = SRational::new(-1, 2);
        assert_eq!(r.to_f64(), -0.5);

        let r = SRational::new(-1, 0);
        assert_eq!(r.to_f64(), f64::NEG_INFINITY);
    }

    #[test]
    fn test_tiff_value_as_u32() {
        let v = TiffValue::Shorts(vec![42]);
        assert_eq!(v.as_u32(), Some(42));

        let v = TiffValue::Longs(vec![1000]);
        assert_eq!(v.as_u32(), Some(1000));

        let v = TiffValue::Longs(vec![1, 2]);
        assert_eq!(v.as_u32(), None); // More than one element
    }

    #[test]
    fn test_tiff_value_as_u64() {
        // Short variant
        let v = TiffValue::Shorts(vec![255]);
        assert_eq!(v.as_u64(), Some(255u64));

        // Long variant
        let v = TiffValue::Longs(vec![100_000]);
        assert_eq!(v.as_u64(), Some(100_000u64));

        // Long8 variant
        let v = TiffValue::Long8s(vec![u64::MAX]);
        assert_eq!(v.as_u64(), Some(u64::MAX));

        // More than one element → None
        let v = TiffValue::Shorts(vec![1, 2]);
        assert_eq!(v.as_u64(), None);

        // Unrelated variant → None
        let v = TiffValue::Ascii("hello".to_string());
        assert_eq!(v.as_u64(), None);
    }

    #[test]
    fn test_tiff_value_as_string() {
        let v = TiffValue::Ascii("Canon".to_string());
        assert_eq!(v.as_str(), Some("Canon"));

        // Non-ASCII variants return None
        let v = TiffValue::Shorts(vec![1]);
        assert_eq!(v.as_str(), None);

        let v = TiffValue::Bytes(vec![65, 66]);
        assert_eq!(v.as_str(), None);
    }
}
