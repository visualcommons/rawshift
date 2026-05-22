//! Processing options for the raw image pipeline.

use crate::processing::demosaic::DemosaicMethod;
use crate::transforms::BadPixelCorrectionMode;

/// Options for processing a raw image.
///
/// This struct controls the entire pipeline from raw data to the final exported image:
/// 1. Bad pixel correction (optional, on raw CFA data)
/// 2. Demosaicing (Raw -> RGB)
/// 3. White Balance
/// 4. Color Matrix
/// 5. Noise reduction (optional, on RGB data)
/// 6. Chromatic aberration correction (optional, on RGB data)
/// 7. Gamma Correction
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProcessingOptions {
    /// The demosaicing method to use.
    pub demosaic: DemosaicMethod,
    /// White balance multipliers (R, G, B).
    pub white_balance: Option<(f32, f32, f32)>,
    /// Color matrix (3x3 row-major) to transform from Camera RGB to output space (e.g. sRGB).
    pub color_matrix: Option<[f32; 9]>,
    /// Gamma correction value (e.g. 2.2).
    pub gamma: Option<f32>,
    /// Bad pixel correction mode.
    ///
    /// When `Some`, bad pixels are detected and corrected on the raw CFA data
    /// before demosaicing. Uses a threshold factor of 0.5.
    pub bad_pixel_correction: Option<BadPixelCorrectionMode>,
    /// Noise reduction bilateral filter sigma.
    ///
    /// When `Some`, a bilateral filter is applied to the RGB image after
    /// demosaicing. The value is used as both the spatial sigma (in pixels)
    /// and a scaled range sigma (`sigma * 10000`). A typical value is `2.0`.
    pub denoise_sigma: Option<f32>,
    /// Chromatic aberration correction scale factors `(red_scale, blue_scale)`.
    ///
    /// When `Some`, the R and B channels are rescaled relative to the image
    /// centre to correct lateral chromatic aberration. Values near `1.0` make
    /// small corrections (e.g. `(0.999, 1.001)`).
    pub ca_correction: Option<(f32, f32)>,
}

impl ProcessingOptions {
    /// Create a new builder for ProcessingOptions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the demosaicing method.
    pub fn demosaic(mut self, method: DemosaicMethod) -> Self {
        self.demosaic = method;
        self
    }

    /// Set white balance multipliers.
    pub fn white_balance(mut self, r: f32, g: f32, b: f32) -> Self {
        self.white_balance = Some((r, g, b));
        self
    }

    /// Set the color matrix.
    pub fn color_matrix(mut self, matrix: [f32; 9]) -> Self {
        self.color_matrix = Some(matrix);
        self
    }

    /// Set the gamma value.
    pub fn gamma(mut self, gamma: f32) -> Self {
        self.gamma = Some(gamma);
        self
    }

    /// Enable bad pixel correction with the given mode.
    pub fn bad_pixel_correction(mut self, mode: BadPixelCorrectionMode) -> Self {
        self.bad_pixel_correction = Some(mode);
        self
    }

    /// Enable bilateral noise reduction with the given spatial sigma.
    pub fn denoise(mut self, sigma: f32) -> Self {
        self.denoise_sigma = Some(sigma);
        self
    }

    /// Enable chromatic aberration correction with per-channel scale factors.
    pub fn ca_correction(mut self, red_scale: f32, blue_scale: f32) -> Self {
        self.ca_correction = Some((red_scale, blue_scale));
        self
    }
}
