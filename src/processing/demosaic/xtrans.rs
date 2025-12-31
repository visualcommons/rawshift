//! X-Trans-specific demosaicing algorithms.
//!
//! This module contains demosaicing algorithms designed for Fujifilm's 6x6
//! X-Trans color filter array pattern.

use super::{Demosaic, DemosaicError};
use crate::core::image::RawImage;

/// Markesteijn demosaicing algorithm for X-Trans sensors.
///
/// The standard algorithm for X-Trans sensors that properly handles
/// the complex 6x6 grid pattern unique to Fujifilm cameras.
pub struct Markesteijn;

impl Demosaic for Markesteijn {
    fn demosaic_into(&self, _raw: &RawImage, _output: &mut [u16]) -> Result<(), DemosaicError> {
        unimplemented!("Markesteijn")
    }
}

/// Markesteijn 3-pass demosaicing algorithm for X-Trans sensors.
///
/// A slower, more thorough version of the Markesteijn algorithm that
/// performs three passes to aggressively reduce moiré and false colors.
pub struct Markesteijn3Pass;

impl Demosaic for Markesteijn3Pass {
    fn demosaic_into(&self, _raw: &RawImage, _output: &mut [u16]) -> Result<(), DemosaicError> {
        unimplemented!("Markesteijn3Pass")
    }
}

/// Fast demosaicing algorithm for X-Trans sensors.
///
/// A faster, simpler interpolation method for X-Trans previews
/// where speed is more important than absolute quality.
pub struct XTransFast;

impl Demosaic for XTransFast {
    fn demosaic_into(&self, _raw: &RawImage, _output: &mut [u16]) -> Result<(), DemosaicError> {
        unimplemented!("X-Trans Fast")
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::core::image::{CfaPattern, Point, Rect, Size};

//     // fn create_test_raw() -> RawImage {
//     //     RawImage {
//     //         size: Size::new(6, 6),
//     //         active_area: Rect::new(Point::ORIGIN, Size::new(6, 6)),
//     //         bit_depth: 14,
//     //         cfa_pattern: CfaPattern::Rggb, // Note: X-Trans would need its own pattern
//     //         black_levels: [0; 4],
//     //         white_level: 16383,
//     //         data: vec![1000; 36],
//     //     }
//     // }
// }
