//! The curated surface, for `use rawshift_video::prelude::*;`.
//!
//! Everything needed to open a file, inspect its tracks and metadata, and
//! decode frames — without naming a container or backend crate.

pub use crate::{VideoFile, VideoProbe, available_decoders, detect_container, hw_decode_available};

pub use rawshift_video_core::{
    AudioTrack, ContainerId, Demuxer, Packet, Rotation, SeekMode, SeekTo, Track, TrackId,
    TrackKind, VideoCodecId, VideoDecoder, VideoError, VideoFrame, VideoMetadata, VideoResult,
    VideoTrack,
};
