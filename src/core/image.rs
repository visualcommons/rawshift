//! Core image structures and types.
//!
//! This module defines the fundamental structures for representing
//! image dimensions, coordinates, and raw image data.

/// Compute the maximum pixel value (white level) for a given bit depth, clamped to `u16`.
///
/// Returns `u16::MAX` when `bit_depth >= 16`, and `(1 << bit_depth) - 1` otherwise.
#[inline]
pub fn white_level_from_bit_depth(bit_depth: u8) -> u16 {
    if bit_depth >= 16 {
        u16::MAX
    } else if bit_depth == 0 {
        0
    } else {
        (1u16 << bit_depth) - 1
    }
}

/// Image dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
}

impl Size {
    /// Create a new Size.
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Check if dimensions are valid (non-zero).
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Total number of pixels.
    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

/// A point in image coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    /// X coordinate
    pub x: u32,
    /// Y coordinate
    pub y: u32,
}

impl Point {
    /// Create a new Point.
    pub fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }

    /// Origin point (0, 0).
    pub const ORIGIN: Point = Point { x: 0, y: 0 };
}

/// A rectangular region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Origin (top-left corner)
    pub origin: Point,
    /// Size of the rectangle
    pub size: Size,
}

impl Rect {
    /// Create a new Rect.
    pub fn new(origin: Point, size: Size) -> Self {
        Self { origin, size }
    }

    /// Create a rect from coordinates.
    pub fn from_coords(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            origin: Point::new(x, y),
            size: Size::new(width, height),
        }
    }

    /// Right edge (x + width).
    pub fn right(&self) -> u32 {
        self.origin.x.saturating_add(self.size.width)
    }

    /// Bottom edge (y + height).
    pub fn bottom(&self) -> u32 {
        self.origin.y.saturating_add(self.size.height)
    }
}

/// CFA (Color Filter Array) pattern.
///
/// Represents the Bayer pattern used in the camera's sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfaPattern {
    /// Red-Green / Green-Blue
    Rggb,
    /// Green-Red / Blue-Green
    Grbg,
    /// Blue-Green / Green-Red
    Bggr,
    /// Green-Blue / Red-Green
    Gbrg,
}

impl CfaPattern {
    /// Parse from a 4-element array (row-major 2x2).
    ///
    /// Values: 0=Red, 1=Green, 2=Blue
    pub fn from_array(pattern: [u8; 4]) -> Option<Self> {
        match pattern {
            [0, 1, 1, 2] => Some(CfaPattern::Rggb),
            [1, 0, 2, 1] => Some(CfaPattern::Grbg),
            [2, 1, 1, 0] => Some(CfaPattern::Bggr),
            [1, 2, 0, 1] => Some(CfaPattern::Gbrg),
            _ => None,
        }
    }

    /// Convert to a 4-element array.
    pub fn to_array(self) -> [u8; 4] {
        match self {
            CfaPattern::Rggb => [0, 1, 1, 2],
            CfaPattern::Grbg => [1, 0, 2, 1],
            CfaPattern::Bggr => [2, 1, 1, 0],
            CfaPattern::Gbrg => [1, 2, 0, 1],
        }
    }

    /// Get a human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            CfaPattern::Rggb => "RGGB",
            CfaPattern::Grbg => "GRBG",
            CfaPattern::Bggr => "BGGR",
            CfaPattern::Gbrg => "GBRG",
        }
    }
}

/// X-Trans CFA pattern (6x6 repeating tile).
///
/// Values: 0=Red, 1=Green, 2=Blue
/// Row-major order, 36 elements total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XTransPattern {
    /// 6x6 grid of color indices: 0=Red, 1=Green, 2=Blue
    pub cells: [[u8; 6]; 6],
}

impl XTransPattern {
    /// Standard Fujifilm X-Trans I pattern (as used in RawTherapee/darktable).
    pub fn standard() -> Self {
        Self {
            cells: [
                [1, 2, 1, 1, 0, 1],
                [0, 1, 0, 2, 1, 2],
                [1, 2, 1, 1, 0, 1],
                [1, 0, 1, 2, 1, 2],
                [1, 2, 1, 1, 0, 1],
                [0, 1, 0, 2, 1, 2],
            ],
        }
    }

