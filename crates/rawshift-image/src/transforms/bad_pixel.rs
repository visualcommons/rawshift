//! Bad pixel correction for CFA sensor data.
//!
//! Bad (hot or dead) pixels appear as abnormally bright or dark spots.
//! This module provides detection and correction algorithms that operate
//! directly on raw CFA data before demosaicing.

use crate::core::image::{CfaPattern, RawImage};

/// Bad pixel correction modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BadPixelCorrectionMode {
    /// Median of same-color neighbors in a 5×5 window.
    Median,
    /// Average of same-color neighbors in a 5×5 window.
    Average,
}

/// Return the CFA color index (0=R, 1=G_r, 2=B, 3=G_b) for a given pixel position.
///
/// Uses the same color assignment as the demosaic code.
#[inline]
fn cfa_color(x: u32, y: u32, pattern: CfaPattern) -> u8 {
    match pattern {
        CfaPattern::Rggb => match (x % 2, y % 2) {
            (0, 0) => 0,
            (1, 0) => 1,
            (0, 1) => 3,
            _ => 2,
        },
        CfaPattern::Grbg => match (x % 2, y % 2) {
            (0, 0) => 1,
            (1, 0) => 0,
            (0, 1) => 2,
            _ => 3,
        },
        CfaPattern::Bggr => match (x % 2, y % 2) {
            (0, 0) => 2,
            (1, 0) => 3,
            (0, 1) => 1,
            _ => 0,
        },
        CfaPattern::Gbrg => match (x % 2, y % 2) {
            (0, 0) => 3,
            (1, 0) => 2,
            (0, 1) => 0,
            _ => 1,
        },
    }
}

/// Collect all same-CFA-color neighbors of (cx, cy) within a 5×5 window.
///
/// The center pixel itself is excluded.
fn collect_same_color_neighbors(raw: &RawImage, cx: u32, cy: u32) -> Vec<u16> {
    let center_color = cfa_color(cx, cy, raw.cfa_pattern());
    let width = raw.width();
    let height = raw.height();

    let x_min = cx.saturating_sub(2);
    let x_max = (cx + 2).min(width - 1);
    let y_min = cy.saturating_sub(2);
    let y_max = (cy + 2).min(height - 1);

    let mut neighbors = Vec::with_capacity(12);
    for ny in y_min..=y_max {
        for nx in x_min..=x_max {
            if nx == cx && ny == cy {
                continue;
            }
            if cfa_color(nx, ny, raw.cfa_pattern()) == center_color {
                let idx = (ny as usize) * (width as usize) + (nx as usize);
                neighbors.push(raw.data[idx]);
            }
        }
    }
    neighbors
}

/// Compute the median of a mutable slice of u16 values.
///
/// Returns 0 if the slice is empty.
fn median(values: &mut [u16]) -> u16 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        // average of two middle values
        let a = values[mid - 1] as u32;
        let b = values[mid] as u32;
        ((a + b) / 2) as u16
    } else {
        values[mid]
    }
}

/// Compute the average of a slice of u16 values.
///
/// Returns 0 if the slice is empty.
fn average(values: &[u16]) -> u16 {
    if values.is_empty() {
        return 0;
    }
    let sum: u64 = values.iter().map(|&v| v as u64).sum();
    (sum / values.len() as u64) as u16
}

/// Detect candidate bad pixels using a threshold relative to local median.
///
/// For each pixel the median of its same-CFA-color neighbors in a 5×5 window
/// is computed. If `|pixel - median| > threshold_factor * median` the pixel is
/// considered bad and its coordinates are appended to the returned list.
///
/// A typical value for `threshold_factor` is `0.5`, meaning the pixel must
/// deviate more than 50% from the local neighborhood median.
///
/// Returns a list of `(x, y)` coordinates of suspected bad pixels.
pub fn detect_bad_pixels(raw: &RawImage, threshold_factor: f32) -> Vec<(u32, u32)> {
    let width = raw.width();
    let height = raw.height();
    let mut bad = Vec::new();

    for y in 0..height {
        for x in 0..width {
            let mut neighbors = collect_same_color_neighbors(raw, x, y);
            if neighbors.is_empty() {
                continue;
            }
            let med = median(&mut neighbors) as f32;
            let pixel = raw.data[(y as usize) * (width as usize) + (x as usize)] as f32;
            if med > 0.0 && (pixel - med).abs() > threshold_factor * med {
                bad.push((x, y));
            }
        }
    }

    bad
}

/// Correct bad pixels in-place using the median replacement strategy.
///
/// For each bad pixel coordinate, replaces the pixel value with the median
/// of same-CFA-color neighbors within a 5×5 window.
pub fn correct_bad_pixels(raw: &mut RawImage, bad_pixels: &[(u32, u32)]) {
    let replacements: Vec<(u32, u32, u16)> = bad_pixels
        .iter()
        .map(|&(x, y)| {
            let mut neighbors = collect_same_color_neighbors(raw, x, y);
            let replacement = median(&mut neighbors);
            (x, y, replacement)
        })
        .collect();

    for (x, y, value) in replacements {
        raw.set_pixel(x, y, value);
    }
}

