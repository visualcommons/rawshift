//! Colour management for display.
//!
//! Two layers of the problem, addressed separately:
//!
//! **Container level.** What did the file actually say its colour space was?
//! rawshift answers this — [`RgbImage::color`] carries the resolved
//! [`ColorDescription`] and the raw profile is in
//! [`ImageMetadata::icc_profile`]. [`describe`] renders both.
//!
//! **Display level.** Getting the samples into the viewer's space. rawshift's
//! own `transforms::convert_to_srgb` handles only the sRGB and linear
//! transfers on BT.709 primaries and errors on anything wider, so the gallery
//! goes through `gamut-cmm` — the ICC colour-management module — for a real
//! transform.
//!
//! # The two gamut trees
//!
//! `gamut-cmm` is a git dependency (see this crate's `Cargo.toml`), so it
//! links its *own* copies of `gamut-core` / `gamut-color` / `gamut-icc` from
//! the git checkout. Those are a different Cargo source from the crates.io
//! copies rawshift uses, which makes `gamut_icc::IccProfile` two unrelated
//! types. ICC data therefore crosses the boundary as **bytes**: the profile in
//! `ImageMetadata::icc_profile` is re-parsed here with the git `gamut-icc`
//! (aliased `gamut_icc_git`) before it reaches `gamut-cmm`.
//!
//! # Cost
//!
//! `Pipeline::eval` is scalar `f64` over interleaved samples, so it runs once
//! per image when the display buffer is built, via rayon — never per frame.

use gamut_icc_git::{IccProfile, RenderingIntent};
use rawshift_core::ColorDescription;
use rawshift_image::core::RgbImage;
use rawshift_image::core::metadata::ImageMetadata;
use rayon::prelude::*;

/// How an image's colour was determined, and what was done to display it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorReport {
    /// The tag rawshift resolved from the container.
    pub tag: ColorDescription,
    /// The size of the embedded ICC profile, when the container carried one.
    pub icc_bytes: Option<usize>,
    /// What was actually applied to the samples.
    pub transform: Transform,
    /// Set when the display path could not honour the source colour and fell
    /// back to treating it as sRGB.
    pub warning: Option<String>,
}

/// The transform applied on the way to the display buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    /// Already display sRGB; only bit-depth reduction ran.
    None,
    /// The sRGB opto-electronic transfer function (a linear source).
    SrgbEncode,
    /// A full ICC transform through gamut-cmm, source profile → sRGB.
    Icc,
}

impl Transform {
    pub fn label(self) -> &'static str {
        match self {
            Transform::None => "none (already sRGB)",
            Transform::SrgbEncode => "sRGB OETF (linear source)",
            Transform::Icc => "ICC to sRGB (gamut-cmm)",
        }
    }
}

/// Convert a decoded image to an 8-bit RGBA buffer in display sRGB.
///
/// Returns the buffer alongside a [`ColorReport`] describing what was done, so
/// the UI can state the provenance rather than silently producing pixels.
///
/// Never fails. A profile gamut-cmm cannot link — LUT-based ones, since its
/// phase P5 is unimplemented — falls back to treating the samples as sRGB and
/// records a warning: drawing the image with a caveat beats refusing to draw
/// it.
pub fn to_display_rgba(image: &RgbImage, metadata: &ImageMetadata) -> (Vec<u8>, ColorReport) {
    let tag = image.color();
    let icc = metadata.icc_profile.as_deref();
    let mut warning = None;

    // An ICC-authoritative source: the profile *is* the colour space, so a
    // real transform is both possible and necessary.
    if tag == ColorDescription::UNSPECIFIED
        && let Some(bytes) = icc
    {
        match icc_to_srgb(image, bytes) {
            Ok(rgba) => {
                return (
                    rgba,
                    ColorReport {
                        tag,
                        icc_bytes: Some(bytes.len()),
                        transform: Transform::Icc,
                        warning: None,
                    },
                );
            }
            Err(e) => warning = Some(format!("ICC transform unavailable ({e}); shown as sRGB")),
        }
    }

    // Wide-gamut code points with no profile to link. gamut-cmm needs a
    // profile, and synthesising one from CICP primaries needs a chromaticity
    // accessor gamut-color does not expose yet.
    if warning.is_none() && !rawshift_image::core::is_convertible_to_srgb(tag) {
        warning = Some(format!(
            "{} carries no embedded profile to transform from; shown as sRGB",
            tag.name()
        ));
    }

    let transform = if tag == ColorDescription::LINEAR_SRGB {
        Transform::SrgbEncode
    } else {
        Transform::None
    };
    let rgba = match transform {
        Transform::SrgbEncode => encode_linear(image),
        _ => pack_rgba(image),
    };
    (
        rgba,
        ColorReport {
            tag,
            icc_bytes: icc.map(<[u8]>::len),
            transform,
            warning,
        },
    )
}

