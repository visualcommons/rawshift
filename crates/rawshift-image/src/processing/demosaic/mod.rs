use crate::core::RgbImage;
use crate::core::image::RawImage;

/// Error type for demosaicing operations.
#[derive(Debug, Clone)]
pub enum DemosaicError {
    /// Output buffer size does not match expected size
    BufferSizeMismatch {
        /// Expected buffer size in u16 elements
        expected: usize,
        /// Actual buffer size provided
        actual: usize,
    },
    /// Invalid image dimensions
    InvalidDimensions,
    /// The requested demosaicing algorithm is not yet implemented
    UnsupportedAlgorithm(&'static str),
}

impl std::fmt::Display for DemosaicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DemosaicError::BufferSizeMismatch { expected, actual } => {
                write!(
                    f,
                    "buffer size mismatch: expected {} elements, got {}",
                    expected, actual
                )
            }
            DemosaicError::InvalidDimensions => write!(f, "invalid image dimensions"),
            DemosaicError::UnsupportedAlgorithm(name) => {
                write!(f, "demosaicing algorithm '{}' is not yet implemented", name)
            }
        }
    }
}

impl std::error::Error for DemosaicError {}

// =============================================================================
// High-Level API: DemosaicMethod enum
// =============================================================================

/// High-level selection of demosaicing based on sensor architecture.
///
/// This enum provides a user-friendly API for selecting demosaicing algorithms.
/// Use [`to_demosaic()`](Self::to_demosaic) to get a trait object that implements
/// the actual algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DemosaicMethod {
    /// Automatically detects sensor pattern (Bayer vs X-Trans) from metadata
    /// and chooses the best algorithm for the ISO/exposure.
    #[default]
    Auto,

    /// Algorithms strictly for 2x2 Bayer patterns (Sony, Canon, Nikon, DNG, etc.)
    Bayer(BayerAlgorithm),

    /// Algorithms strictly for 6x6 Fujifilm X-Trans patterns.
    XTrans(XTransAlgorithm),

    /// Returns the raw monochrome CFA data without color reconstruction.
    None,
}

impl std::fmt::Display for DemosaicMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DemosaicMethod::Auto => write!(f, "Auto"),
            DemosaicMethod::Bayer(algo) => write!(f, "Bayer({})", algo),
            DemosaicMethod::XTrans(algo) => write!(f, "XTrans({})", algo),
            DemosaicMethod::None => write!(f, "None"),
        }
    }
}

impl DemosaicMethod {
    /// Get the demosaicing algorithm implementation.
    ///
    /// For `Auto`, this defaults to AMaZE for Bayer sensors.
    /// For `None`, this returns a no-op passthrough.
    pub fn to_demosaic(&self) -> Box<dyn Demosaic + Send + Sync> {
        match self {
            DemosaicMethod::Auto => {
                // Default to AMaZE for Bayer sensors (best quality)
                Box::new(bayer::Amaze)
            }
            DemosaicMethod::Bayer(algo) => algo.to_demosaic(),
            DemosaicMethod::XTrans(algo) => algo.to_demosaic(),
            DemosaicMethod::None => Box::new(NoDemosaic),
        }
    }
}

/// Valid algorithms for standard Bayer sensors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BayerAlgorithm {
    /// Industry standard for high-detail, low-noise images.
    #[default]
    Amaze,
    /// High-ISO specialist; treats noise as a statistical probability.
    Lmmse,
    /// Fast, high-quality alternative to AMaZE; great for organic shapes.
    Rcd,
    /// Very fast; low-quality. Suitable for previews.
    Bilinear,
}

impl std::fmt::Display for BayerAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BayerAlgorithm::Amaze => write!(f, "AMaZE"),
            BayerAlgorithm::Lmmse => write!(f, "LMMSE"),
            BayerAlgorithm::Rcd => write!(f, "RCD"),
            BayerAlgorithm::Bilinear => write!(f, "Bilinear"),
        }
    }
}

impl BayerAlgorithm {
    /// Get the demosaicing algorithm implementation.
    pub fn to_demosaic(&self) -> Box<dyn Demosaic + Send + Sync> {
        match self {
            BayerAlgorithm::Amaze => Box::new(bayer::Amaze),
            BayerAlgorithm::Lmmse => Box::new(bayer::Lmmse),
            BayerAlgorithm::Rcd => Box::new(bayer::Rcd),
            BayerAlgorithm::Bilinear => Box::new(Bilinear),
        }
    }
}

/// Valid algorithms for Fujifilm X-Trans sensors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum XTransAlgorithm {
    /// The standard for X-Trans; handles the complex 6x6 grid.
    #[default]
    Markesteijn,
    /// Slower 3-pass version that aggressively reduces moiré/false colors.
    Markesteijn3Pass,
    /// Faster, simpler interpolation for X-Trans previews.
    Fast,
}

impl std::fmt::Display for XTransAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XTransAlgorithm::Markesteijn => write!(f, "Markesteijn"),
            XTransAlgorithm::Markesteijn3Pass => write!(f, "Markesteijn 3-Pass"),
            XTransAlgorithm::Fast => write!(f, "Fast"),
        }
    }
}