/// Detect and correct bad pixels in one pass.
///
/// Convenience wrapper that calls [`detect_bad_pixels`] and then either
/// [`correct_bad_pixels`] (median) or an average-based correction depending on
/// `mode`.
pub fn apply_bad_pixel_correction(
    raw: &mut RawImage,
    mode: BadPixelCorrectionMode,
    threshold_factor: f32,
) {
    let bad_pixels = detect_bad_pixels(raw, threshold_factor);

    let replacements: Vec<(u32, u32, u16)> = bad_pixels
        .iter()
        .map(|&(x, y)| {
            let neighbors = collect_same_color_neighbors(raw, x, y);
            let replacement = match mode {
                BadPixelCorrectionMode::Median => {
                    let mut n = neighbors;
                    median(&mut n)
                }
                BadPixelCorrectionMode::Average => average(&neighbors),
            };
            (x, y, replacement)
        })
        .collect();

    for (x, y, value) in replacements {
        raw.set_pixel(x, y, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::image::{Rect, Size};

    /// Build a minimal RawImage filled with a uniform value.
    fn make_raw(width: u32, height: u32, fill: u16) -> RawImage {
        let size = Size::new(width, height);
        let active = Rect::from_coords(0, 0, width, height);
        let mut img = RawImage::new(size, active, 14, CfaPattern::Rggb);
        for v in img.data.iter_mut() {
            *v = fill;
        }
        img
    }

    #[test]
    fn test_no_bad_pixels_uniform() {
        // A perfectly uniform image should have zero bad pixels detected.
        let raw = make_raw(10, 10, 1000);
        let bad = detect_bad_pixels(&raw, 0.5);
        assert!(bad.is_empty(), "expected no bad pixels, got {}", bad.len());
    }

    #[test]
    fn test_single_hot_pixel_detected() {
        // Place one pixel 10× brighter than its neighbors.
        let mut raw = make_raw(10, 10, 1000);
        raw.set_pixel(5, 4, 10000); // hot pixel at (5,4), same color as (5,4) in RGGB = (1%2,0%2)=(1,0) → G_r
        let bad = detect_bad_pixels(&raw, 0.5);
        assert!(
            bad.contains(&(5, 4)),
            "hot pixel at (5,4) should be detected; found: {:?}",
            bad
        );
    }

    #[test]
    fn test_correction_replaces_bad_pixel() {
        // The bad pixel should be replaced by the neighborhood median (≈1000).
        let mut raw = make_raw(10, 10, 1000);
        raw.set_pixel(5, 4, 10000);

        let bad = detect_bad_pixels(&raw, 0.5);
        assert!(bad.contains(&(5, 4)));
        correct_bad_pixels(&mut raw, &bad);

        let corrected = raw.get_pixel(5, 4).unwrap();
        // After correction the value should be close to the neighbor median (1000).
        assert!(
            corrected < 2000,
            "corrected value {} should be near 1000",
            corrected
        );
    }

    #[test]
    fn test_correct_bad_pixels_empty_list() {
        // Passing an empty list must not crash or change any pixels.
        let mut raw = make_raw(8, 8, 500);
        correct_bad_pixels(&mut raw, &[]);
        assert!(raw.data.iter().all(|&v| v == 500));
    }

    #[test]
    fn test_detect_empty_image() {
        // A 2×2 image (minimal RGGB tile) should not crash.
        let raw = make_raw(2, 2, 800);
        let bad = detect_bad_pixels(&raw, 0.5);
        // With only one same-color neighbor in the window the detection may or
        // may not fire, but it must not panic.
        let _ = bad;
    }

    #[test]
    fn test_apply_bad_pixel_correction_average_mode() {
        let mut raw = make_raw(10, 10, 1000);
        raw.set_pixel(5, 4, 10000);
        apply_bad_pixel_correction(&mut raw, BadPixelCorrectionMode::Average, 0.5);
        let corrected = raw.get_pixel(5, 4).unwrap();
        assert!(
            corrected < 2000,
            "average-corrected value {corrected} should be near 1000"
        );
    }

    #[test]
    fn test_cfa_color_rggb() {
        assert_eq!(cfa_color(0, 0, CfaPattern::Rggb), 0); // R
        assert_eq!(cfa_color(1, 0, CfaPattern::Rggb), 1); // G_r
        assert_eq!(cfa_color(0, 1, CfaPattern::Rggb), 3); // G_b
        assert_eq!(cfa_color(1, 1, CfaPattern::Rggb), 2); // B
    }
}
