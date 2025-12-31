use crate::processing::demosaic::{Bilinear, DemosaicMethod};

/// Options for processing a raw image.
///
/// This struct controls the entire pipeline from raw data to the final exported image:
/// 1. Demosaicing (Raw -> RGB)
/// 2. White Balance
/// 3. Color Matrix
/// 4. Gamma Correction
#[derive(Clone)]
pub struct ProcessingOptions {
    /// The demosaicing algorithm to use.
    pub demosaic: DemosaicAlgorithm,
    /// White balance multipliers (R, G, B).
    pub white_balance: Option<(f32, f32, f32)>,
    /// Color matrix (3x3 row-major) to transform from Camera RGB to output space (e.g. sRGB).
    pub color_matrix: Option<[f32; 9]>,
    /// Gamma correction value (e.g. 2.2).
    pub gamma: Option<f32>,
}

impl Default for ProcessingOptions {
    fn default() -> Self {
        Self {
            demosaic: DemosaicAlgorithm::Bilinear,
            white_balance: None, // No WB by default (or user provides)
            color_matrix: None,  // No matrix by default
            gamma: None,         // Linear by default
        }
    }
}

impl ProcessingOptions {
    /// Create a new builder for ProcessingOptions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the demosaicing algorithm.
    pub fn demosaic(mut self, algorithm: DemosaicAlgorithm) -> Self {
        self.demosaic = algorithm;
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

/// Available demosaicing algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DemosaicAlgorithm {
    #[default]
    Bilinear,
    /// Adaptive Homogeneity-Directed (AHD) interpolation
    Ahd,
    /// Variable Number of Gradients (VNG) interpolation
    Vng,
}

impl DemosaicAlgorithm {
    /// Get the implementation of this algorithm.
    pub fn implementation(&self) -> Box<dyn DemosaicMethod + Send + Sync> {
        match self {
            DemosaicAlgorithm::Bilinear => Box::new(Bilinear),
            DemosaicAlgorithm::Ahd => unimplemented!("AHD demosaicing is not yet implemented"),
            DemosaicAlgorithm::Vng => unimplemented!("VNG demosaicing is not yet implemented"),
        }
    }
}