    /// Get color at absolute sensor position (x, y) using 6x6 tile wrapping.
    ///
    /// Returns 0=Red, 1=Green, 2=Blue.
    #[inline]
    pub fn color_at(&self, x: usize, y: usize) -> u8 {
        self.cells[y % 6][x % 6]
    }
}

/// Raw image data container.
///
/// Holds the decoded raw sensor data along with associated metadata.
#[derive(Debug, Clone)]
pub struct RawImage {
    /// Full sensor dimensions
    pub size: Size,
    /// Active/crop area (usable image region)
    pub active_area: Rect,
    /// Bits per sample (typically 12, 14, or 16)
    pub bit_depth: u8,
    /// CFA pattern
    pub cfa_pattern: CfaPattern,
    /// X-Trans CFA pattern (6x6 tile). Set for Fujifilm X-Trans sensors.
    /// When `Some`, the Markesteijn demosaic algorithm will use this pattern
    /// instead of `cfa_pattern`.
    pub xtrans_pattern: Option<XTransPattern>,
    /// Black level values (per CFA color channel)
    pub black_levels: [u16; 4],
    /// White/saturation level
    pub white_level: u16,
    /// Raw pixel data (16-bit values, one per sensor pixel)
    /// Stored in row-major order: data[y * width + x]
    pub data: Vec<u16>,
    /// Baseline exposure offset in EV (e.g. -0.8).
    /// Used by DNG to match the "baseline" look. None for standard ARW.
    pub baseline_exposure: Option<f32>,
    /// Default crop rectangle.
    /// Used by DNG to define the final image composition area. None for standard ARW.
    pub default_crop: Option<Rect>,
}

impl RawImage {
    /// Create a new empty RawImage with the given parameters.
    pub fn new(size: Size, active_area: Rect, bit_depth: u8, cfa_pattern: CfaPattern) -> Self {
        let pixel_count = size.pixel_count() as usize;
        Self {
            size,
            active_area,
            bit_depth,
            cfa_pattern,
            xtrans_pattern: None,
            black_levels: [0; 4],
            white_level: white_level_from_bit_depth(bit_depth),
            data: vec![0u16; pixel_count],
            baseline_exposure: None,
            default_crop: None,
        }
    }

    /// Get pixel value at (x, y).
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<u16> {
        if x < self.size.width && y < self.size.height {
            let idx = (y as usize) * (self.size.width as usize) + (x as usize);
            Some(self.data[idx])
        } else {
            None
        }
    }

    /// Set pixel value at (x, y).
    pub fn set_pixel(&mut self, x: u32, y: u32, value: u16) {
        if x < self.size.width && y < self.size.height {
            let idx = (y as usize) * (self.size.width as usize) + (x as usize);
            self.data[idx] = value;
        }
    }
}

/// A simple container for RGB image data
#[derive(Debug, Clone)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    /// Interleaved RGB data (R, G, B, R, G, B...)
    pub data: Vec<u16>,
    /// Baseline exposure offset in EV (e.g. -0.8).
    /// Used by DNG to match the "baseline" look. None for standard ARW.
    pub baseline_exposure: Option<f32>,
    /// Default crop rectangle.
    /// Used by DNG to define the final image composition area. None for standard ARW.
    pub default_crop: Option<Rect>,
}

