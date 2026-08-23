//! Resolving the colour space a container declared for a decoded image.
//!
//! Every standard decoder produces display-referred RGB, but *which* RGB is a
//! property of the source container, not of the decoder. This module turns the
//! two things a container can say — a CICP code-point pair, and/or an embedded
//! ICC profile — into the [`ColorDescription`] tag carried by
//! [`RgbImage`](crate::RgbImage).
//!
//! # Precedence
//!
//! 1. An explicit CICP code-point pair from the container (an AVIF/HEIC `colr`
//!    `nclx` box, a PNG `cICP` chunk) is authoritative and used directly.
//! 2. Otherwise an embedded ICC profile is inspected:
//!    - a `cicp` tag inside the profile (ICC.1:2022 §10.3) is used directly;
//!    - a matrix/TRC profile whose colorants and curves are sRGB resolves to
//!      [`ColorDescription::SRGB`], or to
//!      [`ColorDescription::LINEAR_SRGB`] when its curves are linear;
//!    - anything else resolves to [`ColorDescription::UNSPECIFIED`].
//! 3. With neither, the result is [`ColorDescription::SRGB`] — the documented
//!    default for standard delivery formats.
//!
//! Step 2's final case is deliberate, not a shortfall. [`ColorDescription`] is
//! a CICP pair, and ICC-authoritative spaces such as Adobe RGB and ProPhoto RGB
//! have no faithful CICP expression; inventing a code-point pair for them would
//! misdescribe the samples. `UNSPECIFIED` is what that type documents for
//! exactly this case, and the profile itself is preserved verbatim in
//! [`ImageMetadata::icc_profile`](rawshift_core::metadata::ImageMetadata).

use rawshift_core::{ColorDescription, ColourPrimaries, TransferCharacteristics};

/// The sRGB colorants, chromatically adapted to the ICC PCS illuminant (D50) —
/// the `rXYZ` / `gXYZ` / `bXYZ` triplets a conforming sRGB matrix/TRC profile
/// carries.
const SRGB_COLORANTS_D50: [[f64; 3]; 3] = [
    [0.436_07, 0.222_49, 0.013_92],
    [0.385_15, 0.716_86, 0.096_04],
    [0.143_07, 0.060_61, 0.714_10],
];

/// Tolerance for a colorant match.
///
/// Wide enough to accept every sRGB profile in circulation, which disagree by
/// more than rounding: the ICC's own releases put the green colorant's Z at
/// 0.09708, while the HP/Microsoft "sRGB IEC61966-2.1" profile that most
/// encoders (rawshift's own included) embed puts it at 0.09500. The midpoint
/// above is 0.00104 from each, so 4e-3 clears both with room to spare.
///
/// It stays far from a false positive: the nearest other common space is
/// Display P3, whose red colorant X differs from sRGB's by 0.079 — twenty
/// times this tolerance. Adobe RGB differs by 0.17.
const COLORANT_TOLERANCE: f64 = 4e-3;

/// Tolerance for a sampled or parametric tone curve against the sRGB EOTF.
///
/// sRGB profiles express the curve as the ICC parametric type-3 function or as
/// a sampled 1024- or 4096-entry table, and those disagree by a few parts in a
/// thousand at the sample points. Pure-gamma curves do not go through this
/// path — see [`is_srgb_curve`].
const CURVE_TOLERANCE: f64 = 5e-3;

/// The gamma range accepted as an sRGB approximation.
///
/// A `curveType` with a single entry is a pure power law, which is how most v2
/// profiles approximate sRGB — rawshift's own embedded profile uses the
/// `u8Fixed8` value 0x0238 (2.21875). The true sRGB curve is not a power law,
/// so this is matched by exponent range rather than by sampling: at the toe a
/// 2.2 power law and the sRGB EOTF differ by more than [`CURVE_TOLERANCE`].
const SRGB_GAMMA_RANGE: std::ops::RangeInclusive<f64> = 2.1..=2.3;

/// The gamma range accepted as linear.
const LINEAR_GAMMA_RANGE: std::ops::RangeInclusive<f64> = 0.99..=1.01;

/// Points at which a curve is compared against the sRGB EOTF.
///
/// Both ends are included even though nearly every curve matches there — they
/// cheaply reject a curve that is not normalised — along with four interior
/// points spread across the range.
const CURVE_SAMPLES: [f64; 6] = [0.0, 0.04, 0.25, 0.5, 0.75, 1.0];

/// What a container declared about an image's colour space.
///
/// Both fields record what the *container* said, not what the decoder
/// produced. Build one per decode and pass it to [`resolve_color`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ContainerColor<'a> {
    /// A CICP `(colour_primaries, transfer_characteristics)` pair, when the
    /// container carried one explicitly.
    pub cicp: Option<(u16, u16)>,
    /// The embedded ICC profile, unparsed.
    pub icc: Option<&'a [u8]>,
}

