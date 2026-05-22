//! Orientation and crop transforms for RGB images.
//!
//! Applies EXIF orientation tags and rectangular crop regions to
//! fully demosaiced RGB images.

use crate::core::image::{Rect, RgbImage, Size};

/// Apply EXIF orientation transform to correct image display.
///
/// TIFF orientation values encode how the stored image should be rotated/flipped
/// to produce the correct upright display. This function applies the corresponding
/// pixel transform so the output is always orientation-1 (Normal/upright).
///
/// Orientation values:
/// - 1: Normal (no transform)
/// - 2: Mirror horizontal
/// - 3: Rotate 180°
/// - 4: Mirror vertical
/// - 5: Transpose (mirror horizontal + rotate 90° CCW)
/// - 6: Rotate 90° CW
/// - 7: Transverse (mirror horizontal + rotate 90° CW)
/// - 8: Rotate 90° CCW
pub fn apply_orientation(image: &mut RgbImage, orientation: u16) {
    match orientation {
        1 => {} // No transform needed
        2 => flip_horizontal(image),
        3 => rotate_180(image),
        4 => flip_vertical(image),
        5 => {
            // Transpose = flip horizontal then rotate 90° CCW
            flip_horizontal(image);
            rotate_90_ccw(image);
        }
        6 => rotate_90_cw(image),
        7 => {
            // Transverse = flip horizontal then rotate 90° CW
            flip_horizontal(image);
            rotate_90_cw(image);
        }
        8 => rotate_90_ccw(image),
        _ => tracing::warn!(
            "Unknown orientation value: {}, skipping transform",
            orientation
        ),
    }
}

/// Mirror image horizontally (left ↔ right).
pub fn flip_horizontal(image: &mut RgbImage) {
    let w = image.width() as usize;
    let h = image.height() as usize;
    for row in 0..h {
        for col in 0..w / 2 {
            let a = (row * w + col) * 3;
            let b = (row * w + (w - 1 - col)) * 3;
            image.data.swap(a, b);
            image.data.swap(a + 1, b + 1);
            image.data.swap(a + 2, b + 2);
        }
    }
}

/// Mirror image vertically (top ↔ bottom).
pub fn flip_vertical(image: &mut RgbImage) {
    let w = image.width() as usize;
    let h = image.height() as usize;
    for row in 0..h / 2 {
        for col in 0..w {
            let a = (row * w + col) * 3;
            let b = ((h - 1 - row) * w + col) * 3;
            image.data.swap(a, b);
            image.data.swap(a + 1, b + 1);
            image.data.swap(a + 2, b + 2);
        }
    }
}

/// Rotate image 180°.
pub fn rotate_180(image: &mut RgbImage) {
    let n = image.data.len();
    let mut i = 0;
    let mut j = n - 3;
    while i < j {
        image.data.swap(i, j);
        image.data.swap(i + 1, j + 1);
        image.data.swap(i + 2, j + 2);
        i += 3;
        j -= 3;
    }
}

/// Rotate image 90° clockwise.
///
/// New dimensions: `new_width = old_height`, `new_height = old_width`.
pub fn rotate_90_cw(image: &mut RgbImage) {
    let old_w = image.width() as usize;
    let old_h = image.height() as usize;
    let new_w = old_h;
    let new_h = old_w;
    let mut new_data = vec![0u16; new_w * new_h * 3];
    for old_row in 0..old_h {
        for old_col in 0..old_w {
            let new_row = old_col;
            let new_col = old_h - 1 - old_row;
            let src = (old_row * old_w + old_col) * 3;
            let dst = (new_row * new_w + new_col) * 3;
            new_data[dst] = image.data[src];
            new_data[dst + 1] = image.data[src + 1];
            new_data[dst + 2] = image.data[src + 2];
        }
    }
    image.data = new_data;
    image.set_size(Size::new(new_w as u32, new_h as u32));
}

/// Rotate image 90° counter-clockwise.
///
/// New dimensions: `new_width = old_height`, `new_height = old_width`.
pub fn rotate_90_ccw(image: &mut RgbImage) {
    let old_w = image.width() as usize;
    let old_h = image.height() as usize;
    let new_w = old_h;
    let new_h = old_w;
    let mut new_data = vec![0u16; new_w * new_h * 3];
    for old_row in 0..old_h {
        for old_col in 0..old_w {
            let new_row = old_w - 1 - old_col;
            let new_col = old_row;
            let src = (old_row * old_w + old_col) * 3;
            let dst = (new_row * new_w + new_col) * 3;
            new_data[dst] = image.data[src];
            new_data[dst + 1] = image.data[src + 1];
            new_data[dst + 2] = image.data[src + 2];
        }
    }
    image.data = new_data;
    image.set_size(Size::new(new_w as u32, new_h as u32));
}

