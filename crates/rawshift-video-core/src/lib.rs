//! Shared vocabulary for rawshift's video crates.
//!
//! This is the video counterpart to `rawshift-image-core`: the types every
//! `rawshift-video-*` crate agrees on, and no implementation of its own. No
//! I/O, no platform code, no container or codec dependencies.
//!
//! # Why this crate exists separately
//!
//! The workspace enforces in CI that no `rawshift-image*` crate appears in
//! `rawshift-video`'s dependency tree, so the video crates cannot reuse
//! `rawshift-image-core`'s error type or format traits. Image and video share
//! only [`rawshift_core`] — the media-agnostic geometry, codec descriptors,
//! metadata model, and colour vocabulary — and this crate adds the video
//! half on top of it.
//!
//! # The model
//!
//! - **Identity** — [`ContainerId`], [`VideoCodecId`], [`AudioCodecId`],
//!   [`TrackKind`], [`TrackId`]. All `#[non_exhaustive]`, and all naming more
//!   than rawshift can decode, so "recognised but unsupported" is expressible.
//! - **Errors** — [`VideoError`], which keeps every backend's own error type
//!   private, and distinguishes a capability gap from a broken file via
//!   [`VideoError::is_capability_gap`].
//! - **Tracks** — [`Track`], [`VideoTrack`], [`AudioTrack`], plus
//!   [`CicpColor`] and [`Rotation`]. All header-derived, so a full track list
//!   is available on targets with no decoder at all.
//! - **Packets and frames** — [`Packet`] in, [`VideoFrame`] out.
//! - **Contracts** — [`Demuxer`], [`VideoDecoder`], [`VideoEncoder`], and the
//!   [`DecoderRegistry`] that picks between decoder backends.
//!
//! # Decoding is a two-step conversation
//!
//! [`VideoDecoder`] splits submit from retrieve, because coded order and
//! display order differ whenever a stream reorders. See its documentation for
//! the loop, including the [`flush`](VideoDecoder::flush) without which the
//! last frames of every reordered stream are silently dropped.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod decode;
pub mod demux;
pub mod error;
pub mod frame;
pub mod id;
pub mod metadata;
pub mod packet;
pub mod track;

pub use decode::{DecoderRegistry, VideoDecoder, VideoDecoderFactory, VideoEncoder};
pub use demux::{ContainerSniffer, Demuxer};
pub use error::{VideoError, VideoResult};
pub use frame::{FrameDesc, FramePixelFormat, Plane, VideoFrame};
pub use id::{AudioCodecId, ContainerId, TrackId, TrackKind, VideoCodecId};
pub use metadata::{ContainerMetadata, VideoMetadata};
pub use packet::{Packet, SeekMode, SeekTo};
pub use track::{AudioTrack, CicpColor, CodecConfig, OtherTrack, Rotation, Track, VideoTrack};

// The colour vocabulary is gamut's, re-exported so callers need not depend on
// gamut directly — the same arrangement `rawshift-core` already uses for the
// pixel and CICP types.
pub use gamut_color::{
    ChromaSubsampling, ColorRange, ColourPrimaries, MatrixCoefficients, TransferCharacteristics,
};
