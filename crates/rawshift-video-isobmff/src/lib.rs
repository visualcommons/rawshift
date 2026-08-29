//! MP4, M4V and QuickTime (MOV) support for rawshift.
//!
//! Demuxing is backed by `symphonia-format-isomp4` — MPL-2.0, pure Rust, no
//! build script, so it compiles on every target in `docs/SUPPORT.md`. gamut's
//! charter excludes video, so this is a direct dependency rather than an
//! upstream ask; see "Video is outside gamut's charter" in `AGENTS.md`.
//!
//! Three things the backend does not surface, which this crate supplies:
//!
//! - **Rotation**, from the `tkhd` display matrix. Phones record in the
//!   sensor's orientation and correct in the container, so without this most
//!   phone video presents sideways.
//! - **`ftyp` brands and `mvhd` timescale and times**, for metadata and for
//!   telling MP4 from QuickTime.
//! - **Random access points.** The backend exposes no sync-sample flag at all,
//!   so these are read from the bitstream by `rawshift_video_core::nal`, which
//!   is the authoritative source regardless.
//!
//! No symphonia type appears in this crate's public API.
//!
//! # Not covered
//!
//! ISOBMFF carries colour signalling in a `colr` box the backend does not
//! expose, so tracks report [`CicpColor::UNSPECIFIED`] and colour resolves by
//! picture height. ProRes tracks are invisible: the backend does not recognise
//! the `ap4h`/`apch`/`apcn`/`apcs`/`apco` sample entries.
//!
//! [`CicpColor::UNSPECIFIED`]: rawshift_video_core::CicpColor::UNSPECIFIED

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod boxes;
mod demux;
mod sniff;

pub use demux::IsoBmffDemuxer;
pub use sniff::{IsoBmff, detect};