impl XTransAlgorithm {
    /// Get the demosaicing algorithm implementation.
    pub fn to_demosaic(&self) -> Box<dyn Demosaic + Send + Sync> {
        match self {
            XTransAlgorithm::Markesteijn => Box::new(xtrans::Markesteijn),
            XTransAlgorithm::Markesteijn3Pass => Box::new(xtrans::Markesteijn3Pass),
            XTransAlgorithm::Fast => Box::new(xtrans::XTransFast),
        }
    }
}

// =============================================================================
// Demosaic Trait (Low-Level Implementation Interface)
// =============================================================================

/// Trait for demosaicing algorithms.
///
/// Implementors should override [`demosaic_into`](Self::demosaic_into) as the primary method.
/// The [`demosaic`](Self::demosaic) method provides a convenience wrapper that allocates output.
///
/// # Example
///
/// ```ignore
/// use rawshift::processing::demosaic::{Bilinear, Demosaic};
///
/// let demosaiced = Bilinear.demosaic(&raw_image);
/// ```
pub trait Demosaic {
    /// Demosaic a raw image into a pre-allocated RGB buffer.
    ///
    /// This is the primary method that implementors must override.
    /// The output buffer must have exactly `width * height * 3` elements.
    ///
    /// # Arguments
    /// * `raw` - The raw image to demosaic
    /// * `output` - Pre-allocated buffer for RGB output (interleaved R, G, B, R, G, B, ...)
    ///
    /// # Errors
    /// Returns [`DemosaicError::BufferSizeMismatch`] if buffer size is incorrect.
    fn demosaic_into(&self, raw: &RawImage, output: &mut [u16]) -> Result<(), DemosaicError>;

    /// Demosaic a raw image into a newly allocated RGB image.
    ///
    /// This is a convenience wrapper that allocates the output buffer
    /// and calls [`demosaic_into`](Self::demosaic_into).
    #[must_use]
    fn demosaic(&self, raw: &RawImage) -> RgbImage {
        let width = raw.active_area().size.width;
        let height = raw.active_area().size.height;
        let mut data = vec![0u16; (width as usize) * (height as usize) * 3];
        self.demosaic_into(raw, &mut data)
            .expect("demosaic_into failed with correctly sized buffer");
        RgbImage::new(width, height, data).expect("width*height*3 buffer allocated above")
    }
}

// =============================================================================
// Submodules
// =============================================================================

mod bilinear;
pub use bilinear::Bilinear;

pub mod bayer;
pub mod xtrans;

// =============================================================================
// No-op Demosaic (for DemosaicMethod::None)
// =============================================================================

/// No-op demosaicing that copies raw values to the first channel.
///
/// This is used when `DemosaicMethod::None` is selected. It produces
/// a grayscale image from the raw CFA data without any interpolation.
pub struct NoDemosaic;

impl Demosaic for NoDemosaic {
    fn demosaic_into(&self, raw: &RawImage, output: &mut [u16]) -> Result<(), DemosaicError> {
        let width = raw.active_area().size.width as usize;
        let height = raw.active_area().size.height as usize;
        let x_offset = raw.active_area().origin.x as usize;
        let y_offset = raw.active_area().origin.y as usize;
        let raw_width = raw.width() as usize;

        let expected_size = width * height * 3;
        if output.len() != expected_size {
            return Err(DemosaicError::BufferSizeMismatch {
                expected: expected_size,
                actual: output.len(),
            });
        }

        // Copy raw values to all three channels (grayscale output)
        for y in 0..height {
            for x in 0..width {
                let raw_idx = (y + y_offset) * raw_width + (x + x_offset);
                let out_idx = (y * width + x) * 3;
                let value = raw.data[raw_idx];
                output[out_idx] = value;
                output[out_idx + 1] = value;
                output[out_idx + 2] = value;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demosaic_error_display() {
        let err = DemosaicError::BufferSizeMismatch {
            expected: 300,
            actual: 100,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("300"));
        assert!(msg.contains("100"));

        let err = DemosaicError::InvalidDimensions;
        let msg = format!("{}", err);
        assert!(msg.contains("dimension"));
    }

    #[test]
    fn test_demosaic_method_default() {
        let method = DemosaicMethod::default();
        assert_eq!(method, DemosaicMethod::Auto);
    }

    #[test]
    fn test_bayer_algorithm_default() {
        let algo = BayerAlgorithm::default();
        assert_eq!(algo, BayerAlgorithm::Amaze);
    }

    #[test]
    fn test_xtrans_algorithm_default() {
        let algo = XTransAlgorithm::default();
        assert_eq!(algo, XTransAlgorithm::Markesteijn);
    }

    #[test]
    fn test_demosaic_method_variants() {
        // Just ensure all variants can be created
        let _ = DemosaicMethod::Auto;
        let _ = DemosaicMethod::Bayer(BayerAlgorithm::Bilinear);
        let _ = DemosaicMethod::XTrans(XTransAlgorithm::Fast);
        let _ = DemosaicMethod::None;
    }
}
