//! The track model: what a file contains, before any of it is decoded.
//!
//! Every field here comes from the container's headers, so a full track list
//! is available on any target with no decoder at all — see the "Behavior
//! without a hardware decoder" section of `docs/SUPPORT.md`.

use std::fmt;
use std::time::Duration;

use gamut_color::{
    ChromaSubsampling, ColorRange, ColourPrimaries, MatrixCoefficients, TransferCharacteristics,
};
use rawshift_core::metadata::URational;
use rawshift_core::{BitDepth, ColorDescription, Dimensions};

use crate::id::{AudioCodecId, TrackId, TrackKind, VideoCodecId};

/// The codec configuration record a track carries out of band.
///
/// For ISOBMFF this is the `avcC` or `hvcC` box *body* from the sample entry;
/// for Matroska it is `CodecPrivate`, which holds the same records. It carries
/// the parameter sets (SPS/PPS, and VPS for HEVC) and the NAL length-prefix
/// width every packet of the track uses, so a decoder cannot be opened without
/// it.
///
/// Deliberately opaque: the bytes are a codec-defined structure, and rawshift
/// exposes them rather than a parsed form so that the parsed form stays an
/// implementation detail of whichever decoder consumes it.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CodecConfig(Vec<u8>);

impl CodecConfig {
    /// Wrap a configuration record body.
    #[must_use]
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// The record body.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Whether the track carried no configuration record.
    ///
    /// Legal in some containers and for some codecs, but H.264 and HEVC
    /// tracks without one cannot be decoded: there are no parameter sets and
    /// no length-prefix width.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for CodecConfig {
    /// Prints the length rather than the bytes — a configuration record is
    /// tens to hundreds of opaque bytes and dumping it buries every other
    /// field of a `{:?}`-printed track.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CodecConfig({} bytes)", self.0.len())
    }
}

/// Display rotation the container asks for, in degrees clockwise.
///
/// Phones record in the sensor's native orientation and store the correction
/// in the container rather than rotating pixels, so honouring this is not
/// optional: ignoring it shows most phone video sideways. Decoded frames are
/// **not** rotated by rawshift — this is the instruction, applied by whoever
/// presents the frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Rotation {
    /// No rotation.
    #[default]
    None,
    /// 90° clockwise.
    Clockwise90,
    /// 180°.
    Clockwise180,
    /// 270° clockwise (90° counter-clockwise).
    Clockwise270,
}

impl Rotation {
    /// Degrees clockwise: 0, 90, 180, or 270.
    #[must_use]
    pub const fn degrees(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Clockwise90 => 90,
            Self::Clockwise180 => 180,
            Self::Clockwise270 => 270,
        }
    }

    /// Whether applying this rotation swaps width and height.
    #[must_use]
    pub const fn swaps_axes(self) -> bool {
        matches!(self, Self::Clockwise90 | Self::Clockwise270)
    }

    /// Nearest quarter turn to `degrees` clockwise, normalising any multiple
    /// of 360 and any negative value.
    ///
    /// Containers store rotation as a transform matrix rather than an angle,
    /// so a demuxer recovers an angle that may be slightly off or expressed
    /// as a negative; snapping to the nearest quarter turn is the standard
    /// reading and keeps a 89.98°-from-matrix value from becoming `None`.
    #[must_use]
    pub fn from_degrees(degrees: f64) -> Self {
        if !degrees.is_finite() {
            return Self::None;
        }
        // rem_euclid keeps the result non-negative, so -90 becomes 270.
        let normalised = degrees.rem_euclid(360.0);
        match ((normalised / 90.0).round() as i64) % 4 {
            1 => Self::Clockwise90,
            2 => Self::Clockwise180,
            3 => Self::Clockwise270,
            _ => Self::None,
        }
    }

    /// Recover the rotation from an ISOBMFF `tkhd` display matrix.
    ///
    /// The matrix is stored as nine fixed-point values `[a, b, u, c, d, v, x,
    /// y, w]`; only `a`, `b`, `c`, `d` carry rotation. They are 16.16 fixed
    /// point, so callers pass them already converted to `f64`.
    #[must_use]
    pub fn from_display_matrix(a: f64, b: f64, c: f64, d: f64) -> Self {
        // atan2(b, a) is the angle the matrix rotates by, counter-clockwise in
        // the mathematical convention; the container's convention is clockwise.
        // `c` and `d` are accepted so a caller passes the whole rotation
        // sub-matrix, and are used only to reject a degenerate matrix.
        if a == 0.0 && b == 0.0 && c == 0.0 && d == 0.0 {
            return Self::None;
        }
        Self::from_degrees(b.atan2(a).to_degrees())
    }
}

