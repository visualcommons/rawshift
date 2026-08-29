//! Format and codec identity.
//!
//! Every enum here is `#[non_exhaustive]`. rawshift names containers and
//! codecs it does not yet decode, so that "we know what this is and cannot
//! handle it" is expressible — see [`crate::error::VideoError`] — and so that
//! growing support later is additive rather than breaking.

use std::fmt;

/// A container (file) format.
///
/// A variant existing says only that rawshift can *name* the container. Ask
/// the aggregator whether a demuxer for it was compiled in; opening an
/// unsupported one fails with
/// [`VideoError::UnsupportedContainer`](crate::error::VideoError::UnsupportedContainer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ContainerId {
    /// ISO Base Media File Format as MP4 / M4V (`isom`, `mp41`, `mp42`, …).
    Mp4,
    /// ISOBMFF in its QuickTime flavour (`qt  `) — the `.mov` Apple writes.
    Mov,
    /// Matroska.
    Matroska,
    /// WebM, the Matroska subset restricted to royalty-free codecs.
    WebM,
    /// Audio Video Interleave (RIFF). Named, not implemented.
    Avi,
    /// Material Exchange Format, SMPTE 377M. Named, not implemented.
    Mxf,
}

impl ContainerId {
    /// A short, stable, human-readable name (`"MP4"`, `"QuickTime"`, …).
    ///
    /// Stable across releases: it appears in error messages and in
    /// [`CodecId`](rawshift_core::CodecId) strings.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::Mov => "QuickTime",
            Self::Matroska => "Matroska",
            Self::WebM => "WebM",
            Self::Avi => "AVI",
            Self::Mxf => "MXF",
        }
    }

    /// Whether this container is an ISOBMFF flavour, and so shares a box
    /// grammar and a demuxer with the others that are.
    #[must_use]
    pub const fn is_isobmff(self) -> bool {
        matches!(self, Self::Mp4 | Self::Mov)
    }

    /// Whether this container is a Matroska flavour.
    #[must_use]
    pub const fn is_matroska(self) -> bool {
        matches!(self, Self::Matroska | Self::WebM)
    }
}

impl fmt::Display for ContainerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A video codec.
///
/// As with [`ContainerId`], a variant existing does not imply a decoder was
/// compiled in or that a backend can be found at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum VideoCodecId {
    /// H.264 / MPEG-4 AVC.
    H264,
    /// H.265 / HEVC.
    Hevc,
    /// AOMedia Video 1.
    Av1,
    /// VP9.
    Vp9,
    /// Apple ProRes. Named, not implemented — see the crate README.
    ProRes,
    /// A codec rawshift recognises the track as carrying but does not name.
    Other,
}

impl VideoCodecId {
    /// A short, stable, human-readable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::Hevc => "HEVC",
            Self::Av1 => "AV1",
            Self::Vp9 => "VP9",
            Self::ProRes => "ProRes",
            Self::Other => "unknown",
        }
    }
}

impl fmt::Display for VideoCodecId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// An audio codec.
///
/// rawshift **enumerates** audio tracks and hands back their packets; it never
/// decodes audio. This enum exists so a caller can tell what an audio track
/// carries before routing those packets somewhere that does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AudioCodecId {
    /// Advanced Audio Coding.
    Aac,
    /// Linear PCM.
    Pcm,
    /// Opus.
    Opus,
    /// Vorbis.
    Vorbis,
    /// FLAC.
    Flac,
    /// Dolby AC-3 or E-AC-3.
    Ac3,
    /// A codec rawshift does not name.
    Other,
}

impl AudioCodecId {
    /// A short, stable, human-readable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Aac => "AAC",
            Self::Pcm => "PCM",
            Self::Opus => "Opus",
            Self::Vorbis => "Vorbis",
            Self::Flac => "FLAC",
            Self::Ac3 => "AC-3",
            Self::Other => "unknown",
        }
    }
}

impl fmt::Display for AudioCodecId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What kind of media a track carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum TrackKind {
    /// Coded pictures.
    Video,
    /// Coded audio.
    Audio,
    /// Subtitles or captions.
    Subtitle,
    /// SMPTE timecode.
    Timecode,
    /// Anything else the container carries as a track.
    Data,
}

/// A track's identifier within one file.
///
/// Opaque and container-assigned: for ISOBMFF this is the `tkhd` track ID,
/// which is 1-based and need not be contiguous, so never treat it as an index
/// into [`tracks`](crate::demux::Demuxer::tracks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackId(u32);

impl TrackId {
    /// Wrap a container-assigned track identifier.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// The underlying container-assigned value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isobmff_flavours_are_grouped() {
        assert!(ContainerId::Mp4.is_isobmff());
        assert!(ContainerId::Mov.is_isobmff());
        assert!(!ContainerId::Matroska.is_isobmff());
        assert!(!ContainerId::Avi.is_isobmff());
    }

    #[test]
    fn matroska_flavours_are_grouped() {
        assert!(ContainerId::Matroska.is_matroska());
        assert!(ContainerId::WebM.is_matroska());
        assert!(!ContainerId::Mp4.is_matroska());
    }

    #[test]
    fn a_container_is_never_both_families() {
        for c in [
            ContainerId::Mp4,
            ContainerId::Mov,
            ContainerId::Matroska,
            ContainerId::WebM,
            ContainerId::Avi,
            ContainerId::Mxf,
        ] {
            assert!(
                !(c.is_isobmff() && c.is_matroska()),
                "{c} claims two container families"
            );
        }
    }

    #[test]
    fn track_id_round_trips_and_displays() {
        let id = TrackId::new(7);
        assert_eq!(id.get(), 7);
        assert_eq!(id.to_string(), "7");
    }

    #[test]
    fn names_are_stable_and_non_empty() {
        assert_eq!(ContainerId::Mov.name(), "QuickTime");
        assert_eq!(VideoCodecId::Hevc.name(), "HEVC");
        assert_eq!(AudioCodecId::Aac.name(), "AAC");
        assert!(!VideoCodecId::Other.name().is_empty());
    }
}
