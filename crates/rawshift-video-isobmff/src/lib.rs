//! MP4, M4V and QuickTime (MOV) support for rawshift.
//!
//! The box layer is [`mp4_atom`] — MIT, pure Rust, no build script, so it
//! compiles on every target in `docs/SUPPORT.md`. It performs no
//! interpretation, so rawshift owns the parts that give a container meaning:
//! the [sample index](index) reconstructed from the `stbl` tables, the track
//! model, and the demux and seek semantics.
//!
//! gamut's charter excludes video, so this is a direct dependency rather than
//! an upstream ask; see "Video is outside gamut's charter" in `AGENTS.md`.
//!
//! # Why not an off-the-shelf demuxer
//!
//! This crate first used `symphonia-format-isomp4`, which is audio-first and
//! caps the `hvcC` configuration record at 1 KB. Real HEVC records are 2–3 KB,
//! so **every HEVC MP4 and MOV failed to open** — that is XAVC HS and default
//! iPhone video, the two highest-priority formats on rawshift's roadmap. It
//! also hides the `tkhd` matrix (and so rotation), the `colr` box (and so all
//! colour signalling), the sync-sample table, and ProRes-shaped sample
//! entries. Owning the sample index on a box-level library costs a few hundred
//! lines and removes all five limitations at once.
//!
//! # Not covered
//!
//! Fragmented MP4 (`mvex`/`moof`) is rejected with a clear error rather than
//! presented as a track with no samples. Camera-origin files — rawshift's
//! subject — are not fragmented; streaming-shaped input is a follow-up.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod index;

mod demux;
mod movie;
mod sniff;

pub use demux::IsoBmffDemuxer;
pub use index::{Sample, SampleIndex};
pub use sniff::{IsoBmff, detect};