/// Transform through gamut-cmm: source profile → PCS → sRGB.
fn icc_to_srgb(image: &RgbImage, profile_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let source =
        IccProfile::parse(profile_bytes).map_err(|e| format!("unparsable profile: {e}"))?;
    let destination =
        IccProfile::parse(&srgb_profile_bytes()).map_err(|e| format!("sRGB profile: {e}"))?;

    let intent = RenderingIntent::MediaRelativeColorimetric;
    let to_pcs = gamut_cmm::link::device_to_pcs(&source, intent).map_err(|e| e.to_string())?;
    let from_pcs =
        gamut_cmm::link::pcs_to_device(&destination, intent).map_err(|e| e.to_string())?;
    let pipeline = to_pcs.compose(from_pcs).map_err(|e| e.to_string())?;

    if pipeline.input_channels() != 3 || pipeline.output_channels() != 3 {
        return Err(format!(
            "expected a 3-to-3 transform, got {}-to-{}",
            pipeline.input_channels(),
            pipeline.output_channels()
        ));
    }

    let scale = f64::from(u16::MAX);
    let rgba = image
        .data()
        .par_chunks_exact(3)
        .flat_map_iter(|px| {
            let input = [
                f64::from(px[0]) / scale,
                f64::from(px[1]) / scale,
                f64::from(px[2]) / scale,
            ];
            let mut output = [0.0f64; 3];
            // A per-pixel failure cannot be reported from inside the map, and
            // a pipeline that evaluates for one pixel evaluates for all: the
            // only failure modes are channel-count and buffer-length
            // mismatches, both checked once above. Passing the input through
            // on the impossible branch keeps the image readable.
            if pipeline.eval(&input, &mut output).is_err() {
                output = input;
            }
            [
                to_u8(output[0]),
                to_u8(output[1]),
                to_u8(output[2]),
                u8::MAX,
            ]
        })
        .collect();
    Ok(rgba)
}

/// The sRGB profile to convert *to*.
///
/// Built with the git `gamut-icc` so it is the type gamut-cmm expects.
/// Deliberately not borrowed from rawshift's `IccProfile::srgb()`: those bytes
/// come from the crates.io copy, and constructing locally keeps the two trees
/// visibly separate.
fn srgb_profile_bytes() -> Vec<u8> {
    use gamut_icc_git::{
        ColorSpace, Curve, DeviceClass, ProfileHeader, Signature, TagData, U8Fixed8, XyzNumber,
    };

    let xyz = |v: [f64; 3]| TagData::Xyz(vec![XyzNumber::from_f64(v)]);
    // Gamma 2.2, matching the approximation rawshift itself embeds.
    let trc = TagData::Curve(Curve::Gamma(U8Fixed8(0x0238)));
    IccProfile {
        header: ProfileHeader::new(DeviceClass::Display, ColorSpace::Rgb),
        tags: vec![
            (Signature(*b"rXYZ"), xyz([0.436_07, 0.222_49, 0.013_92])),
            (Signature(*b"gXYZ"), xyz([0.385_15, 0.716_86, 0.095_00])),
            (Signature(*b"bXYZ"), xyz([0.143_07, 0.060_61, 0.714_07])),
            (Signature(*b"rTRC"), trc.clone()),
            (Signature(*b"gTRC"), trc.clone()),
            (Signature(*b"bTRC"), trc),
        ],
    }
    .to_bytes()
    .expect("the static sRGB profile serialises")
}

/// Apply the sRGB OETF to linear samples on the way to 8 bits.
fn encode_linear(image: &RgbImage) -> Vec<u8> {
    let scale = f64::from(u16::MAX);
    image
        .data()
        .par_chunks_exact(3)
        .flat_map_iter(|px| {
            let encode = |v: u16| to_u8(gamut_color::transfer::srgb_oetf(f64::from(v) / scale));
            [encode(px[0]), encode(px[1]), encode(px[2]), u8::MAX]
        })
        .collect()
}

/// Pack 16-bit RGB into 8-bit RGBA, taking the high byte of each sample.
fn pack_rgba(image: &RgbImage) -> Vec<u8> {
    image
        .data()
        .par_chunks_exact(3)
        .flat_map_iter(|px| {
            [
                (px[0] >> 8) as u8,
                (px[1] >> 8) as u8,
                (px[2] >> 8) as u8,
                u8::MAX,
            ]
        })
        .collect()
}