impl<'a> ContainerColor<'a> {
    /// A container that declared nothing about colour.
    pub fn none() -> Self {
        Self::default()
    }

    /// A container that declared only an ICC profile.
    pub fn icc(profile: &'a [u8]) -> Self {
        Self {
            cicp: None,
            icc: Some(profile),
        }
    }

    /// A container that declared only a CICP code-point pair.
    pub fn cicp(primaries: u16, transfer: u16) -> Self {
        Self {
            cicp: Some((primaries, transfer)),
            icc: None,
        }
    }

    /// Attach an ICC profile to an existing declaration.
    #[must_use]
    pub fn with_icc(mut self, profile: Option<&'a [u8]>) -> Self {
        self.icc = profile;
        self
    }

    /// Attach a CICP pair to an existing declaration.
    #[must_use]
    pub fn with_cicp(mut self, cicp: Option<(u16, u16)>) -> Self {
        self.cicp = cicp;
        self
    }
}

/// Resolve the [`ColorDescription`] to tag a decoded image with.
///
/// See the [module documentation](self) for the precedence rules. This never
/// fails: a malformed or unclassifiable ICC profile resolves to
/// [`ColorDescription::UNSPECIFIED`], because a profile rawshift cannot
/// classify is no reason to reject an otherwise good decode.
pub fn resolve_color(declared: ContainerColor<'_>) -> ColorDescription {
    if let Some(color) = declared.cicp.and_then(from_code_points) {
        return color;
    }

    let Some(bytes) = declared.icc else {
        // Nothing declared: standard delivery formats are sRGB by convention.
        return ColorDescription::SRGB;
    };

    let Ok(profile) = gamut_icc::IccProfile::parse(bytes) else {
        return ColorDescription::UNSPECIFIED;
    };

    if let Some(color) = cicp_tag(&profile) {
        return color;
    }
    classify_matrix_trc(&profile).unwrap_or(ColorDescription::UNSPECIFIED)
}

/// Build a `ColorDescription` from a code-point pair, rejecting the pair that
/// says nothing.
///
/// `(2, 2)` is CICP's "unspecified/unspecified", which a container writes when
/// it has no information. Treating that as a declaration would shadow an ICC
/// profile that does carry the answer, so it is rejected and the caller falls
/// through to the profile.
fn from_code_points((primaries, transfer): (u16, u16)) -> Option<ColorDescription> {
    let color = ColorDescription::from_code_points(primaries, transfer)?;
    (color != ColorDescription::UNSPECIFIED).then_some(color)
}

/// Read a `cicp` tag out of a profile, if it carries one.
///
/// ICC.1:2022 §10.3 lets a profile state its colour space as CICP code points
/// directly, which is both cheaper and less ambiguous than inferring it from
/// the colorants.
fn cicp_tag(profile: &gamut_icc::IccProfile) -> Option<ColorDescription> {
    let gamut_icc::TagData::Cicp(cicp) = profile.get(gamut_icc::KnownTag::Cicp)? else {
        return None;
    };
    from_code_points((
        u16::from(cicp.colour_primaries),
        u16::from(cicp.transfer_characteristics),
    ))
}

/// Classify a matrix/TRC profile whose colorants are sRGB's.
///
/// Returns `None` when the profile is not matrix/TRC at all (it is LUT-based,
/// or not RGB), when its colorants are some other space, or when its curves
/// are neither sRGB nor linear.
fn classify_matrix_trc(profile: &gamut_icc::IccProfile) -> Option<ColorDescription> {
    use gamut_icc::KnownTag::{
        BlueColorant, BlueTrc, GreenColorant, GreenTrc, RedColorant, RedTrc,
    };

    let colorants = [
        (RedColorant, SRGB_COLORANTS_D50[0]),
        (GreenColorant, SRGB_COLORANTS_D50[1]),
        (BlueColorant, SRGB_COLORANTS_D50[2]),
    ];
    for (tag, expected) in colorants {
        let gamut_icc::TagData::Xyz(values) = profile.get(tag)? else {
            return None;
        };
        let actual = values.first()?.to_f64();
        if !close(actual, expected, COLORANT_TOLERANCE) {
            return None;
        }
    }

    let curves = [RedTrc, GreenTrc, BlueTrc];
    if curves.iter().all(|&tag| is_srgb_curve(profile, tag)) {
        Some(ColorDescription::SRGB)
    } else if curves.iter().all(|&tag| is_linear_curve(profile, tag)) {
        Some(ColorDescription::LINEAR_SRGB)
    } else {
        None
    }
}

