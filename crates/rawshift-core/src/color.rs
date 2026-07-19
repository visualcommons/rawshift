//! Color descriptors shared across image and video.
//!
//! These are lightweight *tags*, not a color-management engine. A
//! [`ColorDescription`] records *which* space a buffer of samples is in — as a
//! CICP (ITU-T H.273) code-point pair — so callers (and the optional sRGB
//! conversion in `rawshift-image`) can tell whether a transform is needed. The
//! precise source ICC profile, when one exists, is preserved separately as raw
//! bytes in [`ImageMetadata::icc_profile`](crate::metadata::ImageMetadata) and
//! is authoritative for spaces CICP cannot express (e.g. Adobe RGB).
//!
//! [`BitDepth`] is gamut's encode-side depth descriptor, re-exported so the
//! whole workspace shares one vocabulary.

/// Bits per pixel sample of an encoded image (re-exported from `gamut-color`).
///
/// `#[non_exhaustive]` upstream. `Ten` and `Twelve` are honoured by the
/// HDR-capable encoder backends; the 8-bit/16-bit backends reject them as
/// unsupported. Note the sensor-side depth on
/// [`RawImage`](crate::image::RawImage) stays a raw `u8` (12/14-bit sensors
/// have no encode-side equivalent here).
pub use gamut_color::BitDepth;

pub use gamut_color::cicp::{ColourPrimaries, TransferCharacteristics};

/// The color space a set of RGB samples is encoded in, as a CICP pair.
///
/// A coarse, `Copy` tag — deliberately not a full ICC profile. Spaces CICP
/// cannot express (Adobe RGB, ProPhoto RGB) are carried as
/// [`UNSPECIFIED`](Self::UNSPECIFIED) with the ICC profile in
/// [`ImageMetadata::icc_profile`](crate::metadata::ImageMetadata) as the
/// authority — the pair deliberately never lies about what the samples are.
///
/// Both fields are `#[non_exhaustive]` enums upstream, so more code points may
/// appear without a breaking change here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorDescription {
    /// CICP `ColourPrimaries` code point.
    pub primaries: ColourPrimaries,
    /// CICP `TransferCharacteristics` code point.
    pub transfer: TransferCharacteristics,
}

impl ColorDescription {
    /// sRGB: BT.709 primaries + sRGB transfer — standard display RGB.
    pub const SRGB: Self = Self {
        primaries: ColourPrimaries::Bt709,
        transfer: TransferCharacteristics::Srgb,
    };

    /// Linear sRGB: BT.709 primaries + linear transfer.
    ///
    /// The working space of the RAW development pipeline and the documented
    /// input of the encode functions.
    pub const LINEAR_SRGB: Self = Self {
        primaries: ColourPrimaries::Bt709,
        transfer: TransferCharacteristics::Linear,
    };

    /// Display P3: DCI-P3 primaries + sRGB transfer.
    pub const DISPLAY_P3: Self = Self {
        primaries: ColourPrimaries::DisplayP3,
        transfer: TransferCharacteristics::Srgb,
    };

    /// ITU-R BT.2020 primaries, transfer unspecified.
    pub const REC2020: Self = Self {
        primaries: ColourPrimaries::Bt2020,
        transfer: TransferCharacteristics::Unspecified,
    };

    /// The color space could not be determined, or has no CICP expression.
    ///
    /// Conversions treat this as [`SRGB`](Self::SRGB) on a best-effort basis.
    /// ICC-authoritative spaces (Adobe RGB, ProPhoto RGB) are tagged with this
    /// value: their truth lives in the preserved ICC profile, and inventing a
    /// wrong code-point pair for them would misdescribe the samples.
    pub const UNSPECIFIED: Self = Self {
        primaries: ColourPrimaries::Unspecified,
        transfer: TransferCharacteristics::Unspecified,
    };

    /// A short human-readable name for the well-known pairs, e.g. for logging.
    pub fn name(self) -> &'static str {
        match self {
            Self::SRGB => "sRGB",
            Self::LINEAR_SRGB => "Linear sRGB",
            Self::DISPLAY_P3 => "Display P3",
            Self::REC2020 => "Rec. 2020",
            Self::UNSPECIFIED => "Unspecified",
            _ => "CICP",
        }
    }

    /// The CICP code-point pair `(primaries, transfer)`.
    ///
    /// The wire form used by the manual serde implementation and by container
    /// `colr`/nclx boxes.
    pub fn code_points(self) -> (u16, u16) {
        (self.primaries.code_point(), self.transfer.code_point())
    }

    /// Reconstruct from CICP code points, `None` if either point is not
    /// modelled by gamut-color (a later gamut release may turn a `None` into a
    /// `Some`).
    pub fn from_code_points(primaries: u16, transfer: u16) -> Option<Self> {
        Some(Self {
            primaries: ColourPrimaries::from_code_point(primaries)?,
            transfer: TransferCharacteristics::from_code_point(transfer)?,
        })
    }
}

