//! Pixel type system for generic image processing.
//!
//! Provides traits and type aliases for working with different pixel
//! representations (u8, u16, f32) in a uniform way.

/// Trait for scalar sample values used in image processing.
///
/// Implemented for `u8`, `u16`, and `f32` — the common pixel sample types
/// in raw image processing pipelines.
pub trait Sample:
    Copy + Clone + PartialOrd + Default + Send + Sync + 'static + Into<f32> + FromF32
{
    /// The maximum representable value for this sample type.
    const MAX: Self;

    /// The minimum representable value for this sample type.
    const MIN: Self;

    /// The number of bits used to represent this sample.
    const BIT_DEPTH: u8;

    /// Clamp a value to the valid range.
    fn clamp_sample(self) -> Self;

    /// Convert from a normalized f32 value in [0.0, 1.0] to this sample type.
    fn from_normalized(v: f32) -> Self;

    /// Convert this sample to a normalized f32 value in [0.0, 1.0].
    fn to_normalized(self) -> f32;
}

/// Conversion trait from f32 to a sample type.
pub trait FromF32 {
    fn from_f32(v: f32) -> Self;
}

impl FromF32 for u8 {
    #[inline]
    fn from_f32(v: f32) -> Self {
        v.round().clamp(0.0, 255.0) as u8
    }
}

impl FromF32 for u16 {
    #[inline]
    fn from_f32(v: f32) -> Self {
        v.round().clamp(0.0, 65535.0) as u16
    }
}

impl FromF32 for f32 {
    #[inline]
    fn from_f32(v: f32) -> Self {
        v
    }
}

impl Sample for u8 {
    const MAX: Self = 255;
    const MIN: Self = 0;
    const BIT_DEPTH: u8 = 8;

    #[inline]
    fn clamp_sample(self) -> Self {
        self // u8 is always in range
    }

    #[inline]
    fn from_normalized(v: f32) -> Self {
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    }

    #[inline]
    fn to_normalized(self) -> f32 {
        self as f32 / 255.0
    }
}

impl Sample for u16 {
    const MAX: Self = 65535;
    const MIN: Self = 0;
    const BIT_DEPTH: u8 = 16;

    #[inline]
    fn clamp_sample(self) -> Self {
        self // u16 is always in range
    }

    #[inline]
    fn from_normalized(v: f32) -> Self {
        (v * 65535.0).round().clamp(0.0, 65535.0) as u16
    }

    #[inline]
    fn to_normalized(self) -> f32 {
        self as f32 / 65535.0
    }
}

impl Sample for f32 {
    const MAX: Self = 1.0;
    const MIN: Self = 0.0;
    const BIT_DEPTH: u8 = 32;

    #[inline]
    fn clamp_sample(self) -> Self {
        self.clamp(0.0, 1.0)
    }

    #[inline]
    fn from_normalized(v: f32) -> Self {
        v
    }

    #[inline]
    fn to_normalized(self) -> f32 {
        self
    }
}

/// An RGB pixel with three components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb<S: Sample> {
    pub r: S,
    pub g: S,
    pub b: S,
}

impl<S: Sample> Rgb<S> {
    /// Create a new RGB pixel.
    #[inline]
    pub fn new(r: S, g: S, b: S) -> Self {
        Self { r, g, b }
    }

    /// Convert to normalized f32 RGB.
    #[inline]
    pub fn to_f32(self) -> Rgb<f32> {
        Rgb {
            r: self.r.to_normalized(),
            g: self.g.to_normalized(),
            b: self.b.to_normalized(),
        }
    }

    /// Convert from normalized f32 RGB.
    #[inline]
    pub fn from_f32(src: Rgb<f32>) -> Self {
        Rgb {
            r: S::from_normalized(src.r),
            g: S::from_normalized(src.g),
            b: S::from_normalized(src.b),
        }
    }
}

impl<S: Sample> Default for Rgb<S> {
    fn default() -> Self {
        Self {
            r: S::default(),
            g: S::default(),
            b: S::default(),
        }
    }
}

