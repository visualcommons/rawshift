//! Black and white level handling.
//!
//! This module handles the normalization of raw sensor data:
//! - Subtraction of Black Level (pedestal).
//! - Normalization against White Level (saturation).
//! - Handling of split black levels (e.g. per-channel or repeating patterns).

use crate::error::RawResult;

/// Applies black level subtraction and white level normalization.
///
/// # TODO
/// - Implement `apply_black_level` taking raw CFA data and the `BlackLevel` tag values.
/// - Handle `BlackLevelDeltaH` and `BlackLevelDeltaV` (common in DNGs).
/// - Handle `BlackLevelRepeatDim` to map the black values to the CFA pattern.
/// - Normalize data to 0.0..1.0 float range (or scaled integer) based on `WhiteLevel`.
pub struct BlackLevelCorrection {
    // TODO: Store repeating pattern and values
}

impl Default for BlackLevelCorrection {
    fn default() -> Self {
        Self::new()
    }
}

impl BlackLevelCorrection {
    pub fn new() -> Self {
        Self {}
    }

    pub fn apply(&self, _data: &mut [u16], _width: usize, _height: usize) -> RawResult<()> {
        // TODO: Loop through pixels and subtract black level
        // TODO: Clamp to 0
        Ok(())
    }
}
