//! The RGB image container for decoded and developed images.
//!
//! [`RgbImage`] wraps gamut's validated [`ImageBuf<Rgb16>`] and adds the
//! rawshift-specific carry-alongs: the [`ColorDescription`] tag, baseline
//! exposure, and the default crop. The buffer invariant (`data.len() ==
//! width * height * 3`) is enforced by gamut at every construction and
//! mutation point — there is no way to hold an `RgbImage` whose data length
//! disagrees with its dimensions.

use rawshift_core::{ColorDescription, Dimensions, ImageBuf, Rgb16};

use crate::core::Rect;
use crate::error::{RawError, RawResult};

/// A container for interleaved 16-bit RGB image data (R, G, B, R, G, B, …).
///
/// Backed by [`ImageBuf<Rgb16>`], which validates the length invariant on
/// construction. Pixel data is reached through [`data`](Self::data) /
/// [`data_mut`](Self::data_mut); dimension changes go through
/// [`replace_data`](Self::replace_data), which revalidates atomically.
#[derive(Debug, Clone)]
pub struct RgbImage {
    buf: ImageBuf<Rgb16>,
    color: ColorDescription,
    baseline_exposure: Option<f32>,
    default_crop: Option<Rect>,
}

impl RgbImage {
    /// Create a new `RgbImage` with an
    /// [`UNSPECIFIED`](ColorDescription::UNSPECIFIED) color description.
    ///
    /// Use [`with_color`](Self::with_color) or
    /// [`set_color`](Self::set_color) when the space is known.
    ///
    /// # Errors
    /// Returns [`RawError::Gamut`] when `data.len() != width * height * 3` or
    /// either dimension is zero.
    pub fn new(width: u32, height: u32, data: Vec<u16>) -> RawResult<Self> {
        Self::with_color(width, height, data, ColorDescription::UNSPECIFIED)
    }

    /// Create a new `RgbImage` tagged with a known color description.
    ///
    /// # Errors
    /// Returns [`RawError::Gamut`] when `data.len() != width * height * 3` or
    /// either dimension is zero.
    pub fn with_color(
        width: u32,
        height: u32,
        data: Vec<u16>,
        color: ColorDescription,
    ) -> RawResult<Self> {
        let dims = Dimensions::new(width, height)
            .map_err(|e| RawError::gamut("RgbImage dimensions", e))?;
        let buf = ImageBuf::<Rgb16>::new(data, dims)
            .map_err(|e| RawError::gamut("RgbImage buffer", e))?;
        Ok(Self::from_buf(buf, color))
    }

    /// Wrap an already-validated gamut buffer.
    pub fn from_buf(buf: ImageBuf<Rgb16>, color: ColorDescription) -> Self {
        Self {
            buf,
            color,
            baseline_exposure: None,
            default_crop: None,
        }
    }

    // ── Read accessors ───────────────────────────────────────────────────

    /// Image dimensions.
    pub fn size(&self) -> Dimensions {
        self.buf.dimensions()
    }

    /// Image width in pixels.
    pub fn width(&self) -> u32 {
        self.buf.width()
    }

    /// Image height in pixels.
    pub fn height(&self) -> u32 {
        self.buf.height()
    }

    /// Interleaved RGB samples (R, G, B, R, G, B, …), row-major.
    pub fn data(&self) -> &[u16] {
        self.buf.as_samples()
    }

    /// Mutable interleaved RGB samples.
    ///
    /// The slice length is fixed by the dimensions; to change both together
    /// use [`replace_data`](Self::replace_data).
    pub fn data_mut(&mut self) -> &mut [u16] {
        self.buf.as_mut_samples()
    }

    /// The underlying gamut buffer.
    pub fn as_buf(&self) -> &ImageBuf<Rgb16> {
        &self.buf
    }

    /// Consume into the underlying gamut buffer (for hand-off to gamut
    /// encoders).
    pub fn into_buf(self) -> ImageBuf<Rgb16> {
        self.buf
    }

    /// Consume into the raw sample vector.
    pub fn into_data(self) -> Vec<u16> {
        self.buf.into_samples()
    }

    /// Baseline exposure offset in EV.
    pub fn baseline_exposure(&self) -> Option<f32> {
        self.baseline_exposure
    }

    /// Default crop rectangle.
    pub fn default_crop(&self) -> Option<Rect> {
        self.default_crop
    }