/// An RGBA pixel with four components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba<S: Sample> {
    pub r: S,
    pub g: S,
    pub b: S,
    pub a: S,
}

impl<S: Sample> Rgba<S> {
    /// Create a new RGBA pixel.
    #[inline]
    pub fn new(r: S, g: S, b: S, a: S) -> Self {
        Self { r, g, b, a }
    }

    /// Convert to RGB, discarding the alpha channel.
    #[inline]
    pub fn to_rgb(self) -> Rgb<S> {
        Rgb::new(self.r, self.g, self.b)
    }
}

impl<S: Sample> Default for Rgba<S> {
    fn default() -> Self {
        Self {
            r: S::default(),
            g: S::default(),
            b: S::default(),
            a: S::MAX,
        }
    }
}

/// Convenience type aliases for common pixel representations.
pub type Rgb8 = Rgb<u8>;
pub type Rgb16 = Rgb<u16>;
pub type RgbF32 = Rgb<f32>;
pub type Rgba8 = Rgba<u8>;
pub type Rgba16 = Rgba<u16>;
pub type RgbaF32 = Rgba<f32>;

/// Convert a slice of interleaved u16 RGB data to a Vec of Rgb16 pixels.
pub fn rgb16_from_interleaved(data: &[u16]) -> Vec<Rgb16> {
    debug_assert!(data.len().is_multiple_of(3));
    data.chunks_exact(3)
        .map(|c| Rgb16::new(c[0], c[1], c[2]))
        .collect()
}

/// Convert a slice of Rgb16 pixels to interleaved u16 data.
pub fn rgb16_to_interleaved(pixels: &[Rgb16]) -> Vec<u16> {
    let mut data = Vec::with_capacity(pixels.len() * 3);
    for p in pixels {
        data.push(p.r);
        data.push(p.g);
        data.push(p.b);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_u8_sample() {
        assert_eq!(u8::MAX, 255);
        assert_eq!(u8::from_normalized(1.0), 255);
        assert_eq!(u8::from_normalized(0.0), 0);
        assert_eq!(u8::from_normalized(0.5), 128);
        assert!((128u8.to_normalized() - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_u16_sample() {
        assert_eq!(u16::MAX, 65535);
        assert_eq!(u16::from_normalized(1.0), 65535);
        assert_eq!(u16::from_normalized(0.0), 0);
        let half = u16::from_normalized(0.5);
        assert!((half as i32 - 32768).abs() <= 1);
    }

    #[test]
    fn test_f32_sample() {
        assert_eq!(f32::from_normalized(0.5), 0.5);
        assert_eq!((0.5f32).to_normalized(), 0.5);
        assert_eq!((1.5f32).clamp_sample(), 1.0);
        assert_eq!((-0.5f32).clamp_sample(), 0.0);
    }

    #[test]
    fn test_rgb_pixel() {
        let p = Rgb16::new(1000, 2000, 3000);
        assert_eq!(p.r, 1000);
        assert_eq!(p.g, 2000);
        assert_eq!(p.b, 3000);
    }

    #[test]
    fn test_rgb_roundtrip() {
        let p = Rgb16::new(1000, 2000, 3000);
        let f = p.to_f32();
        let back = Rgb16::from_f32(f);
        assert!((back.r as i32 - 1000).abs() <= 1);
        assert!((back.g as i32 - 2000).abs() <= 1);
        assert!((back.b as i32 - 3000).abs() <= 1);
    }

    #[test]
    fn test_rgba_default() {
        let p = Rgba16::default();
        assert_eq!(p.r, 0);
        assert_eq!(p.g, 0);
        assert_eq!(p.b, 0);
        assert_eq!(p.a, 65535);
    }

    #[test]
    fn test_interleaved_roundtrip() {
        let data = vec![100u16, 200, 300, 400, 500, 600];
        let pixels = rgb16_from_interleaved(&data);
        assert_eq!(pixels.len(), 2);
        assert_eq!(pixels[0], Rgb16::new(100, 200, 300));
        let back = rgb16_to_interleaved(&pixels);
        assert_eq!(back, data);
    }
}
