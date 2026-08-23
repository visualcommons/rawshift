//! Turning decoded images into textures the window can draw.
//!
//! Kept apart from [`crate::color`] so the colour decisions stay testable
//! without `iced` in scope: this module only downsizes and wraps.

use iced::widget::image::Handle;
use rawshift_image::core::RgbImage;
use rawshift_image::core::metadata::ImageMetadata;

use crate::color::{self, ColorReport};

/// The longest edge, in pixels, a preview texture is reduced to.
///
/// The colour transform is scalar `f64` per pixel, so a 60-megapixel RAW would
/// spend seconds in `Pipeline::eval` at full size for a thumbnail nobody can
/// see the detail of. Reducing first bounds that work; the numeric comparison
/// in [`crate::pipeline`] always runs on the full-resolution buffers, so
/// nothing that is *measured* is measured on a downscaled image.
pub const PREVIEW_MAX_EDGE: u32 = 512;

/// A texture plus the account of how its colour was handled.
pub struct Preview {
    pub handle: Handle,
    pub report: ColorReport,
    /// The full-resolution dimensions, which the preview may not have.
    pub source_size: (u32, u32),
}

/// Build a colour-managed preview texture.
pub fn preview(image: &RgbImage, metadata: &ImageMetadata) -> Preview {
    let source_size = (image.width(), image.height());
    let reduced = downsample(image, PREVIEW_MAX_EDGE);
    let subject = reduced.as_ref().unwrap_or(image);
    let (rgba, report) = color::to_display_rgba(subject, metadata);
    Preview {
        handle: Handle::from_rgba(subject.width(), subject.height(), rgba),
        report,
        source_size,
    }
}

/// Nearest-neighbour reduction to fit `max_edge`, or `None` when the image
/// already fits.
///
/// Nearest-neighbour rather than a filtered resample on purpose: this is a
/// codec demonstration, and a smoothing filter would hide exactly the blocking
/// and ringing artefacts a lossy encoder produces, which is the thing worth
/// looking at.
fn downsample(image: &RgbImage, max_edge: u32) -> Option<RgbImage> {
    let (width, height) = (image.width(), image.height());
    let longest = width.max(height);
    if longest <= max_edge {
        return None;
    }

    // Integer step keeps the mapping exact and cheap.
    let step = longest.div_ceil(max_edge);
    let (new_width, new_height) = (width.div_ceil(step), height.div_ceil(step));
    let src = image.data();
    let mut out = Vec::with_capacity((new_width * new_height * 3) as usize);
    for y in 0..new_height {
        let sy = (y * step).min(height - 1);
        for x in 0..new_width {
            let sx = (x * step).min(width - 1);
            let i = ((sy * width + sx) * 3) as usize;
            out.extend_from_slice(&src[i..i + 3]);
        }
    }
    // The arithmetic above guarantees the invariant; a failure here would be a
    // bug in this function rather than bad input, so fall back to the original
    // rather than losing the image.
    RgbImage::with_color(new_width, new_height, out, image.color()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawshift_core::ColorDescription;

    fn image(width: u32, height: u32) -> RgbImage {
        RgbImage::with_color(
            width,
            height,
            vec![0x8000; (width * height * 3) as usize],
            ColorDescription::SRGB,
        )
        .expect("valid buffer")
    }

    #[test]
    fn small_images_are_not_resampled() {
        assert!(downsample(&image(64, 64), PREVIEW_MAX_EDGE).is_none());
    }

    #[test]
    fn large_images_fit_within_the_preview_bound() {
        let reduced = downsample(&image(2000, 1000), 512).expect("resampled");
        assert!(reduced.width().max(reduced.height()) <= 512);
        assert_eq!(
            reduced.data().len(),
            (reduced.width() * reduced.height() * 3) as usize
        );
    }

    #[test]
    fn downsampling_preserves_the_colour_tag() {
        let source = RgbImage::with_color(
            1000,
            1000,
            vec![0u16; 1000 * 1000 * 3],
            ColorDescription::DISPLAY_P3,
        )
        .expect("valid buffer");
        let reduced = downsample(&source, 100).expect("resampled");
        assert_eq!(
            reduced.color(),
            ColorDescription::DISPLAY_P3,
            "dropping the tag here would make the preview silently mis-managed"
        );
    }

    #[test]
    fn non_square_images_keep_both_edges_bounded() {
        let reduced = downsample(&image(4000, 37), 512).expect("resampled");
        assert!(reduced.width() <= 512);
        assert!(reduced.height() >= 1);
    }

    #[test]
    fn preview_reports_the_full_resolution_size() {
        let preview = preview(&image(1024, 768), &ImageMetadata::default());
        assert_eq!(preview.source_size, (1024, 768));
    }
}