impl fmt::Display for Rotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}°", self.degrees())
    }
}

/// A video track's header-derived description.
/// # Serialization
///
/// Not `Serialize`/`Deserialize` even under the `serde` feature. The track
/// types embed upstream geometry and colour vocabulary (`Dimensions`,
/// `BitDepth`, `ChromaSubsampling`) that gamut does not implement serde for,
/// and skipping those fields would serialize a track that silently lost its
/// resolution and pixel format — worse than not serializing at all. The leaf
/// types here ([`CicpColor`], [`Rotation`], [`CodecConfig`], and everything in
/// [`crate::id`]) do support serde, so a caller can build and serialize its
/// own projection.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct VideoTrack {
    /// Container-assigned identifier.
    pub id: TrackId,
    /// The codec the samples are coded with.
    pub codec: VideoCodecId,
    /// Coded picture size, before [`rotation`](Self::rotation) is applied.
    pub dimensions: Dimensions,
    /// The track's colour signalling.
    pub color: CicpColor,
    /// Bits per sample.
    pub bit_depth: BitDepth,
    /// Chroma subsampling.
    pub chroma: ChromaSubsampling,
    /// Average frame rate, when the container states or implies one.
    ///
    /// Absent for variable-frame-rate recordings that state no average. Not a
    /// substitute for per-packet timestamps, which remain authoritative.
    pub frame_rate: Option<URational>,
    /// The unit of this track's timestamps, in seconds.
    ///
    /// Every `pts`/`dts` on a [`Packet`](crate::packet::Packet) of this track
    /// is a count of these.
    pub time_base: URational,
    /// Track duration, when the container states one.
    pub duration: Option<Duration>,
    /// Number of coded pictures, when the container states one.
    pub frame_count: Option<u64>,
    /// Display rotation the container asks for.
    pub rotation: Rotation,
    /// The out-of-band codec configuration record.
    pub codec_config: CodecConfig,
}

impl VideoTrack {
    /// A video track with `codec`, `dimensions` and `time_base` set and every
    /// other field at its neutral default.
    ///
    /// These types are `#[non_exhaustive]`, so demuxers in other crates build
    /// one through this and then assign the fields their container supplied.
    /// A new field therefore arrives with a default rather than breaking every
    /// demuxer.
    #[must_use]
    pub fn new(
        id: TrackId,
        codec: VideoCodecId,
        dimensions: Dimensions,
        time_base: URational,
    ) -> Self {
        Self {
            id,
            codec,
            dimensions,
            color: CicpColor::UNSPECIFIED,
            bit_depth: BitDepth::Eight,
            chroma: ChromaSubsampling::Cs420,
            frame_rate: None,
            time_base,
            duration: None,
            frame_count: None,
            rotation: Rotation::None,
            codec_config: CodecConfig::default(),
        }
    }
}

/// A track's colour signalling, as the container states it.
///
/// Stored as raw CICP (ITU-T H.273) code points rather than as typed enums,
/// for two reasons: it is exactly what the container carries, and a code point
/// no library names yet survives round-tripping instead of collapsing to
/// "unspecified". The accessors below type them where a type exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CicpColor {
    /// CICP colour primaries code point. 2 means unspecified.
    pub primaries: u16,
    /// CICP transfer characteristics code point. 2 means unspecified.
    pub transfer: u16,
    /// CICP matrix coefficients code point. 2 means unspecified.
    ///
    /// Its own axis, separate from the primaries: this is what maps YCbCr to
    /// RGB, and getting it wrong tints the entire picture.
    pub matrix: u16,
    /// Whether samples occupy the full code range or the studio subset.
    pub full_range: bool,
}

impl CicpColor {
    /// The code points meaning "unspecified" on every axis, studio range —
    /// what an untagged camera file effectively asserts.
    pub const UNSPECIFIED: Self = Self {
        primaries: 2,
        transfer: 2,
        matrix: 2,
        full_range: false,
    };

    /// The typed colour primaries, when the code point is one gamut names.
    #[must_use]
    pub fn primaries(self) -> Option<ColourPrimaries> {
        ColourPrimaries::from_code_point(self.primaries)
    }