/// Whether one TRC tag is the sRGB transfer curve, or the pure-gamma curve
/// commonly used to approximate it.
fn is_srgb_curve(profile: &gamut_icc::IccProfile, tag: gamut_icc::KnownTag) -> bool {
    match profile.get(tag) {
        // A single-entry `curveType` is a power law. The sRGB EOTF is not one,
        // so match it by exponent rather than by sampling.
        Some(gamut_icc::TagData::Curve(gamut_icc::Curve::Gamma(g))) => {
            SRGB_GAMMA_RANGE.contains(&g.to_f64())
        }
        Some(gamut_icc::TagData::Curve(curve)) => samples_match(|x| curve.eval(x)),
        Some(gamut_icc::TagData::ParametricCurve(curve)) => samples_match(|x| curve.eval(x)),
        _ => false,
    }
}

/// Whether one TRC tag is the identity (a linear transfer).
fn is_linear_curve(profile: &gamut_icc::IccProfile, tag: gamut_icc::KnownTag) -> bool {
    match profile.get(tag) {
        Some(gamut_icc::TagData::Curve(gamut_icc::Curve::Identity)) => true,
        Some(gamut_icc::TagData::Curve(gamut_icc::Curve::Gamma(g))) => {
            LINEAR_GAMMA_RANGE.contains(&g.to_f64())
        }
        _ => false,
    }
}

/// Whether a curve tracks the sRGB EOTF across [`CURVE_SAMPLES`].
fn samples_match(eval: impl Fn(f64) -> f64) -> bool {
    CURVE_SAMPLES
        .into_iter()
        .all(|x| (eval(x) - gamut_color::transfer::srgb_eotf(x)).abs() <= CURVE_TOLERANCE)
}

/// Component-wise comparison within `tolerance`.
fn close(a: [f64; 3], b: [f64; 3], tolerance: f64) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= tolerance)
}