impl RgbImage {
    /// Create a new RgbImage
    pub fn new(width: u32, height: u32, data: Vec<u16>) -> Self {
        Self {
            width,
            height,
            data,
            baseline_exposure: None,
            default_crop: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_white_level_from_bit_depth() {
        assert_eq!(white_level_from_bit_depth(0), 0);
        assert_eq!(white_level_from_bit_depth(1), 1);
        assert_eq!(white_level_from_bit_depth(8), 255);
        assert_eq!(white_level_from_bit_depth(12), 4095);
        assert_eq!(white_level_from_bit_depth(14), 16383);
        assert_eq!(white_level_from_bit_depth(15), 32767);
        // bit_depth >= 16 clamps to u16::MAX
        assert_eq!(white_level_from_bit_depth(16), u16::MAX);
        assert_eq!(white_level_from_bit_depth(32), u16::MAX);
        assert_eq!(white_level_from_bit_depth(255), u16::MAX);
    }

    #[test]
    fn test_size() {
        let size = Size::new(100, 200);
        assert_eq!(size.pixel_count(), 20000);
        assert!(size.is_valid());

        let empty = Size::new(0, 100);
        assert!(!empty.is_valid());
    }

    #[test]
    fn test_cfa_pattern() {
        assert_eq!(CfaPattern::from_array([0, 1, 1, 2]), Some(CfaPattern::Rggb));
        assert_eq!(CfaPattern::Rggb.to_array(), [0, 1, 1, 2]);
        assert_eq!(CfaPattern::Rggb.name(), "RGGB");
    }

    #[test]
    fn test_raw_image() {
        let size = Size::new(10, 10);
        let active = Rect::from_coords(0, 0, 10, 10);
        let mut img = RawImage::new(size, active, 14, CfaPattern::Rggb);

        img.set_pixel(5, 5, 1000);
        assert_eq!(img.get_pixel(5, 5), Some(1000));
        assert_eq!(img.get_pixel(100, 100), None);
    }

    #[test]
    fn test_raw_image_pixel_access() {
        let size = Size::new(4, 4);
        let active = Rect::from_coords(0, 0, 4, 4);
        let mut img = RawImage::new(size, active, 14, CfaPattern::Rggb);

        // Set several pixels and verify get_pixel returns correct values
        img.set_pixel(0, 0, 100);
        img.set_pixel(3, 0, 200);
        img.set_pixel(0, 3, 300);
        img.set_pixel(3, 3, 400);
        img.set_pixel(2, 1, 500);

        assert_eq!(img.get_pixel(0, 0), Some(100));
        assert_eq!(img.get_pixel(3, 0), Some(200));
        assert_eq!(img.get_pixel(0, 3), Some(300));
        assert_eq!(img.get_pixel(3, 3), Some(400));
        assert_eq!(img.get_pixel(2, 1), Some(500));

        // Out-of-bounds returns None
        assert_eq!(img.get_pixel(4, 0), None);
        assert_eq!(img.get_pixel(0, 4), None);
        assert_eq!(img.get_pixel(u32::MAX, u32::MAX), None);
    }

    #[test]
    fn test_rgb_image_indexing() {
        // RgbImage stores interleaved RGB: R G B R G B ...
        let data = vec![
            100u16, 200, 300, // pixel 0: R=100, G=200, B=300
            400, 500, 600, // pixel 1: R=400, G=500, B=600
        ];
        let img = RgbImage::new(2, 1, data.clone());

        assert_eq!(img.data[0], 100, "pixel 0 R");
        assert_eq!(img.data[1], 200, "pixel 0 G");
        assert_eq!(img.data[2], 300, "pixel 0 B");
        assert_eq!(img.data[3], 400, "pixel 1 R");
        assert_eq!(img.data[4], 500, "pixel 1 G");
        assert_eq!(img.data[5], 600, "pixel 1 B");

        assert_eq!(img.width, 2);
        assert_eq!(img.height, 1);
        assert_eq!(img.data.len(), 6);
    }

    #[test]
    fn test_size_pixel_count() {
        let s = Size::new(1920, 1080);
        assert_eq!(s.pixel_count(), 1920 * 1080);

        // Zero dimension
        let s = Size::new(0, 100);
        assert_eq!(s.pixel_count(), 0);

        // Large dimensions (check u64 doesn't overflow)
        let s = Size::new(10000, 10000);
        assert_eq!(s.pixel_count(), 100_000_000u64);
    }

    #[test]
    fn test_rect_dimensions() {
        let r = Rect::from_coords(10, 20, 100, 200);
        assert_eq!(r.origin.x, 10);
        assert_eq!(r.origin.y, 20);
        assert_eq!(r.size.width, 100);
        assert_eq!(r.size.height, 200);
        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 220);
    }
}