/// Clamp and quantise a `[0, 1]` value to 8 bits.
fn to_u8(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A human-readable account of an image's colour, for the detail panel.
pub fn describe(report: &ColorReport) -> Vec<(String, String)> {
    let (primaries, transfer) = report.tag.code_points();
    let mut rows = vec![
        ("rawshift tag".to_owned(), report.tag.name().to_owned()),
        (
            "CICP code points".to_owned(),
            format!("primaries {primaries}, transfer {transfer}"),
        ),
        (
            "embedded ICC".to_owned(),
            match report.icc_bytes {
                Some(n) => format!("{n} bytes"),
                None => "none".to_owned(),
            },
        ),
        (
            "display transform".to_owned(),
            report.transform.label().to_owned(),
        ),
    ];
    if let Some(warning) = &report.warning {
        rows.push(("warning".to_owned(), warning.clone()));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(fill: u16, color: ColorDescription) -> RgbImage {
        RgbImage::with_color(2, 2, vec![fill; 2 * 2 * 3], color).expect("valid buffer")
    }

    fn profile(colorants: [[f64; 3]; 3]) -> Vec<u8> {
        use gamut_icc_git::{
            ColorSpace, Curve, DeviceClass, ProfileHeader, Signature, TagData, U8Fixed8, XyzNumber,
        };
        let xyz = |v: [f64; 3]| TagData::Xyz(vec![XyzNumber::from_f64(v)]);
        let trc = TagData::Curve(Curve::Gamma(U8Fixed8(0x0238)));
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
        .expect("fixture serialises")
    }

    /// Adobe RGB (1998), D50-adapted.
    const ADOBE_RGB: [[f64; 3]; 3] = [
        [0.609_74, 0.311_11, 0.019_47],
        [0.205_28, 0.625_67, 0.060_87],
        [0.149_19, 0.063_22, 0.744_57],
    ];

    #[test]
    fn srgb_source_needs_no_transform() {
        let (rgba, report) = to_display_rgba(
            &image(0xFFFF, ColorDescription::SRGB),
            &ImageMetadata::default(),
        );
        assert_eq!(report.transform, Transform::None);
        assert!(report.warning.is_none());
        assert_eq!(rgba.len(), 2 * 2 * 4);
        assert_eq!(&rgba[..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn linear_source_gets_the_srgb_oetf() {
        // Linear 0.5 encodes to roughly 0.74 — that gap is the whole point.
        let (rgba, report) = to_display_rgba(
            &image(0x8000, ColorDescription::LINEAR_SRGB),
            &ImageMetadata::default(),
        );
        assert_eq!(report.transform, Transform::SrgbEncode);
        assert!(
            rgba[0] > 180,
            "linear 0.5 should encode well above 128, got {}",
            rgba[0]
        );
    }

    #[test]
    fn wide_gamut_without_a_profile_warns_rather_than_failing() {
        let (rgba, report) = to_display_rgba(
            &image(0x8000, ColorDescription::DISPLAY_P3),
            &ImageMetadata::default(),
        );
        assert_eq!(rgba.len(), 2 * 2 * 4, "the image is still drawn");
        let warning = report.warning.expect("a caveat must be recorded");
        assert!(warning.contains("Display P3"), "got: {warning}");
    }

    /// The load-bearing case: an ICC-authoritative source is transformed
    /// through gamut-cmm rather than shown as-is.
    #[test]
    fn icc_authoritative_source_goes_through_gamut_cmm() {
        let metadata = ImageMetadata {
            icc_profile: Some(profile(ADOBE_RGB)),
            ..ImageMetadata::default()
        };
        let (rgba, report) =
            to_display_rgba(&image(0x8000, ColorDescription::UNSPECIFIED), &metadata);
        assert_eq!(
            report.transform,
            Transform::Icc,
            "warning was: {:?}",
            report.warning
        );
        assert!(report.warning.is_none());
        assert_eq!(rgba.len(), 2 * 2 * 4);
        // Adobe RGB's wider primaries put a mid grey at a different sRGB value
        // than a straight pass-through would.
        assert_ne!(
            rgba[0], 0x80,
            "a real transform must change the sample, not pass it through"
        );
    }

    #[test]
    fn unparsable_profile_falls_back_with_a_warning() {
        let metadata = ImageMetadata {
            icc_profile: Some(b"not a profile".to_vec()),
            ..ImageMetadata::default()
        };
        let (rgba, report) =
            to_display_rgba(&image(0x8000, ColorDescription::UNSPECIFIED), &metadata);
        assert_eq!(rgba.len(), 2 * 2 * 4);
        assert_eq!(report.transform, Transform::None);
        assert!(report.warning.is_some());
    }

    #[test]
    fn the_destination_srgb_profile_links() {
        let bytes = srgb_profile_bytes();
        let parsed = IccProfile::parse(&bytes).expect("parses");
        gamut_cmm::link::pcs_to_device(&parsed, RenderingIntent::MediaRelativeColorimetric)
            .expect("the destination profile must link, or nothing can be displayed");
    }

    #[test]
    fn describe_reports_the_warning_when_there_is_one() {
        let (_, report) = to_display_rgba(
            &image(0, ColorDescription::REC2020),
            &ImageMetadata::default(),
        );
        let rows = describe(&report);
        assert!(rows.iter().any(|(k, _)| k == "warning"));
        assert!(
            rows.iter()
                .any(|(k, v)| k == "rawshift tag" && v == "Rec. 2020")
        );
    }
}