    /// The typed transfer characteristics, when the code point is one gamut
    /// names.
    #[must_use]
    pub fn transfer(self) -> Option<TransferCharacteristics> {
        TransferCharacteristics::from_code_point(self.transfer)
    }

    /// The typed matrix coefficients, when the code point is one gamut names.
    #[must_use]
    pub fn matrix(self) -> Option<MatrixCoefficients> {
        MatrixCoefficients::from_code_point(self.matrix)
    }

    /// Whether samples occupy the full code range or the studio subset.
    #[must_use]
    pub fn range(self) -> ColorRange {
        if self.full_range {
            ColorRange::Full
        } else {
            ColorRange::Limited
        }
    }

    /// The primaries-and-transfer pair as rawshift's [`ColorDescription`],
    /// falling back to its `UNSPECIFIED` value for code points gamut does not
    /// name.
    #[must_use]
    pub fn description(self) -> ColorDescription {
        match (self.primaries(), self.transfer()) {
            (Some(primaries), Some(transfer)) => ColorDescription {
                primaries,
                transfer,
            },
            _ => ColorDescription::UNSPECIFIED,
        }
    }

    /// The matrix to actually decode with, resolving "unspecified" against the
    /// coded picture height.
    ///
    /// Containers frequently omit the matrix, and decoders in the field all
    /// fall back the same way: standard definition means BT.601, anything
    /// larger means BT.709. Defaulting to BT.2020 is never right — files that
    /// use it say so, because HDR is unusable if mistagged.
    #[must_use]
    pub fn resolved_matrix(self, coded_height: u32) -> MatrixCoefficients {
        match self.matrix() {
            Some(MatrixCoefficients::Unspecified) | None => {
                if coded_height <= 576 {
                    MatrixCoefficients::Bt601
                } else {
                    MatrixCoefficients::Bt709
                }
            }
            Some(explicit) => explicit,
        }
    }
}

/// An audio track's header-derived description.
///
/// rawshift enumerates audio and yields its packets; it never decodes audio.
/// # Serialization
///
/// Not `Serialize`/`Deserialize` even under the `serde` feature. The track
/// types embed upstream geometry and colour vocabulary (`Dimensions`,
/// `BitDepth`, `ChromaSubsampling`) that gamut does not implement serde for,
/// and skipping those fields would serialize a track that silently lost its
/// resolution and pixel format — worse than not serializing at all. The leaf
/// types here ([`CicpColor`], [`Rotation`], [`CodecConfig`], and everything in
/// [`crate::id`]) do support serde, so a caller can build and serialize its
/// own projection.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AudioTrack {
    /// Container-assigned identifier.
    pub id: TrackId,
    /// The codec the samples are coded with.
    pub codec: AudioCodecId,
    /// Sampling rate in Hz, when stated.
    pub sample_rate: Option<u32>,
    /// Channel count, when stated.
    pub channels: Option<u16>,
    /// The unit of this track's timestamps, in seconds.
    pub time_base: URational,
    /// Track duration, when the container states one.
    pub duration: Option<Duration>,
    /// The out-of-band codec configuration record, when the track has one.
    pub codec_config: CodecConfig,
    /// BCP 47 / ISO 639 language tag, when the container states one.
    pub language: Option<String>,
}

impl AudioTrack {
    /// An audio track with `codec` and `time_base` set and every other field
    /// at its neutral default. See [`VideoTrack::new`].
    #[must_use]
    pub fn new(id: TrackId, codec: AudioCodecId, time_base: URational) -> Self {
        Self {
            id,
            codec,
            sample_rate: None,
            channels: None,
            time_base,
            duration: None,
            codec_config: CodecConfig::default(),
            language: None,
        }
    }
}

/// A track rawshift enumerates but models no further.
/// # Serialization
///
/// Not `Serialize`/`Deserialize` even under the `serde` feature. The track
/// types embed upstream geometry and colour vocabulary (`Dimensions`,
/// `BitDepth`, `ChromaSubsampling`) that gamut does not implement serde for,
/// and skipping those fields would serialize a track that silently lost its
/// resolution and pixel format — worse than not serializing at all. The leaf
/// types here ([`CicpColor`], [`Rotation`], [`CodecConfig`], and everything in
/// [`crate::id`]) do support serde, so a caller can build and serialize its
/// own projection.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OtherTrack {
    /// Container-assigned identifier.
    pub id: TrackId,
    /// What the track carries.
    pub kind: TrackKind,
    /// The unit of this track's timestamps, in seconds.
    pub time_base: URational,
    /// Track duration, when the container states one.
    pub duration: Option<Duration>,
    /// BCP 47 / ISO 639 language tag, when the container states one.
    pub language: Option<String>,
}