/// Crop an RGB image to the given rectangle.
///
/// If the crop region extends beyond image bounds, a warning is logged and
/// the image is left unchanged.
pub fn apply_crop(image: &mut RgbImage, crop: Rect) {
    let x = crop.origin.x as usize;
    let y = crop.origin.y as usize;
    let w = crop.size.width as usize;
    let h = crop.size.height as usize;

    if x + w <= image.width() as usize && y + h <= image.height() as usize {
        let img_width = image.width() as usize;
        let mut new_data = Vec::with_capacity(w * h * 3);
        for row in 0..h {
            let src_base = ((y + row) * img_width + x) * 3;
            new_data.extend_from_slice(&image.data[src_base..src_base + w * 3]);
        }
        image.set_size(Size::new(w as u32, h as u32));
        image.data = new_data;
    } else {
        tracing::warn!(
            "Crop region out of bounds: {:?} vs {}x{}",
            crop,
            image.width(),
            image.height()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::image::{Point, Size};

    fn make_image(w: u32, h: u32, data: Vec<u16>) -> RgbImage {
        RgbImage::new(w, h, data)
    }

    #[test]
    fn test_flip_horizontal_2x2() {
        // 2x2 image: [R0,G0,B0, R1,G1,B1,  R2,G2,B2, R3,G3,B3]
        let mut img = make_image(2, 2, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        flip_horizontal(&mut img);
        assert_eq!(img.data, vec![4, 5, 6, 1, 2, 3, 10, 11, 12, 7, 8, 9]);
    }

    #[test]
    fn test_flip_vertical_2x2() {
        let mut img = make_image(2, 2, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        flip_vertical(&mut img);
        assert_eq!(img.data, vec![7, 8, 9, 10, 11, 12, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_rotate_180_2x2() {
        let mut img = make_image(2, 2, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        rotate_180(&mut img);
        assert_eq!(img.data, vec![10, 11, 12, 7, 8, 9, 4, 5, 6, 1, 2, 3]);
    }

    #[test]
    fn test_rotate_90_cw_2x2() {
        // Original:  [A B]    After 90 CW:  [C A]
        //            [C D]                   [D B]
        let mut img = make_image(2, 2, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        rotate_90_cw(&mut img);
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
        assert_eq!(img.data, vec![7, 8, 9, 1, 2, 3, 10, 11, 12, 4, 5, 6]);
    }

    #[test]
    fn test_rotate_90_ccw_2x2() {
        // Original:  [A B]    After 90 CCW: [B D]
        //            [C D]                   [A C]
        let mut img = make_image(2, 2, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        rotate_90_ccw(&mut img);
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
        assert_eq!(img.data, vec![4, 5, 6, 10, 11, 12, 1, 2, 3, 7, 8, 9]);
    }

    #[test]
    fn test_rotate_90_cw_non_square() {
        // 3x2 → 2x3
        let mut img = make_image(
            3,
            2,
            vec![1, 0, 0, 2, 0, 0, 3, 0, 0, 4, 0, 0, 5, 0, 0, 6, 0, 0],
        );
        rotate_90_cw(&mut img);
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 3);
    }

    #[test]
    fn test_apply_orientation_identity() {
        let mut img = make_image(2, 2, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        let original = img.data.clone();
        apply_orientation(&mut img, 1);
        assert_eq!(img.data, original);
    }

    #[test]
    fn test_crop_basic() {
        // 4x4 image, crop to 2x2 at (1,1)
        let mut data = Vec::with_capacity(4 * 4 * 3);
        for i in 0..16 {
            data.push(i as u16);
            data.push(0);
            data.push(0);
        }
        let mut img = make_image(4, 4, data);
        apply_crop(&mut img, Rect::new(Point::new(1, 1), Size::new(2, 2)));
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
        // Row 1 of original: pixels 4,5,6,7 → crop cols 1..3 → pixels 5,6
        assert_eq!(img.data[0], 5); // pixel(1,1).r
        assert_eq!(img.data[3], 6); // pixel(2,1).r
    }

    #[test]
    fn test_crop_out_of_bounds() {
        let mut img = make_image(4, 4, vec![0u16; 4 * 4 * 3]);
        let original_size = img.size();
        apply_crop(&mut img, Rect::new(Point::new(3, 3), Size::new(2, 2)));
        // Should be unchanged
        assert_eq!(img.size(), original_size);
    }
}
