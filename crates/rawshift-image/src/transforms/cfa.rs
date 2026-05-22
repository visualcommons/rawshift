//! Color Filter Array (CFA) Demosaicing.
//!
//! This transform module acts as a high-level wrapper around the demosaicing algorithms
//! defined in [`crate::processing::demosaic`]. It handles the selection of the appropriate
//! algorithm and manages the conversion from raw sensor data to RGB image buffers.

use crate::core::image::{RawImage, RgbImage};
use crate::error::RawResult;
use crate::processing::demosaic::DemosaicMethod;

/// Demosaicing transform pipeline step.
pub struct CfaTransform {
    method: DemosaicMethod,
}

impl CfaTransform {
    /// Create a new CFA transform with the specified demosaicing method.
    pub fn new(method: DemosaicMethod) -> Self {
        Self { method }
    }

    /// Apply demosaicing to a RawImage, producing an RgbImage.
    pub fn apply(&self, raw: &RawImage) -> RawResult<RgbImage> {
        let demosaic = self.method.to_demosaic();
        Ok(demosaic.demosaic(raw))
    }
}

impl Default for CfaTransform {
    fn default() -> Self {
        Self::new(DemosaicMethod::default())
    }
}
