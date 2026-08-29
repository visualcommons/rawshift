//! Video decoding and metadata for [rawshift].
//!
//! The video counterpart to `rawshift-image`: a feature facade over the
//! container and decoder crates, plus the entry points that tie them together.
//!
//! ```no_run
//! use rawshift_video::{VideoFile, Track};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut file = VideoFile::open(std::fs::File::open("clip.mp4")?)?;
//!
//! println!("{}, {:?}", file.container(), file.metadata().container.duration);
//! for track in file.video_tracks() {
//!     println!("{} {}x{}", track.codec, track.dimensions.width, track.dimensions.height);
//! }
//!
//! // Decoding needs a hardware backend; everything above does not.
//! if let Some(frame) = file.thumbnail()? {
//!     println!("{}x{}", frame.width(), frame.height());
//! }
//! # Ok(()) }
//! ```
//!
//! # What works without a decoder
//!
//! Container parsing, track enumeration, the timeline, packet access and
//! metadata work on **every** target in `docs/SUPPORT.md`, including `wasm32`
//! and `musl` where no hardware decode API exists. Only frame decode needs a
//! backend; without one it reports
//! [`VideoError::HwDecoderUnavailable`], which
//! [`hw_decode_available`] lets a caller check up front.
//!
//! # Features
//!
//! - **Bundles** — `video` (every container and codec), `full` (adds `hw`).
//! - **Containers** — `mp4` (MP4/M4V/QuickTime), `mkv` (Matroska/WebM).
//! - **Codecs** — `h264`, `hevc`.
//! - **Backends** — `hw` (portable), `hw-vaapi` / `hw-videotoolbox` /
//!   `hw-mediacodec` (pinned; these fail to compile off-target by design).
//! - **Device aliases** — `xavc-hs`, `xavc-s`.
//!
//! A container feature without a codec feature is a valid
//! metadata-and-timeline build, the same shape as `heic`/`avif` without `hw`
//! on the image side.
//!
//! [rawshift]: https://github.com/visualcommons/rawshift

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod file;

pub mod prelude;

pub use file::{VideoFile, VideoProbe, VideoSource, detect_container};

// The vocabulary is re-exported wholesale: callers work in these types, and
// making them reach for `rawshift-video-core` would be a papercut with no
// benefit.
pub use rawshift_video_core::{
    AudioCodecId, AudioTrack, ChromaSubsampling, CicpColor, CodecConfig, ColorRange, ContainerId,
    ContainerMetadata, Demuxer, FrameDesc, FramePixelFormat, MatrixCoefficients, Packet, Plane,
    Rotation, SeekMode, SeekTo, Track, TrackId, TrackKind, VideoCodecId, VideoDecoder,
    VideoDecoderFactory, VideoEncoder, VideoError, VideoFrame, VideoMetadata, VideoResult,
    VideoTrack,
};

use rawshift_core::{CodecDirection, CodecInfo};
use rawshift_video_core::DecoderRegistry;

/// The decoder backends compiled into this build, in priority order.
///
/// Built once: the set is fixed at compile time, and each factory's
/// `supports()` answers the per-machine question on every call anyway.
fn registry() -> &'static DecoderRegistry {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<DecoderRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        #[allow(unused_mut)]
        let mut registry = DecoderRegistry::new();
        // `dep:` optional dependencies create no implicit feature, so this
        // keys off the direction features that pull the crate in.
        #[cfg(any(feature = "h264-decode", feature = "hevc-decode"))]
        rawshift_video_hwdec::register(&mut registry);
        registry
    })
}

/// Every decoder compiled into this build.
///
/// Reports what was *compiled in*, not what this machine can run — ask
/// [`hw_decode_available`] for that. Mirrors `rawshift_image::available_decoders`.
#[must_use]
pub fn available_decoders() -> Vec<CodecInfo> {
    registry()
        .factories()
        .iter()
        .map(|f| CodecInfo {
            id: f.id(),
            // Hardware decoders report the platform API's version at runtime,
            // which the seam does not surface; the crate version is the honest
            // stand-in and matches how the image registry handles pure-Rust
            // backends.
            version: env!("CARGO_PKG_VERSION").to_string(),
            direction: CodecDirection::Decode,
        })
        .collect()
}

/// Whether a decoder for `codec` can actually run on this machine right now.
///
/// False when no decoder was compiled in **and** when one was but no backend
/// is usable — a caller that only needs to know "will decoding work" should
/// ask this rather than inspecting features. Mirrors the image side's
/// `heic_hw_decode_available()` / `avif_hw_decode_available()`.
#[must_use]
pub fn hw_decode_available(codec: VideoCodecId) -> bool {
    registry()
        .factories()
        .iter()
        .any(|f| f.codec() == codec && f.supports(&probe_track(codec)))
}

/// A minimal track used only to ask a factory whether it could decode this
/// codec on this machine.
fn probe_track(codec: VideoCodecId) -> VideoTrack {
    let mut track = VideoTrack::new(
        TrackId::new(0),
        codec,
        rawshift_core::Dimensions {
            width: 1920,
            height: 1080,
        },
        rawshift_core::metadata::URational::new(1, 1000),
    );
    // Factories refuse a track with no configuration record, since no session
    // could be opened for it. A non-empty placeholder asks the question this
    // function actually means: "is there a backend for this codec".
    track.codec_config = CodecConfig::new(vec![0u8; 32]);
    track
}
