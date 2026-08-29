//! Matroska and WebM support for rawshift.
//!
//! Backed by `symphonia-format-mkv` — MPL-2.0, pure Rust, no build script —
//! which maps `V_MPEG4/ISO/AVC` and `V_MPEGH/ISO/HEVC` tracks and surfaces
//! their `CodecPrivate` as codec extra data. That `CodecPrivate` holds the
//! same `avcC`/`hvcC` records MP4 carries, so decoders take the same input
//! from either container.
//!
//! As in `rawshift-video-isobmff`, random access points are read from the
//! bitstream rather than from a container flag, and no symphonia type appears
//! in the public API.
//!
//! # Not covered
//!
//! Matroska expresses display rotation through `Video/Projection`, which the
//! backend does not surface; tracks therefore report
//! [`Rotation::None`](rawshift_video_core::Rotation::None). This costs nothing
//! in practice — rotation is a phone-capture concern, and phones write MP4 or
//! QuickTime, not Matroska.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod demux;
mod sniff;

pub use demux::MatroskaDemuxer;
pub use sniff::{Matroska, detect};
