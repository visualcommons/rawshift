use super::{Demosaic, DemosaicError};
use crate::core::image::{CfaPattern, RawImage};
use rayon::prelude::*;

/// Bilinear interpolation demosaicing algorithm.
///
/// A fast, simple demosaicing algorithm that interpolates missing color
/// values using the average of neighboring pixels. This produces acceptable
/// results for most images but may show color fringing on high-contrast edges.
///
/// This implementation uses rayon for parallel row processing on multi-core systems.
pub struct Bilinear;

impl Demosaic for Bilinear {
    fn demosaic_into(&self, raw: &RawImage, output: &mut [u16]) -> Result<(), DemosaicError> {
        let width = raw.active_area().size.width;
        let height = raw.active_area().size.height;
        let x_offset = raw.active_area().origin.x;
        let y_offset = raw.active_area().origin.y;

        let expected_size = (width as usize) * (height as usize) * 3;
        if output.len() != expected_size {
            return Err(DemosaicError::BufferSizeMismatch {
                expected: expected_size,
                actual: output.len(),
            });
        }

        let raw_width = raw.width();
        let raw_height = raw.height();
        let raw_data = &raw.data;
        let cfa_pattern = raw.cfa_pattern();
        let row_stride = (width as usize) * 3;

        // Get raw pixel with bounds checking (closure captures raw data)
        let get_raw = |x: u32, y: u32| -> u16 {
            if x < raw_width && y < raw_height {
                raw_data[(y as usize) * (raw_width as usize) + (x as usize)]
            } else {
                0
            }
        };

        // Process rows in parallel
        output
            .par_chunks_mut(row_stride)
            .enumerate()
            .for_each(|(y, row_output)| {
                let abs_y = (y as u32) + y_offset;

                for x in 0..width {
                    let abs_x = x + x_offset;
                    let (r, g, b) = demosaic_pixel_bilinear(abs_x, abs_y, cfa_pattern, &get_raw);

                    let idx = (x as usize) * 3;
                    row_output[idx] = r;
                    row_output[idx + 1] = g;
                    row_output[idx + 2] = b;
                }
            });

        Ok(())
    }
}

