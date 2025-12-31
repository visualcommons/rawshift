//! Processing options for the raw image pipeline.

use crate::processing::demosaic::DemosaicMethod;

/// Options for processing a raw image.
///
/// This struct controls the entire pipeline from raw data to the final exported image:
/// 1. Demosaicing (Raw -> RGB)
/// 2. White Balance
/// 3. Color Matrix
/// 4. Gamma Correction
#[derive(Clone, Default)]
pub struct ProcessingOptions {
    /// The demosaicing method to use.
    pub demosaic: DemosaicMethod,
    /// White balance multipliers (R, G, B).
    pub white_balance: Option<(f32, f32, f32)>,
    /// Color matrix (3x3 row-major) to transform from Camera RGB to output space (e.g. sRGB).
    pub color_matrix: Option<[f32; 9]>,
    /// Gamma correction value (e.g. 2.2).
    pub gamma: Option<f32>,
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
}
