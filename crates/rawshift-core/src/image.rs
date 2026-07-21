//! Core image structures and types.
//!
//! This module defines the fundamental structures for representing image
//! dimensions, coordinates, and raw sensor data. Pixel dimensions are gamut's
//! [`Dimensions`]; the sensor-specific vocabulary ([`RawImage`],
//! [`CfaPattern`], [`XTransPattern`]) is rawshift's own — a Bayer/X-Trans
//! mosaic has no gamut equivalent.

/// Image dimensions in pixels (re-exported from `gamut-core`).
///
/// Fields are public `u32`s; [`Dimensions::new`] is fallible and rejects
/// zero-sized images, while struct-literal construction is unvalidated for
/// call sites that permit zero (e.g. probes of degenerate headers).
pub use gamut_core::Dimensions;

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

/// Number of pixels in `dims` as a `usize`, for buffer allocation.
///
/// Zero-sized dimensions yield 0. Panics if the product overflows `usize`
/// (impossible on 64-bit targets; on 32-bit it means an allocation that could
/// never succeed anyway).
#[inline]
pub(crate) fn pixel_count(dims: Dimensions) -> usize {
    dims.num_pixels()
        .expect("pixel count overflows usize on this target")
}

/// A point in image coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// A rectangular region: an origin plus [`Dimensions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Origin (top-left corner)
    pub origin: Point,
    /// Size of the rectangle
    pub size: Dimensions,
}

impl Rect {
    /// Create a new Rect.
    pub fn new(origin: Point, size: Dimensions) -> Self {
        Self { origin, size }
    }