/// Interpolates a single pixel using bilinear interpolation.
/// `get_raw`: (x: u32, y: u32) -> <raw pixel value>: u16
fn demosaic_pixel_bilinear<F>(x: u32, y: u32, pattern: CfaPattern, get_raw: &F) -> (u16, u16, u16)
where
    F: Fn(u32, u32) -> u16,
{
    // Determine the color of the current pixel location
    let (is_red, is_blue) = match pattern {
        CfaPattern::Rggb => ((x % 2 == 0) && (y % 2 == 0), (x % 2 == 1) && (y % 2 == 1)),
        CfaPattern::Grbg => ((x % 2 == 1) && (y % 2 == 0), (x % 2 == 0) && (y % 2 == 1)),
        CfaPattern::Gbrg => ((x % 2 == 0) && (y % 2 == 1), (x % 2 == 1) && (y % 2 == 0)),
        CfaPattern::Bggr => ((x % 2 == 1) && (y % 2 == 1), (x % 2 == 0) && (y % 2 == 0)),
    };

    // Green is true if it's neither red nor blue
    let is_green = !is_red && !is_blue;

    if is_green {
        // Current pixel is Green.
        // We need to interpolate Red and Blue.
        // One of them will be vertical neighbors, the other horizontal neighbors.

        let g = get_raw(x, y);

        let (r, b) = match pattern {
            // For RGGB:
            // R G R G
            // G B G B
            // If we are at (0,1) [Green], Left/Right is B, Up/Down is R
            // If we are at (1,0) [Green], Left/Right is R, Up/Down is B
            CfaPattern::Rggb | CfaPattern::Bggr => {
                if y % 2 == 0 {
                    // Even row
                    // R G R G (RGGB case) -> (x%2==1) -> Horizontal is R, Vertical is B
                    // B G B G (BGGR case) -> (x%2==1) -> Horizontal is B, Vertical is R
                    // checking pattern specifically:
                    if matches!(pattern, CfaPattern::Rggb) {
                        // Row 0: R G R G. We are at G. Neighbors (left/right) are R. Vertical are B.
                        (
                            avg2(get_raw(x.saturating_sub(1), y), get_raw(x + 1, y)),
                            avg2(get_raw(x, y.saturating_sub(1)), get_raw(x, y + 1)),
                        )
                    } else {
                        // BGGR
                        // Row 0: B G B G. We are at G. Neighbors (left/right) are B. Vertical are R.
                        (
                            avg2(get_raw(x, y.saturating_sub(1)), get_raw(x, y + 1)),
                            avg2(get_raw(x.saturating_sub(1), y), get_raw(x + 1, y)),
                        )
                    }
                } else {
                    // Odd row
                    // G B G B (RGGB case). We are at G. Left/Right B, Vert R.
                    // G R G R (BGGR case). We are at G. Left/Right R, Vert B.
                    if matches!(pattern, CfaPattern::Rggb) {
                        (
                            avg2(get_raw(x, y.saturating_sub(1)), get_raw(x, y + 1)),
                            avg2(get_raw(x.saturating_sub(1), y), get_raw(x + 1, y)),
                        )
                    } else {
                        (
                            avg2(get_raw(x.saturating_sub(1), y), get_raw(x + 1, y)),
                            avg2(get_raw(x, y.saturating_sub(1)), get_raw(x, y + 1)),
                        )
                    }
                }
            }
            CfaPattern::Grbg | CfaPattern::Gbrg => {
                // Similar logic...
                // GRBG:
                // G R G R
                // B G B G
                if y % 2 == 0 {
                    // Even row G R G R. At G. Horizontal R, Vert B.
                    if matches!(pattern, CfaPattern::Grbg) {
                        (
                            avg2(get_raw(x.saturating_sub(1), y), get_raw(x + 1, y)),
                            avg2(get_raw(x, y.saturating_sub(1)), get_raw(x, y + 1)),
                        )
                    } else {
                        // GBRG: G B G B. At G. Horizontal B, Vert R.
                        (
                            avg2(get_raw(x, y.saturating_sub(1)), get_raw(x, y + 1)),
                            avg2(get_raw(x.saturating_sub(1), y), get_raw(x + 1, y)),
                        )
                    }
                } else {
                    // Odd row
                    // GRBG: B G B G. At G. Horizontal B, Vert R.
                    if matches!(pattern, CfaPattern::Grbg) {
                        (
                            avg2(get_raw(x, y.saturating_sub(1)), get_raw(x, y + 1)),
                            avg2(get_raw(x.saturating_sub(1), y), get_raw(x + 1, y)),
                        )
                    } else {
                        // GBRG: R G R G. At G. Horizontal R, Vert B.
                        (
                            avg2(get_raw(x.saturating_sub(1), y), get_raw(x + 1, y)),
                            avg2(get_raw(x, y.saturating_sub(1)), get_raw(x, y + 1)),
                        )
                    }
                }
            }
        };
        (r, g, b)
    } else if is_red {
        // Current is Red.
        let r = get_raw(x, y);
        // Green is average of 4 cross neighbors (up, down, left, right)
        let g = avg4(
            get_raw(x, y.saturating_sub(1)),
            get_raw(x, y + 1),
            get_raw(x.saturating_sub(1), y),
            get_raw(x + 1, y),
        );
        // Blue is average of 4 diagonal neighbors
        let b = avg4(
            get_raw(x.saturating_sub(1), y.saturating_sub(1)),
            get_raw(x + 1, y.saturating_sub(1)),
            get_raw(x.saturating_sub(1), y + 1),
            get_raw(x + 1, y + 1),
        );
        (r, g, b)
    } else {
        // Current is Blue.
        let b = get_raw(x, y);
        // Green is average of 4 cross neighbors
        let g = avg4(
            get_raw(x, y.saturating_sub(1)),
            get_raw(x, y + 1),
            get_raw(x.saturating_sub(1), y),
            get_raw(x + 1, y),
        );
        // Red is average of 4 diagonal neighbors
        let r = avg4(
            get_raw(x.saturating_sub(1), y.saturating_sub(1)),
            get_raw(x + 1, y.saturating_sub(1)),
            get_raw(x.saturating_sub(1), y + 1),
            get_raw(x + 1, y + 1),
        );
        (r, g, b)
    }
}

#[inline(always)]
fn avg2(a: u16, b: u16) -> u16 {
    ((a as u32 + b as u32) / 2) as u16
}