/// Whether rawshift can currently convert samples in `color` to sRGB.
///
/// [`convert_to_srgb`] handles the sRGB and linear transfers on BT.709
/// primaries. Wide-gamut primaries need a colour-management engine, which is
/// not wired into the library yet — see the crate documentation.
///
/// [`convert_to_srgb`]: https://docs.rs/rawshift-image/latest/rawshift_image/transforms/fn.convert_to_srgb.html
pub fn is_convertible_to_srgb(color: ColorDescription) -> bool {
    matches!(
        color.primaries,
        ColourPrimaries::Bt709 | ColourPrimaries::Unspecified
    ) && matches!(
        color.transfer,
        TransferCharacteristics::Srgb
            | TransferCharacteristics::Linear
            | TransferCharacteristics::Unspecified
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamut_icc::{
        ColorSpace, Curve, DeviceClass, IccProfile, ProfileHeader, S15Fixed16, Signature, TagData,
        U8Fixed8, XyzNumber,
    };

    /// Build a matrix/TRC profile from explicit colorants and one shared TRC,
    /// so each test states exactly the profile shape it is about.
    fn matrix_trc_profile(colorants: [[f64; 3]; 3], trc: TagData) -> Vec<u8> {
        let xyz = |v: [f64; 3]| TagData::Xyz(vec![XyzNumber::from_f64(v)]);
        IccProfile {
            header: ProfileHeader::new(DeviceClass::Display, ColorSpace::Rgb),
            tags: vec![
                (Signature(*b"rXYZ"), xyz(colorants[0])),
                (Signature(*b"gXYZ"), xyz(colorants[1])),
                (Signature(*b"bXYZ"), xyz(colorants[2])),
                (Signature(*b"rTRC"), trc.clone()),
                (Signature(*b"gTRC"), trc.clone()),
                (Signature(*b"bTRC"), trc),
            ],
        }
        .to_bytes()
        .expect("hand-built matrix/TRC fixture serialises")
    }

    /// Gamma 2.21875 — the `u8Fixed8` value rawshift's own embedded sRGB
    /// profile uses.
    fn gamma_22() -> TagData {
        TagData::Curve(Curve::Gamma(U8Fixed8(0x0238)))
    }

    /// The true sRGB curve as ICC parametric function type 3.
    fn srgb_parametric() -> TagData {
        let p = |v: f64| S15Fixed16((v * 65536.0).round() as i32);
        TagData::ParametricCurve(gamut_icc::ParametricCurve {
            function_type: 3,
            params: vec![
                p(2.4),
                p(1.0 / 1.055),
                p(0.055 / 1.055),
                p(1.0 / 12.92),
                p(0.04045),
            ],
        })
    }

    /// The HP/Microsoft "sRGB IEC61966-2.1" colorants — the variant rawshift
    /// itself embeds, whose green Z is 0.09500 rather than the ICC's 0.09708.
    const HP_SRGB_COLORANTS: [[f64; 3]; 3] = [
        [0.436_07, 0.222_49, 0.013_92],
        [0.385_15, 0.716_86, 0.095_00],
        [0.143_07, 0.060_61, 0.714_07],
    ];

    /// The ICC's own sRGB colorants.
    const ICC_SRGB_COLORANTS: [[f64; 3]; 3] = [
        [0.436_07, 0.222_49, 0.013_92],
        [0.385_15, 0.716_87, 0.097_08],
        [0.143_07, 0.060_61, 0.714_10],
    ];

    /// Display P3, D50-adapted — the nearest space that must NOT be mistaken
    /// for sRGB.
    const DISPLAY_P3_COLORANTS: [[f64; 3]; 3] = [
        [0.515_12, 0.241_20, -0.001_05],
        [0.291_98, 0.692_24, 0.041_89],
        [0.157_10, 0.066_57, 0.784_07],
    ];

    /// Adobe RGB (1998), D50-adapted.
    const ADOBE_RGB_COLORANTS: [[f64; 3]; 3] = [
        [0.609_74, 0.311_11, 0.019_47],
        [0.205_28, 0.625_67, 0.060_87],
        [0.149_19, 0.063_22, 0.744_57],
    ];

    #[test]
    fn nothing_declared_defaults_to_srgb() {
        assert_eq!(
            resolve_color(ContainerColor::none()),
            ColorDescription::SRGB
        );
    }

    #[test]
    fn explicit_cicp_wins() {
        let (p, t) = ColorDescription::DISPLAY_P3.code_points();
        assert_eq!(
            resolve_color(ContainerColor::cicp(p, t)),
            ColorDescription::DISPLAY_P3
        );
    }

    #[test]
    fn unspecified_cicp_falls_through_to_the_profile() {
        let profile = matrix_trc_profile(ICC_SRGB_COLORANTS, srgb_parametric());
        // (2, 2) is CICP "unspecified": it must not shadow the profile.
        let declared = ContainerColor::cicp(2, 2).with_icc(Some(&profile));
        assert_eq!(resolve_color(declared), ColorDescription::SRGB);
    }

    #[test]
    fn unassigned_cicp_falls_through() {
        // 250 is not an assigned CICP primaries code point.
        assert_eq!(
            resolve_color(ContainerColor::cicp(250, 250)),
            ColorDescription::SRGB
        );
    }

    #[test]
    fn icc_srgb_profile_is_recognised() {
        let profile = matrix_trc_profile(ICC_SRGB_COLORANTS, srgb_parametric());
        assert_eq!(
            resolve_color(ContainerColor::icc(&profile)),
            ColorDescription::SRGB
        );
    }

    /// The regression that motivated [`COLORANT_TOLERANCE`]: rawshift embeds
    /// the HP/Microsoft sRGB variant with a gamma-2.2 curve, so failing to
    /// recognise it would make rawshift's own encode output round-trip as
    /// `UNSPECIFIED`.
    #[test]
    fn rawshift_own_srgb_profile_is_recognised() {
        let profile = matrix_trc_profile(HP_SRGB_COLORANTS, gamma_22());
        assert_eq!(
            resolve_color(ContainerColor::icc(&profile)),
            ColorDescription::SRGB
        );
    }

    #[test]
    fn linear_srgb_profile_is_recognised() {
        let profile = matrix_trc_profile(ICC_SRGB_COLORANTS, TagData::Curve(Curve::Identity));
        assert_eq!(
            resolve_color(ContainerColor::icc(&profile)),
            ColorDescription::LINEAR_SRGB
        );
    }

    #[test]
    fn display_p3_profile_is_not_mistaken_for_srgb() {
        let profile = matrix_trc_profile(DISPLAY_P3_COLORANTS, srgb_parametric());
        assert_eq!(
            resolve_color(ContainerColor::icc(&profile)),
            ColorDescription::UNSPECIFIED
        );
    }

    #[test]
    fn adobe_rgb_profile_is_unspecified() {
        let profile = matrix_trc_profile(ADOBE_RGB_COLORANTS, gamma_22());
        assert_eq!(
            resolve_color(ContainerColor::icc(&profile)),
            ColorDescription::UNSPECIFIED
        );
    }

    #[test]
    fn malformed_profile_is_unspecified_not_an_error() {
        assert_eq!(
            resolve_color(ContainerColor::icc(b"not a profile")),
            ColorDescription::UNSPECIFIED
        );
    }

    #[test]
    fn srgb_and_linear_srgb_are_convertible() {
        assert!(is_convertible_to_srgb(ColorDescription::SRGB));
        assert!(is_convertible_to_srgb(ColorDescription::LINEAR_SRGB));
        assert!(is_convertible_to_srgb(ColorDescription::UNSPECIFIED));
        assert!(!is_convertible_to_srgb(ColorDescription::DISPLAY_P3));
        assert!(!is_convertible_to_srgb(ColorDescription::REC2020));
    }
}
