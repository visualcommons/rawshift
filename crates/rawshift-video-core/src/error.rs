//! The error type every rawshift video crate returns.
//!
//! Mirrors `rawshift-image-core`'s `RawError` in shape and in one rule: a
//! backend's own error type never crosses a rawshift boundary. Demuxer and
//! decoder backends are stringified into [`VideoError::Container`] or
//! [`VideoError::Decode`] at the leaf crate that owns them, so swapping a
//! backend is not a breaking change for callers.

use std::io;

use thiserror::Error;

use crate::id::{ContainerId, TrackId, VideoCodecId};

/// Anything that can go wrong reading or decoding a video file.
///
/// `#[non_exhaustive]`: new containers and codecs bring new failure modes, and
/// that must not be a breaking change. Match with a `_` arm.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VideoError {
    /// The underlying reader failed.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// The container is malformed, truncated, or uses a structure the demuxer
    /// does not implement.
    ///
    /// `container` names the demuxer that rejected the file so the message can
    /// be attributed; `message` is the backend's own text, flattened to a
    /// string precisely so the backend's error type stays private.
    #[error("{container} container error: {message}")]
    Container {
        /// The demuxer that produced this error.
        container: &'static str,
        /// Backend-supplied detail.
        message: String,
    },

    /// The bytes are not a container rawshift can open.
    ///
    /// `detected` is `Some` when rawshift recognised the format but has no
    /// demuxer compiled in for it (AVI and MXF are named but unimplemented;
    /// MP4 and Matroska are feature-gated), and `None` when nothing matched.
    #[error("unsupported container: {}", match .detected {
        Some(c) => c.name(),
        None => "unrecognised",
    })]
    UnsupportedContainer {
        /// The container, when it was recognised.
        detected: Option<ContainerId>,
    },

    /// The track's codec has no decoder in this build.
    ///
    /// Distinct from [`HwDecoderUnavailable`](Self::HwDecoderUnavailable):
    /// this one means no decoder for the codec was compiled in at all, not
    /// that a compiled decoder found no backend at runtime.
    #[error("unsupported codec: {codec}")]
    UnsupportedCodec {
        /// The codec the track carries.
        codec: VideoCodecId,
    },

    /// A decoder was found, and decoding failed.
    #[error("{codec} decode failed: {message}")]
    Decode {
        /// The codec being decoded.
        codec: VideoCodecId,
        /// Backend-supplied detail.
        message: String,
    },

    /// A decoder for this codec exists in the build, but no hardware backend
    /// could be found at runtime.
    ///
    /// This is the expected outcome on every target with no hardware decode
    /// API — `wasm32`, `musl`, Windows — and on Linux hosts with no libva or
    /// no capable driver. It is deliberately matchable so a caller can fall
    /// back or degrade rather than treat it as corruption. Container parsing,
    /// track enumeration, and metadata all still work: see `docs/SUPPORT.md`.
    #[error("no hardware decoder available for {codec}: {reason}")]
    HwDecoderUnavailable {
        /// The codec no backend was found for.
        codec: VideoCodecId,
        /// Why — no backend compiled in, no device, or no driver support.
        reason: String,
    },

    /// The file has no track with the requested identifier.
    #[error("no such track: {id}")]
    NoSuchTrack {
        /// The identifier that did not match a track.
        id: TrackId,
    },

    /// Container-level metadata was present but could not be interpreted.
    ///
    /// Metadata failures are never fatal to opening a file — a file with
    /// unreadable metadata still demuxes — so this surfaces only from the
    /// explicit metadata entry points.
    #[error("metadata error: {message}")]
    Metadata {
        /// What could not be interpreted.
        message: String,
    },
}

impl VideoError {
    /// Build a [`VideoError::Container`] from a backend error.
    ///
    /// The one place demuxer backends are flattened, so the rule "no backend
    /// type crosses a public boundary" has a single obvious implementation.
    pub fn container(container: &'static str, message: impl std::fmt::Display) -> Self {
        Self::Container {
            container,
            message: message.to_string(),
        }
    }

    /// Build a [`VideoError::Decode`] from a backend error.
    pub fn decode(codec: VideoCodecId, message: impl std::fmt::Display) -> Self {
        Self::Decode {
            codec,
            message: message.to_string(),
        }
    }

    /// Build a [`VideoError::HwDecoderUnavailable`].
    pub fn hw_unavailable(codec: VideoCodecId, reason: impl std::fmt::Display) -> Self {
        Self::HwDecoderUnavailable {
            codec,
            reason: reason.to_string(),
        }
    }

    /// Whether this error means "this build or machine cannot decode it",
    /// rather than "this file is broken".
    ///
    /// True for [`UnsupportedContainer`](Self::UnsupportedContainer),
    /// [`UnsupportedCodec`](Self::UnsupportedCodec) and
    /// [`HwDecoderUnavailable`](Self::HwDecoderUnavailable). Lets a caller
    /// distinguish a capability gap from corruption without matching every
    /// variant of a `#[non_exhaustive]` enum.
    #[must_use]
    pub const fn is_capability_gap(&self) -> bool {
        matches!(
            self,
            Self::UnsupportedContainer { .. }
                | Self::UnsupportedCodec { .. }
                | Self::HwDecoderUnavailable { .. }
        )
    }
}

/// `Result` alias used throughout rawshift's video crates.
pub type VideoResult<T> = Result<T, VideoError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_gaps_are_distinguished_from_corruption() {
        assert!(
            VideoError::UnsupportedCodec {
                codec: VideoCodecId::ProRes
            }
            .is_capability_gap()
        );
        assert!(VideoError::hw_unavailable(VideoCodecId::Hevc, "no libva").is_capability_gap());
        assert!(
            VideoError::UnsupportedContainer {
                detected: Some(ContainerId::Mxf)
            }
            .is_capability_gap()
        );

        assert!(!VideoError::container("MP4", "truncated moov").is_capability_gap());
        assert!(!VideoError::decode(VideoCodecId::H264, "bad slice").is_capability_gap());
        assert!(!VideoError::Io(io::Error::from(io::ErrorKind::UnexpectedEof)).is_capability_gap());
    }

    #[test]
    fn unsupported_container_reads_well_both_ways() {
        let named = VideoError::UnsupportedContainer {
            detected: Some(ContainerId::Avi),
        };
        assert_eq!(named.to_string(), "unsupported container: AVI");

        let unknown = VideoError::UnsupportedContainer { detected: None };
        assert_eq!(unknown.to_string(), "unsupported container: unrecognised");
    }

    #[test]
    fn backend_detail_is_preserved_in_the_message() {
        let e = VideoError::container("Matroska", "unexpected EBML id 0x1f43b675");
        assert!(e.to_string().contains("Matroska"));
        assert!(e.to_string().contains("0x1f43b675"));
    }

    #[test]
    fn hw_unavailable_names_the_codec_and_reason() {
        let e = VideoError::hw_unavailable(VideoCodecId::H264, "no VAAPI device");
        let s = e.to_string();
        assert!(s.contains("H.264"), "{s}");
        assert!(s.contains("no VAAPI device"), "{s}");
    }

    #[test]
    fn io_errors_convert_with_the_question_mark_operator() {
        fn inner() -> VideoResult<()> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))?;
            unreachable!()
        }
        assert!(matches!(inner(), Err(VideoError::Io(_))));
    }
}