    /// The color description the RGB samples are in.
    pub fn color(&self) -> ColorDescription {
        self.color
    }

    // ── Write accessors ──────────────────────────────────────────────────

    /// Set baseline exposure offset.
    pub fn set_baseline_exposure(&mut self, ev: Option<f32>) {
        self.baseline_exposure = ev;
    }

    /// Set the color description tag for the RGB samples.
    pub fn set_color(&mut self, color: ColorDescription) {
        self.color = color;
    }

    /// Set default crop rectangle.
    pub fn set_default_crop(&mut self, crop: Option<Rect>) {
        self.default_crop = crop;
    }

    /// Replace dimensions and data together (used by orientation transforms
    /// and crops, which change both).
    ///
    /// Atomic: on error the image is left unchanged. The color tag, baseline
    /// exposure, and default crop are preserved.
    ///
    /// # Errors
    /// Returns [`RawError::Gamut`] when `data.len() != width * height * 3` or
    /// either dimension is zero.
    pub fn replace_data(&mut self, width: u32, height: u32, data: Vec<u16>) -> RawResult<()> {
        let dims = Dimensions::new(width, height)
            .map_err(|e| RawError::gamut("RgbImage dimensions", e))?;
        let buf = ImageBuf::<Rgb16>::new(data, dims)
            .map_err(|e| RawError::gamut("RgbImage buffer", e))?;
        self.buf = buf;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_image_indexing() {
        // RgbImage stores interleaved RGB: R G B R G B ...
        let data = vec![
            100u16, 200, 300, // pixel 0: R=100, G=200, B=300
            400, 500, 600, // pixel 1: R=400, G=500, B=600
        ];
        let img = RgbImage::new(2, 1, data).expect("valid buffer");

        assert_eq!(img.data()[0], 100, "pixel 0 R");
        assert_eq!(img.data()[1], 200, "pixel 0 G");
        assert_eq!(img.data()[2], 300, "pixel 0 B");
        assert_eq!(img.data()[3], 400, "pixel 1 R");
        assert_eq!(img.data()[4], 500, "pixel 1 G");
        assert_eq!(img.data()[5], 600, "pixel 1 B");

        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 1);
        assert_eq!(img.data().len(), 6);
    }

    #[test]
    fn rgb_image_accessors() {
        let img = RgbImage::new(100, 200, vec![0u16; 100 * 200 * 3]).expect("valid buffer");
        assert_eq!(img.width(), 100);
        assert_eq!(img.height(), 200);
        assert_eq!(
            img.size(),
            Dimensions {
                width: 100,
                height: 200
            }
        );
        assert_eq!(img.baseline_exposure(), None);
        assert_eq!(img.default_crop(), None);
        assert_eq!(img.color(), ColorDescription::UNSPECIFIED);
    }

    #[test]
    fn length_invariant_is_enforced() {
        // Wrong length: 2x1 RGB needs 6 samples.
        assert!(matches!(
            RgbImage::new(2, 1, vec![0u16; 5]),
            Err(RawError::Gamut { .. })
        ));
        // Zero dimension.
        assert!(matches!(
            RgbImage::new(0, 1, vec![]),
            Err(RawError::Gamut { .. })
        ));
    }

    #[test]
    fn replace_data_is_atomic() {
        let mut img =
            RgbImage::with_color(2, 1, vec![0u16; 6], ColorDescription::LINEAR_SRGB).unwrap();
        img.set_baseline_exposure(Some(0.5));

        // A failed replace leaves everything unchanged.
        assert!(img.replace_data(3, 1, vec![0u16; 5]).is_err());
        assert_eq!(img.width(), 2);
        assert_eq!(img.data().len(), 6);

        // A successful replace swaps dims+data and preserves the carry-alongs.
        img.replace_data(1, 2, vec![1u16; 6]).unwrap();
        assert_eq!(img.width(), 1);
        assert_eq!(img.height(), 2);
        assert_eq!(img.color(), ColorDescription::LINEAR_SRGB);
        assert_eq!(img.baseline_exposure(), Some(0.5));
    }

    #[test]
    fn data_mut_edits_in_place() {
        let mut img = RgbImage::new(1, 1, vec![1, 2, 3]).unwrap();
        img.data_mut()[1] = 42;
        assert_eq!(img.data(), &[1, 42, 3]);
    }
}