    /// Create a rect from coordinates.
    pub fn from_coords(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            origin: Point::new(x, y),
            size: Dimensions { width, height },
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

// Manual serde: `Dimensions` is a gamut type without serde derives
// (visualcommons/gamut#257), so `Rect` flattens to four integers on the wire —
// which is also the more natural stable form.
#[cfg(feature = "serde")]
impl serde::Serialize for Rect {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Rect", 4)?;
        s.serialize_field("x", &self.origin.x)?;
        s.serialize_field("y", &self.origin.y)?;
        s.serialize_field("width", &self.size.width)?;
        s.serialize_field("height", &self.size.height)?;
        s.end()
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Rect {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Wire {
            x: u32,
            y: u32,
            width: u32,
            height: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Rect::from_coords(w.x, w.y, w.width, w.height))
    }
}

/// Serde adapter for the re-exported [`Dimensions`], which carries no serde
/// derives upstream (visualcommons/gamut#257).
///
/// Use on struct fields:
/// `#[cfg_attr(feature = "serde", serde(with = "rawshift_core::image::dimensions_serde"))]`
#[cfg(feature = "serde")]
pub mod dimensions_serde {
    use super::Dimensions;
    use serde::Deserialize;

    /// Serialize as `{ "width": u32, "height": u32 }`.
    pub fn serialize<S: serde::Serializer>(v: &Dimensions, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Dimensions", 2)?;
        st.serialize_field("width", &v.width)?;
        st.serialize_field("height", &v.height)?;
        st.end()
    }

    /// Deserialize from `{ "width": u32, "height": u32 }` (zero permitted, as
    /// with struct-literal construction).
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Dimensions, D::Error> {
        #[derive(serde::Deserialize)]
        struct Wire {
            width: u32,
            height: u32,
        }
        let w = Wire::deserialize(d)?;
        Ok(Dimensions {
            width: w.width,
            height: w.height,
        })
    }
}

/// CFA (Color Filter Array) pattern.
///
/// Represents the Bayer pattern used in the camera's sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
/// Use [`RawImageBuilder`] to construct new instances.
#[derive(Debug, Clone)]
pub struct RawImage {
    size: Dimensions,
    active_area: Rect,
    bit_depth: u8,
    cfa_pattern: CfaPattern,
    xtrans_pattern: Option<XTransPattern>,
    black_levels: [u16; 4],
    white_level: u16,
    /// Raw pixel data (16-bit values, one per sensor pixel).
    /// Stored in row-major order: data[y * width + x]
    pub data: Vec<u16>,
    baseline_exposure: Option<f32>,
    default_crop: Option<Rect>,
}

impl RawImage {
    /// Create a new empty RawImage with the given parameters.
    pub fn new(
        size: Dimensions,
        active_area: Rect,
        bit_depth: u8,
        cfa_pattern: CfaPattern,
    ) -> Self {
        Self {
            size,
            active_area,
            bit_depth,
            cfa_pattern,
            xtrans_pattern: None,
            black_levels: [0; 4],
            white_level: white_level_from_bit_depth(bit_depth),
            data: vec![0u16; pixel_count(size)],
            baseline_exposure: None,
            default_crop: None,
        }
    }

    /// Create a builder for constructing a RawImage.
    pub fn builder(
        size: Dimensions,
        active_area: Rect,
        bit_depth: u8,
        cfa_pattern: CfaPattern,
    ) -> RawImageBuilder {
        RawImageBuilder {
            size,
            active_area,
            bit_depth,
            cfa_pattern,
            xtrans_pattern: None,
            black_levels: [0; 4],
            white_level: white_level_from_bit_depth(bit_depth),
            data: None,
            baseline_exposure: None,
            default_crop: None,
        }
    }

    // ── Read accessors ───────────────────────────────────────────────────

    /// Full sensor dimensions.
    pub fn size(&self) -> Dimensions {
        self.size
    }

    /// Sensor width in pixels.
    pub fn width(&self) -> u32 {
        self.size.width
    }

    /// Sensor height in pixels.
    pub fn height(&self) -> u32 {
        self.size.height
    }

    /// Active/crop area (usable image region).
    pub fn active_area(&self) -> Rect {
        self.active_area
    }

    /// Bits per sample (typically 12, 14, or 16).
    pub fn bit_depth(&self) -> u8 {
        self.bit_depth
    }

    /// CFA (Bayer) pattern.
    pub fn cfa_pattern(&self) -> CfaPattern {
        self.cfa_pattern
    }

    /// X-Trans CFA pattern, if applicable.
    pub fn xtrans_pattern(&self) -> Option<&XTransPattern> {
        self.xtrans_pattern.as_ref()
    }

    /// Black level values (per CFA color channel).
    pub fn black_levels(&self) -> &[u16; 4] {
        &self.black_levels
    }

    /// White/saturation level.
    pub fn white_level(&self) -> u16 {
        self.white_level
    }

    /// Baseline exposure offset in EV.
    pub fn baseline_exposure(&self) -> Option<f32> {
        self.baseline_exposure
    }

    /// Default crop rectangle.
    pub fn default_crop(&self) -> Option<Rect> {
        self.default_crop
    }

    // ── Write accessors ──────────────────────────────────────────────────

    /// Set black level values.
    pub fn set_black_levels(&mut self, levels: [u16; 4]) {
        self.black_levels = levels;
    }

    /// Set white/saturation level.
    pub fn set_white_level(&mut self, level: u16) {
        self.white_level = level;
    }

    /// Set baseline exposure offset.
    pub fn set_baseline_exposure(&mut self, ev: Option<f32>) {
        self.baseline_exposure = ev;
    }

    /// Set default crop rectangle.
    pub fn set_default_crop(&mut self, crop: Option<Rect>) {
        self.default_crop = crop;
    }

    /// Set X-Trans pattern.
    pub fn set_xtrans_pattern(&mut self, pattern: Option<XTransPattern>) {
        self.xtrans_pattern = pattern;
    }

    /// Set bit depth.
    pub fn set_bit_depth(&mut self, bit_depth: u8) {
        self.bit_depth = bit_depth;
    }

    // ── Pixel access ─────────────────────────────────────────────────────

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

/// Builder for constructing [`RawImage`] instances.
pub struct RawImageBuilder {
    size: Dimensions,
    active_area: Rect,
    bit_depth: u8,
    cfa_pattern: CfaPattern,
    xtrans_pattern: Option<XTransPattern>,
    black_levels: [u16; 4],
    white_level: u16,
    data: Option<Vec<u16>>,
    baseline_exposure: Option<f32>,
    default_crop: Option<Rect>,
}

impl RawImageBuilder {
    /// Set black level values.
    pub fn black_levels(mut self, levels: [u16; 4]) -> Self {
        self.black_levels = levels;
        self
    }

    /// Set white/saturation level.
    pub fn white_level(mut self, level: u16) -> Self {
        self.white_level = level;
        self
    }

    /// Set X-Trans pattern.
    pub fn xtrans_pattern(mut self, pattern: XTransPattern) -> Self {
        self.xtrans_pattern = Some(pattern);
        self
    }

    /// Set baseline exposure offset in EV.
    pub fn baseline_exposure(mut self, ev: f32) -> Self {
        self.baseline_exposure = Some(ev);
        self
    }

    /// Set default crop rectangle.
    pub fn default_crop(mut self, crop: Rect) -> Self {
        self.default_crop = Some(crop);
        self
    }

    /// Set pixel data.
    pub fn data(mut self, data: Vec<u16>) -> Self {
        self.data = Some(data);
        self
    }

    /// Build the RawImage.
    pub fn build(self) -> RawImage {
        let data = self
            .data
            .unwrap_or_else(|| vec![0u16; pixel_count(self.size)]);
        RawImage {
            size: self.size,
            active_area: self.active_area,
            bit_depth: self.bit_depth,
            cfa_pattern: self.cfa_pattern,
            xtrans_pattern: self.xtrans_pattern,
            black_levels: self.black_levels,
            white_level: self.white_level,
            data,
            baseline_exposure: self.baseline_exposure,
            default_crop: self.default_crop,
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
    fn test_dimensions() {
        let size = Dimensions {
            width: 100,
            height: 200,
        };
        assert_eq!(pixel_count(size), 20000);
        assert!(!size.is_empty());

        let empty = Dimensions {
            width: 0,
            height: 100,
        };
        assert!(empty.is_empty());
        assert_eq!(pixel_count(empty), 0);

        // The validating constructor rejects zero sizes.
        assert!(Dimensions::new(0, 100).is_err());
        assert!(Dimensions::new(100, 200).is_ok());
    }

    #[test]
    fn test_cfa_pattern() {
        assert_eq!(CfaPattern::from_array([0, 1, 1, 2]), Some(CfaPattern::Rggb));
        assert_eq!(CfaPattern::Rggb.to_array(), [0, 1, 1, 2]);
        assert_eq!(CfaPattern::Rggb.name(), "RGGB");
    }

    #[test]
    fn test_raw_image() {
        let size = Dimensions {
            width: 10,
            height: 10,
        };
        let active = Rect::from_coords(0, 0, 10, 10);
        let mut img = RawImage::new(size, active, 14, CfaPattern::Rggb);

        img.set_pixel(5, 5, 1000);
        assert_eq!(img.get_pixel(5, 5), Some(1000));
        assert_eq!(img.get_pixel(100, 100), None);
    }

    #[test]
    fn test_raw_image_pixel_access() {
        let size = Dimensions {
            width: 4,
            height: 4,
        };
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
    fn test_rect_dimensions() {
        let r = Rect::from_coords(10, 20, 100, 200);
        assert_eq!(r.origin.x, 10);
        assert_eq!(r.origin.y, 20);
        assert_eq!(r.size.width, 100);
        assert_eq!(r.size.height, 200);
        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 220);
    }

    #[test]
    fn test_raw_image_builder() {
        let size = Dimensions {
            width: 10,
            height: 10,
        };
        let active = Rect::from_coords(0, 0, 10, 10);
        let img = RawImage::builder(size, active, 14, CfaPattern::Rggb)
            .black_levels([100, 100, 100, 100])
            .white_level(16383)
            .build();

        assert_eq!(img.size(), size);
        assert_eq!(img.active_area(), active);
        assert_eq!(img.bit_depth(), 14);
        assert_eq!(img.cfa_pattern(), CfaPattern::Rggb);
        assert_eq!(*img.black_levels(), [100, 100, 100, 100]);
        assert_eq!(img.white_level(), 16383);
        assert_eq!(img.data.len(), 100);
    }

    #[test]
    fn test_raw_image_builder_with_data() {
        let size = Dimensions {
            width: 2,
            height: 2,
        };
        let active = Rect::from_coords(0, 0, 2, 2);
        let img = RawImage::builder(size, active, 14, CfaPattern::Rggb)
            .data(vec![1000, 2000, 3000, 4000])
            .build();

        assert_eq!(img.data, vec![1000, 2000, 3000, 4000]);
    }

    #[test]
    fn test_raw_image_setters() {
        let size = Dimensions {
            width: 4,
            height: 4,
        };
        let active = Rect::from_coords(0, 0, 4, 4);
        let mut img = RawImage::new(size, active, 14, CfaPattern::Rggb);

        img.set_black_levels([100, 100, 100, 100]);
        assert_eq!(*img.black_levels(), [100, 100, 100, 100]);

        img.set_white_level(4095);
        assert_eq!(img.white_level(), 4095);

        img.set_baseline_exposure(Some(-0.8));
        assert_eq!(img.baseline_exposure(), Some(-0.8));

        let crop = Rect::from_coords(1, 1, 2, 2);
        img.set_default_crop(Some(crop));
        assert_eq!(img.default_crop(), Some(crop));

        img.set_xtrans_pattern(Some(XTransPattern::standard()));
        assert!(img.xtrans_pattern().is_some());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn rect_serde_round_trip() {
        let r = Rect::from_coords(10, 20, 100, 200);
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"x":10,"y":20,"width":100,"height":200}"#);
        let back: Rect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
