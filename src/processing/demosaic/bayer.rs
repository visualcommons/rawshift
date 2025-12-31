//! Bayer-specific demosaicing algorithms.
//!
//! This module contains demosaicing algorithms designed for standard 2x2 Bayer
//! color filter arrays found in most cameras (Sony, Canon, Nikon, DNG, etc.).

use super::{Demosaic, DemosaicError};
use crate::core::image::RawImage;

/// AMaZE (Aliasing Minimization and Zipper Elimination) demosaicing algorithm.
///
/// Industry standard for high-detail, low-noise images. This algorithm provides
/// excellent edge detection and artifact reduction.
///
/// Reference: [RawTherapee AMaZE implementation](https://github.com/RawTherapee/RawTherapee)
pub struct Amaze;

impl Demosaic for Amaze {
    fn demosaic_into(&self, _raw: &RawImage, _output: &mut [u16]) -> Result<(), DemosaicError> {
        unimplemented!("AMaZE")
    }
}

/// LMMSE (Linear Minimum Mean Square Error) demosaicing algorithm.
///
/// High-ISO specialist that treats noise as a statistical probability.
/// Particularly effective for images shot at high ISO where noise is prominent.
pub struct Lmmse;

impl Demosaic for Lmmse {
    fn demosaic_into(&self, _raw: &RawImage, _output: &mut [u16]) -> Result<(), DemosaicError> {
        unimplemented!("LMMSE")
    }
}

/// RCD (Ratio Corrected Demosaicing) algorithm.
///
/// Fast, high-quality alternative to AMaZE that's particularly good for
/// organic shapes and natural textures.
pub struct Rcd;

impl Demosaic for Rcd {
    fn demosaic_into(&self, _raw: &RawImage, _output: &mut [u16]) -> Result<(), DemosaicError> {
        unimplemented!("RCD")
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::core::image::{CfaPattern, Point, Rect, Size};

//     // fn create_test_raw() -> RawImage {
//     //     RawImage {
//     //         size: Size::new(4, 4),
//     //         active_area: Rect::new(Point::ORIGIN, Size::new(4, 4)),
//     //         bit_depth: 14,
//     //         cfa_pattern: CfaPattern::Rggb,
//     //         black_levels: [0; 4],
//     //         white_level: 16383,
//     //         data: vec![1000; 16],
//     //     }
//     // }
// }