#[inline(always)]
fn avg4(a: u16, b: u16, c: u16, d: u16) -> u16 {
    ((a as u32 + b as u32 + c as u32 + d as u32) / 4) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::image::{Point, Rect, Size};

    /// Create a test raw image with given dimensions and CFA pattern.
    fn create_test_raw(width: u32, height: u32, pattern: CfaPattern, value: u16) -> RawImage {
        let size = Size::new(width, height);
        let active_area = Rect::new(Point::ORIGIN, size);
        let pixel_count = (width * height) as usize;
        RawImage::builder(size, active_area, 14, pattern)
            .white_level(16383)
            .data(vec![value; pixel_count])
            .build()
    }

    /// Create a raw image with a 2x2 Bayer pattern.
    fn create_bayer_2x2(pattern: CfaPattern) -> RawImage {
        // Create a 2x2 pattern with distinct values for each cell
        let mut raw = create_test_raw(2, 2, pattern, 0);
        // Set values for each pixel position
        raw.data[0] = 1000; // (0, 0)
        raw.data[1] = 2000; // (1, 0)
        raw.data[2] = 3000; // (0, 1)
        raw.data[3] = 4000; // (1, 1)
        raw
    }

    #[test]
    fn test_demosaic_into_correct_size() {
        let raw = create_test_raw(10, 10, CfaPattern::Rggb, 1000);
        let mut output = vec![0u16; 10 * 10 * 3]; // Correct size

        let result = Bilinear.demosaic_into(&raw, &mut output);
        assert!(result.is_ok());
    }

    #[test]
    fn test_demosaic_into_wrong_size() {
        let raw = create_test_raw(10, 10, CfaPattern::Rggb, 1000);
        let mut output = vec![0u16; 50]; // Too small

        let result = Bilinear.demosaic_into(&raw, &mut output);
        assert!(matches!(
            result,
            Err(DemosaicError::BufferSizeMismatch { .. })
        ));
    }

    #[test]
    fn test_demosaic_solid_color() {
        // A uniform input should produce roughly uniform output
        let raw = create_test_raw(10, 10, CfaPattern::Rggb, 5000);
        let demosaic = Bilinear;
        let rgb = demosaic.demosaic(&raw);

        assert_eq!(rgb.width(), 10);
        assert_eq!(rgb.height(), 10);
        assert_eq!(rgb.data.len(), 10 * 10 * 3);

        // Interior pixels (not on edge) should be close to input value
        // Edge pixels may have lower values due to boundary handling
        for y in 1..9 {
            for x in 1..9 {
                let idx = ((y * 10 + x) * 3) as usize;
                for c in 0..3 {
                    let pixel = rgb.data[idx + c];
                    assert!(
                        (4000..=5500).contains(&pixel),
                        "Interior pixel at ({},{}) channel {} value {} out of range",
                        x,
                        y,
                        c,
                        pixel
                    );
                }
            }
        }
    }

    #[test]
    fn test_demosaic_all_cfa_patterns() {
        let patterns = [
            CfaPattern::Rggb,
            CfaPattern::Grbg,
            CfaPattern::Gbrg,
            CfaPattern::Bggr,
        ];

        for pattern in patterns {
            let raw = create_test_raw(8, 8, pattern, 2000);
            let rgb = Bilinear.demosaic(&raw);

            assert_eq!(rgb.width(), 8);
            assert_eq!(rgb.height(), 8);
            assert_eq!(rgb.data.len(), 8 * 8 * 3);

            // Verify all pixels have reasonable values
            for pixel in &rgb.data {
                assert!(
                    *pixel <= 3000,
                    "Pattern {:?}: pixel value {} too high",
                    pattern,
                    pixel
                );
            }
        }
    }

    #[test]
    fn test_demosaic_rggb_pattern() {
        // Test that RGGB pattern is correctly interpreted
        let raw = create_bayer_2x2(CfaPattern::Rggb);
        let rgb = Bilinear.demosaic(&raw);

        // For RGGB, position (0,0) is Red
        // The output should have interpolated values
        assert_eq!(rgb.width(), 2);
        assert_eq!(rgb.height(), 2);

        // First pixel (0,0) - this is a Red position in RGGB
        let r = rgb.data[0];
        let g = rgb.data[1];
        let _b = rgb.data[2];

        // Red should be the original value (1000)
        assert_eq!(r, 1000, "Red at RGGB position (0,0) should be 1000");

        // Green should be interpolated from neighbors
        // At (0,0), only (1,0) and (0,1) are in bounds, both at edges
        assert!(g > 0, "Green should be interpolated");
    }

    #[test]
    fn test_demosaic_with_active_area() {
        // Test that active_area is respected
        let raw = {
            let size = Size::new(10, 10);
            let active_area = Rect::from_coords(3, 3, 4, 4);
            RawImage::builder(size, active_area, 14, CfaPattern::Rggb)
                .white_level(16383)
                .data(vec![1000u16; 100])
                .build()
        };

        let rgb = Bilinear.demosaic(&raw);

        // Output dimensions should match active area
        assert_eq!(rgb.width(), 4);
        assert_eq!(rgb.height(), 4);
        assert_eq!(rgb.data.len(), 4 * 4 * 3);
    }

    #[test]
    fn test_avg2() {
        assert_eq!(avg2(0, 0), 0);
        assert_eq!(avg2(100, 100), 100);
        assert_eq!(avg2(100, 200), 150);
        assert_eq!(avg2(0, 65535), 32767);
    }

    #[test]
    fn test_avg4() {
        assert_eq!(avg4(0, 0, 0, 0), 0);
        assert_eq!(avg4(100, 100, 100, 100), 100);
        assert_eq!(avg4(0, 100, 200, 300), 150);
        assert_eq!(avg4(0, 0, 0, 65535), 16383);
    }

    #[test]
    fn test_bilinear_all_cfa_patterns() {
        // All 4 CFA patterns should produce valid RGB output with correct dimensions
        let patterns = [
            CfaPattern::Rggb,
            CfaPattern::Grbg,
            CfaPattern::Gbrg,
            CfaPattern::Bggr,
        ];

        for pattern in patterns {
            let raw = create_test_raw(6, 6, pattern, 8000);
            let rgb = Bilinear.demosaic(&raw);

            assert_eq!(rgb.width(), 6, "width for {:?}", pattern);
            assert_eq!(rgb.height(), 6, "height for {:?}", pattern);
            assert_eq!(rgb.data.len(), 6 * 6 * 3, "data length for {:?}", pattern);

            // All output pixels must be in valid u16 range (which they always are,
            // but also check that at least some pixels are non-zero for a non-zero input)
            let non_zero = rgb.data.iter().any(|&v| v > 0);
            assert!(
                non_zero,
                "Output for {:?} should have non-zero pixels",
                pattern
            );
        }
    }

    #[test]
    fn test_bilinear_with_active_area() {
        // Test various active area offsets
        let raw = {
            let size = Size::new(12, 12);
            let active_area = Rect::from_coords(2, 4, 6, 6);
            RawImage::builder(size, active_area, 14, CfaPattern::Rggb)
                .white_level(16383)
                .data(vec![5000u16; 144])
                .build()
        };

        let rgb = Bilinear.demosaic(&raw);

        assert_eq!(
            rgb.width(),
            6,
            "output width should match active area width"
        );
        assert_eq!(
            rgb.height(),
            6,
            "output height should match active area height"
        );
        assert_eq!(
            rgb.data.len(),
            6 * 6 * 3,
            "output should have correct data length"
        );

        // Output should have reasonable values
        for &v in &rgb.data {
            assert!(v <= 65535, "pixel value out of range");
        }
    }

    #[test]
    fn test_bilinear_gradient_smooth() {
        // A horizontally-varying input should produce a smooth (monotonically varying)
        // green channel in the output, since bilinear averages neighbors.
        let width = 8u32;
        let height = 4u32;
        let size = Size::new(width, height);
        let active_area = Rect::new(Point::ORIGIN, size);

        // Fill with a horizontal gradient: pixel value increases with x
        let mut data = vec![0u16; (width * height) as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                data[y * width as usize + x] = (x as u16) * 1000;
            }
        }

        let raw = RawImage::builder(size, active_area, 14, CfaPattern::Rggb)
            .white_level(16383)
            .data(data)
            .build();

        let rgb = Bilinear.demosaic(&raw);

        // Check that the green channel (index 1) generally increases left to right
        // for a middle row. Allow for some non-monotonicity at edges.
        let mid_row = 2usize;
        let row_start = mid_row * width as usize * 3;

        // Compare interior pixels: pixel at x+1 should have green >= pixel at x
        // (with tolerance for boundary effects)
        for x in 1..(width as usize - 2) {
            let g_left = rgb.data[row_start + (x - 1) * 3 + 1];
            let g_right = rgb.data[row_start + (x + 1) * 3 + 1];
            assert!(
                g_right >= g_left || (g_right as i32 - g_left as i32).abs() < 2000,
                "gradient smoothness: g[{}]={} should not greatly exceed g[{}]={} in row {}",
                x - 1,
                g_left,
                x + 1,
                g_right,
                mid_row
            );
        }
    }
}