impl Default for ColorDescription {
    /// [`LINEAR_SRGB`](Self::LINEAR_SRGB) — the pipeline working space.
    fn default() -> Self {
        Self::LINEAR_SRGB
    }
}

// Manual serde over the CICP code points: the gamut enums do not (yet) derive
// serde (justin13888/gamut#257); the numeric pair is also the stable wire form,
// so this representation survives that issue landing.
#[cfg(feature = "serde")]
impl serde::Serialize for ColorDescription {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let (primaries, transfer) = self.code_points();
        let mut s = serializer.serialize_struct("ColorDescription", 2)?;
        s.serialize_field("primaries", &primaries)?;
        s.serialize_field("transfer", &transfer)?;
        s.end()
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ColorDescription {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Wire {
            primaries: u16,
            transfer: u16,
        }
        let w = Wire::deserialize(deserializer)?;
        ColorDescription::from_code_points(w.primaries, w.transfer).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "unmodelled CICP code points: primaries {} / transfer {}",
                w.primaries, w.transfer
            ))
        })
    }
}

/// Serde adapter for the re-exported [`BitDepth`], which carries no serde
/// derives upstream (justin13888/gamut#257). Serializes as the bit count.
///
/// Use on struct fields:
/// `#[cfg_attr(feature = "serde", serde(with = "rawshift_core::color::bit_depth_serde"))]`
#[cfg(feature = "serde")]
pub mod bit_depth_serde {
    use super::BitDepth;
    use serde::Deserialize;

    /// Serialize as the number of bits (`8`, `10`, `12`, `16`).
    pub fn serialize<S: serde::Serializer>(v: &BitDepth, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(v.bits())
    }

    /// Deserialize from the number of bits; rejects unmodelled depths.
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BitDepth, D::Error> {
        let bits = u8::deserialize(d)?;
        BitDepth::from_bits(u32::from(bits))
            .ok_or_else(|| serde::de::Error::custom(format!("unsupported bit depth: {bits}")))
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
    }

    #[test]
    fn color_description_defaults_names_and_code_points() {
        assert_eq!(ColorDescription::default(), ColorDescription::LINEAR_SRGB);
        assert_eq!(ColorDescription::SRGB.name(), "sRGB");
        assert_eq!(ColorDescription::LINEAR_SRGB.name(), "Linear sRGB");
        assert_eq!(ColorDescription::UNSPECIFIED.name(), "Unspecified");
        // The pairs are the H.273 code points.
        assert_eq!(ColorDescription::SRGB.code_points(), (1, 13));
        assert_eq!(ColorDescription::LINEAR_SRGB.code_points(), (1, 8));
        assert_eq!(ColorDescription::DISPLAY_P3.code_points(), (12, 13));
        assert_eq!(ColorDescription::REC2020.code_points(), (9, 2));
        assert_eq!(ColorDescription::UNSPECIFIED.code_points(), (2, 2));
    }

    #[test]
    fn color_description_code_point_round_trip() {
        for desc in [
            ColorDescription::SRGB,
            ColorDescription::LINEAR_SRGB,
            ColorDescription::DISPLAY_P3,
            ColorDescription::REC2020,
            ColorDescription::UNSPECIFIED,
        ] {
            let (p, t) = desc.code_points();
            assert_eq!(ColorDescription::from_code_points(p, t), Some(desc));
        }
        // Unmodelled points refuse to construct rather than guessing.
        assert_eq!(ColorDescription::from_code_points(3, 13), None);
        assert_eq!(ColorDescription::from_code_points(1, 3), None);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn color_description_serde_round_trip() {
        let json = serde_json::to_string(&ColorDescription::LINEAR_SRGB).unwrap();
        assert_eq!(json, r#"{"primaries":1,"transfer":8}"#);
        let back: ColorDescription = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ColorDescription::LINEAR_SRGB);
        // Unmodelled code points are a deserialization error, not a guess.
        assert!(
            serde_json::from_str::<ColorDescription>(r#"{"primaries":3,"transfer":13}"#).is_err()
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn bit_depth_serde_round_trip() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Holder {
            #[serde(with = "super::bit_depth_serde")]
            depth: BitDepth,
        }
        let json = serde_json::to_string(&Holder {
            depth: BitDepth::Twelve,
        })
        .unwrap();
        assert_eq!(json, r#"{"depth":12}"#);
        let back: Holder = serde_json::from_str(&json).unwrap();
        assert_eq!(back.depth, BitDepth::Twelve);
        assert!(serde_json::from_str::<Holder>(r#"{"depth":13}"#).is_err());
    }
}
