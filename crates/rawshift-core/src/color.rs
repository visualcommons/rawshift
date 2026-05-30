//! Color space and bit-depth descriptors shared across image and video.
//!
//! These are lightweight *tags*, not a color-management engine. A [`ColorSpace`]
//! records *which* space a buffer of samples is in, so callers (and the optional
//! sRGB conversion in `rawshift-image`) can tell whether a transform is needed.
//! The precise source ICC profile, when one exists, is preserved separately as
//! raw bytes in [`ImageMetadata::icc_profile`](crate::metadata::ImageMetadata).

use crate::image::white_level_from_bit_depth;

/// The color space a set of RGB samples is encoded in.
///
/// A coarse, `Copy` tag — deliberately not a full ICC profile. It is
/// `#[non_exhaustive]`: more spaces may be added without a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ColorSpace {
    /// sRGB primaries with the sRGB transfer function — standard display RGB.
    Srgb,
    /// sRGB primaries with a linear transfer function.
    ///
    /// The working space of the RAW development pipeline and the documented
    /// input of the encode functions.
    #[default]
    LinearSrgb,
    /// Display P3: DCI-P3 primaries with the sRGB transfer function.
    DisplayP3,
    /// ITU-R BT.2020 primaries.
    Rec2020,
    /// Adobe RGB (1998).
    AdobeRgb,
    /// ROMM RGB / ProPhoto RGB (wide gamut).
    ProPhotoRgb,
    /// The color space could not be determined.
    ///
    /// Conversions treat this as [`Srgb`](Self::Srgb) on a best-effort basis.
    Unknown,
}

impl ColorSpace {
    /// A short human-readable name, e.g. for logging or UI pickers.
    pub fn name(self) -> &'static str {
        match self {
            ColorSpace::Srgb => "sRGB",
            ColorSpace::LinearSrgb => "Linear sRGB",
            ColorSpace::DisplayP3 => "Display P3",
            ColorSpace::Rec2020 => "Rec. 2020",
            ColorSpace::AdobeRgb => "Adobe RGB",
            ColorSpace::ProPhotoRgb => "ProPhoto RGB",
            ColorSpace::Unknown => "Unknown",
        }
    }
}

/// Bits per pixel sample of an encoded image.
///
/// `#[non_exhaustive]`: further variants can be added without a breaking change.
/// `Ten` and `Twelve` are honoured by the HDR-capable encoder backends (e.g.
/// libaom AVIF); the 8-bit/16-bit backends reject them as unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum BitDepth {
    /// 8 bits per sample.
    Eight,
    /// 10 bits per sample. High-bit-depth output (e.g. AV1/AVIF).
    Ten,
    /// 12 bits per sample. High-bit-depth output (e.g. AV1/AVIF).
    Twelve,
    /// 16 bits per sample.
    #[default]
    Sixteen,
}

impl BitDepth {
    /// Number of bits per sample.
    pub fn bits(self) -> u8 {
        match self {
            BitDepth::Eight => 8,
            BitDepth::Ten => 10,
            BitDepth::Twelve => 12,
            BitDepth::Sixteen => 16,
        }
    }

    /// Maximum representable sample value, clamped to `u16`.
    pub fn max_value(self) -> u16 {
        white_level_from_bit_depth(self.bits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_depth_bits_and_max() {
        assert_eq!(BitDepth::Eight.bits(), 8);
        assert_eq!(BitDepth::Ten.bits(), 10);
        assert_eq!(BitDepth::Twelve.bits(), 12);
        assert_eq!(BitDepth::Sixteen.bits(), 16);
        assert_eq!(BitDepth::Eight.max_value(), 255);
        assert_eq!(BitDepth::Ten.max_value(), 1023);
        assert_eq!(BitDepth::Twelve.max_value(), 4095);
        assert_eq!(BitDepth::Sixteen.max_value(), u16::MAX);
        assert_eq!(BitDepth::default(), BitDepth::Sixteen);
    }

    #[test]
    fn color_space_defaults_and_names() {
        assert_eq!(ColorSpace::default(), ColorSpace::LinearSrgb);
        assert_eq!(ColorSpace::Srgb.name(), "sRGB");
        assert_eq!(ColorSpace::Unknown.name(), "Unknown");
    }
}