impl OtherTrack {
    /// A track of `kind` with `time_base` set and every other field at its
    /// neutral default. See [`VideoTrack::new`].
    #[must_use]
    pub fn new(id: TrackId, kind: TrackKind, time_base: URational) -> Self {
        Self {
            id,
            kind,
            time_base,
            duration: None,
            language: None,
        }
    }
}

/// One track of any kind.
/// # Serialization
///
/// Not `Serialize`/`Deserialize` even under the `serde` feature. The track
/// types embed upstream geometry and colour vocabulary (`Dimensions`,
/// `BitDepth`, `ChromaSubsampling`) that gamut does not implement serde for,
/// and skipping those fields would serialize a track that silently lost its
/// resolution and pixel format — worse than not serializing at all. The leaf
/// types here ([`CicpColor`], [`Rotation`], [`CodecConfig`], and everything in
/// [`crate::id`]) do support serde, so a caller can build and serialize its
/// own projection.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Track {
    /// A video track.
    Video(VideoTrack),
    /// An audio track.
    Audio(AudioTrack),
    /// A subtitle, timecode, or data track.
    Other(OtherTrack),
}

impl Track {
    /// The track's container-assigned identifier.
    #[must_use]
    pub fn id(&self) -> TrackId {
        match self {
            Self::Video(t) => t.id,
            Self::Audio(t) => t.id,
            Self::Other(t) => t.id,
        }
    }

    /// What the track carries.
    #[must_use]
    pub fn kind(&self) -> TrackKind {
        match self {
            Self::Video(_) => TrackKind::Video,
            Self::Audio(_) => TrackKind::Audio,
            Self::Other(t) => t.kind,
        }
    }

    /// The video track, if this is one.
    #[must_use]
    pub fn as_video(&self) -> Option<&VideoTrack> {
        match self {
            Self::Video(t) => Some(t),
            _ => None,
        }
    }

    /// The audio track, if this is one.
    #[must_use]
    pub fn as_audio(&self) -> Option<&AudioTrack> {
        match self {
            Self::Audio(t) => Some(t),
            _ => None,
        }
    }

