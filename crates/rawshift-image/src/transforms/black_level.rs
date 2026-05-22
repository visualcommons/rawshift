//! Black level subtraction for raw sensor data.
//!
//! Subtracts the per-channel black level (pedestal) from raw CFA data.
//! Black levels are stored per 2×2 Bayer position in [`RawImage::black_levels`].

use crate::core::image::RawImage;

/// Subtract per-channel black levels from raw CFA data in place.
///
/// Each pixel's black level is determined by its position in the 2×2 Bayer
/// pattern: `black_levels[(y % 2) * 2 + (x % 2)]`.
///
/// This is a no-op if all black levels are zero.
pub fn apply_black_level(raw: &mut RawImage) {
    let bl = *raw.black_levels();
    if bl[0] == 0 && bl[1] == 0 && bl[2] == 0 && bl[3] == 0 {
        return;
    }

    let width = raw.width() as usize;
    for (i, pixel) in raw.data.iter_mut().enumerate() {
        let x = i % width;
        let y = i / width;
        let bl_idx = (y % 2) * 2 + (x % 2);
        *pixel = pixel.saturating_sub(bl[bl_idx]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::image::{CfaPattern, Rect, Size};

    fn make_raw(width: u32, height: u32, data: Vec<u16>, black_levels: [u16; 4]) -> RawImage {
        let size = Size::new(width, height);
        let active = Rect::from_coords(0, 0, width, height);
        let mut raw = RawImage::new(size, active, 14, CfaPattern::Rggb);
        raw.data = data;
        raw.set_black_levels(black_levels);
        raw
    }

    #[test]
    fn no_op_when_all_zero() {
        let original = vec![100, 200, 300, 400];
        let mut raw = make_raw(2, 2, original.clone(), [0, 0, 0, 0]);
        apply_black_level(&mut raw);
        assert_eq!(raw.data, original);
    }

    #[test]
    fn uniform_black_level() {
        let mut raw = make_raw(2, 2, vec![100, 200, 300, 400], [50, 50, 50, 50]);
        apply_black_level(&mut raw);
        assert_eq!(raw.data, vec![50, 150, 250, 350]);
    }

    #[test]
    fn per_channel_subtraction() {
        // 2×2 image, each pixel at a different Bayer position
        // bl_idx mapping: (0,0)=0, (1,0)=1, (0,1)=2, (1,1)=3
        let mut raw = make_raw(2, 2, vec![1000, 1000, 1000, 1000], [10, 20, 30, 40]);
        apply_black_level(&mut raw);
        assert_eq!(raw.data, vec![990, 980, 970, 960]);
    }

    #[test]
    fn saturating_subtraction_does_not_underflow() {
        let mut raw = make_raw(2, 1, vec![5, 10], [100, 100, 100, 100]);
        apply_black_level(&mut raw);
        assert_eq!(raw.data, vec![0, 0]);
    }

    #[test]
    fn test_black_level_all_channels() {
        // 2x2 image: each pixel at a distinct CFA position
        // bl_idx: (0,0)->0, (1,0)->1, (0,1)->2, (1,1)->3
        let data = vec![1000u16, 2000, 3000, 4000];
        let black_levels = [100u16, 200, 300, 400];
        let mut raw = make_raw(2, 2, data, black_levels);
        apply_black_level(&mut raw);
        assert_eq!(raw.data[0], 900, "(0,0) R channel");
        assert_eq!(raw.data[1], 1800, "(1,0) G channel");
        assert_eq!(raw.data[2], 2700, "(0,1) G channel");
        assert_eq!(raw.data[3], 3600, "(1,1) B channel");
    }

    #[test]
    fn test_black_level_clamps_at_zero() {
        // Pixel value less than or equal to black level should become 0
        let data = vec![50u16, 100, 150, 200];
        let black_levels = [100u16, 100, 100, 100];
        let mut raw = make_raw(2, 2, data, black_levels);
        apply_black_level(&mut raw);
        assert_eq!(raw.data[0], 0, "50 - 100 should clamp to 0");
        assert_eq!(raw.data[1], 0, "100 - 100 should equal 0");
        assert_eq!(raw.data[2], 50, "150 - 100 should be 50");
        assert_eq!(raw.data[3], 100, "200 - 100 should be 100");
    }

    #[test]
    fn test_black_level_preserves_above_white() {
        // Values above the typical white level are unchanged by black level subtraction
        // (black_level subtraction doesn't clip to white level)
        let data = vec![65000u16, 65535, 50000, 40000];
        let black_levels = [100u16, 100, 100, 100];
        let mut raw = make_raw(2, 2, data, black_levels);
        apply_black_level(&mut raw);
        assert_eq!(
            raw.data[0], 64900,
            "high value should have black subtracted"
        );
        assert_eq!(raw.data[1], 65435, "max value minus black level");
        assert_eq!(raw.data[2], 49900);
        assert_eq!(raw.data[3], 39900);
    }
}