    /// The unit of this track's timestamps, in seconds.
    #[must_use]
    pub fn time_base(&self) -> URational {
        match self {
            Self::Video(t) => t.time_base,
            Self::Audio(t) => t.time_base,
            Self::Other(t) => t.time_base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_degrees_round_trip() {
        for r in [
            Rotation::None,
            Rotation::Clockwise90,
            Rotation::Clockwise180,
            Rotation::Clockwise270,
        ] {
            assert_eq!(Rotation::from_degrees(f64::from(r.degrees())), r);
        }
    }

    #[test]
    fn rotation_normalises_negative_and_wrapped_angles() {
        assert_eq!(Rotation::from_degrees(-90.0), Rotation::Clockwise270);
        assert_eq!(Rotation::from_degrees(450.0), Rotation::Clockwise90);
        assert_eq!(Rotation::from_degrees(720.0), Rotation::None);
        assert_eq!(Rotation::from_degrees(-360.0), Rotation::None);
    }

    #[test]
    fn rotation_snaps_near_misses_from_fixed_point_matrices() {
        // A 16.16 matrix cannot represent 90° exactly; the recovered angle is
        // a hair off and must not collapse to None.
        assert_eq!(Rotation::from_degrees(89.98), Rotation::Clockwise90);
        assert_eq!(Rotation::from_degrees(270.03), Rotation::Clockwise270);
    }

    #[test]
    fn rotation_rejects_non_finite_angles() {
        assert_eq!(Rotation::from_degrees(f64::NAN), Rotation::None);
        assert_eq!(Rotation::from_degrees(f64::INFINITY), Rotation::None);
    }

    #[test]
    fn display_matrix_recovers_the_quarter_turns_phones_write() {
        // Identity: no rotation.
        assert_eq!(
            Rotation::from_display_matrix(1.0, 0.0, 0.0, 1.0),
            Rotation::None
        );
        // The matrix iOS writes for a portrait capture: 90° clockwise.
        assert_eq!(
            Rotation::from_display_matrix(0.0, 1.0, -1.0, 0.0),
            Rotation::Clockwise90
        );
        // Upside-down landscape.
        assert_eq!(
            Rotation::from_display_matrix(-1.0, 0.0, 0.0, -1.0),
            Rotation::Clockwise180
        );
        // 270°.
        assert_eq!(
            Rotation::from_display_matrix(0.0, -1.0, 1.0, 0.0),
            Rotation::Clockwise270
        );
    }

    #[test]
    fn degenerate_display_matrix_is_not_a_rotation() {
        assert_eq!(
            Rotation::from_display_matrix(0.0, 0.0, 0.0, 0.0),
            Rotation::None
        );
    }

    #[test]
    fn only_quarter_turns_swap_axes() {
        assert!(Rotation::Clockwise90.swaps_axes());
        assert!(Rotation::Clockwise270.swaps_axes());
        assert!(!Rotation::None.swaps_axes());
        assert!(!Rotation::Clockwise180.swaps_axes());
    }

    #[test]
    fn unspecified_matrix_resolves_by_height() {
        let c = CicpColor::UNSPECIFIED;
        // SD heights take BT.601 ...
        assert_eq!(c.resolved_matrix(480), MatrixCoefficients::Bt601);
        assert_eq!(c.resolved_matrix(576), MatrixCoefficients::Bt601);
        // ... anything larger takes BT.709.
        assert_eq!(c.resolved_matrix(720), MatrixCoefficients::Bt709);
        assert_eq!(c.resolved_matrix(2160), MatrixCoefficients::Bt709);
    }

    #[test]
    fn an_unnamed_matrix_code_point_still_resolves_rather_than_panicking() {
        // Code point 7 (SMPTE 240M) is real but not modelled by gamut. It must
        // fall back like "unspecified" rather than being lost or fatal.
        let exotic = CicpColor {
            matrix: 7,
            ..CicpColor::UNSPECIFIED
        };
        assert_eq!(exotic.matrix(), None);
        assert_eq!(exotic.resolved_matrix(1080), MatrixCoefficients::Bt709);
        // ... and the raw code point survives for a caller that does know it.
        assert_eq!(exotic.matrix, 7);
    }

    #[test]
    fn resolving_never_overrides_an_explicit_matrix() {
        // A 4K BT.601 file stays BT.601, and an SD BT.2020 file stays BT.2020.
        let bt601 = CicpColor {
            matrix: MatrixCoefficients::Bt601.code_point(),
            ..CicpColor::UNSPECIFIED
        };
        assert_eq!(bt601.resolved_matrix(2160), MatrixCoefficients::Bt601);

        let bt2020 = CicpColor {
            matrix: MatrixCoefficients::Bt2020Ncl.code_point(),
            ..CicpColor::UNSPECIFIED
        };
        assert_eq!(bt2020.resolved_matrix(480), MatrixCoefficients::Bt2020Ncl);
    }

    #[test]
    fn range_defaults_to_studio_which_is_what_cameras_write() {
        assert_eq!(CicpColor::UNSPECIFIED.range(), ColorRange::Limited);
        assert_eq!(CicpColor::default().range(), ColorRange::Limited);
        let full = CicpColor {
            full_range: true,
            ..CicpColor::UNSPECIFIED
        };
        assert_eq!(full.range(), ColorRange::Full);
    }

    #[test]
    fn typed_accessors_round_trip_named_code_points() {
        let rec709 = CicpColor {
            primaries: ColourPrimaries::Bt709.code_point(),
            transfer: TransferCharacteristics::Bt709.code_point(),
            matrix: MatrixCoefficients::Bt709.code_point(),
            full_range: false,
        };
        assert_eq!(rec709.primaries(), Some(ColourPrimaries::Bt709));
        assert_eq!(rec709.matrix(), Some(MatrixCoefficients::Bt709));
        assert_eq!(rec709.description().primaries, ColourPrimaries::Bt709);
    }

    #[test]
    fn codec_config_debug_reports_length_not_bytes() {
        let cfg = CodecConfig::new(vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(format!("{cfg:?}"), "CodecConfig(4 bytes)");
        assert_eq!(cfg.as_bytes(), &[0xde, 0xad, 0xbe, 0xef]);
        assert!(!cfg.is_empty());
        assert!(CodecConfig::default().is_empty());
    }
}
