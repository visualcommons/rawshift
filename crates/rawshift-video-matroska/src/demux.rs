//! The Matroska / WebM demuxer.

use std::io::{Read, Seek};

use symphonia_core::formats::{FormatOptions, FormatReader};
use symphonia_core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia_format_mkv::MkvReader;

use rawshift_core::MetadataNamespace;
use rawshift_core::MetadataValue;
use rawshift_video_core::{
    CodecConfig, ContainerId, Demuxer, Packet, Rotation, SeekMode, SeekTo, Track, TrackId,
    VideoCodecId, VideoError, VideoMetadata, VideoResult,
};
use rawshift_video_symphonia as bridge;

/// The name that appears in [`VideoError::Container`] messages.
const CONTAINER_NAME: &str = "Matroska";

/// A demuxer for Matroska and WebM files.
pub struct MatroskaDemuxer {
    reader: MkvReader<'static>,
    container: ContainerId,
    tracks: Vec<Track>,
    metadata: VideoMetadata,
}

impl MatroskaDemuxer {
    /// Open a Matroska or WebM file.
    ///
    /// `container` distinguishes the two flavours; use [`crate::detect`] on a
    /// prefix of the file to obtain it.
    ///
    /// # Errors
    ///
    /// [`VideoError::Container`] for a malformed or unsupported file, and
    /// [`VideoError::Io`] for a reader failure.
    pub fn open<R>(source: R, container: ContainerId) -> VideoResult<Self>
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        let stream = MediaSourceStream::new(
            Box::new(bridge::SeekableSource::new(source)),
            MediaSourceStreamOptions::default(),
        );
        let reader = MkvReader::try_new(stream, FormatOptions::default())
            .map_err(|e| VideoError::container(CONTAINER_NAME, e))?;

        // Matroska states rotation in Video/Projection, which the backend does
        // not surface; see the crate docs for why that costs nothing here.
        let tracks = reader
            .tracks()
            .iter()
            .map(|t| bridge::map_track(t, Rotation::None))
            .collect();

        let mut metadata = VideoMetadata::default();
        metadata.container.container = Some(container);
        let info = reader.media_info();
        if let (Some(tb), Some(dur)) = (info.time_base, info.duration) {
            let time_base = bridge::map_time_base(Some(tb));
            metadata.container.duration = bridge::ticks_to_duration(dur.get(), time_base);
            metadata.container.timescale = Some(time_base.denominator);
        }
        metadata.push_container_entry(
            MetadataNamespace::Matroska,
            "DocType",
            MetadataValue::Text(
                if container == ContainerId::WebM {
                    "webm"
                } else {
                    "matroska"
                }
                .to_string(),
            ),
        );

        Ok(Self {
            reader,
            container,
            tracks,
            metadata,
        })
    }

    fn video_codec_of(&self, track: TrackId) -> Option<(VideoCodecId, &CodecConfig)> {
        self.tracks
            .iter()
            .find(|t| t.id() == track)
            .and_then(Track::as_video)
            .map(|v| (v.codec, &v.codec_config))
    }
}

impl Demuxer for MatroskaDemuxer {
    fn container(&self) -> ContainerId {
        self.container
    }

    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn metadata(&self) -> &VideoMetadata {
        &self.metadata
    }

    fn next_packet(&mut self) -> VideoResult<Option<Packet>> {
        // Peek the track before consuming, so the keyframe flag can be derived
        // from the right track's codec configuration.
        let result = self.reader.next_packet();
        let video = match &result {
            Ok(Some(p)) => self.video_codec_of(TrackId::new(p.track_id)),
            _ => None,
        };
        bridge::map_packet_result(CONTAINER_NAME, result, video)
    }

    fn seek(&mut self, track: TrackId, to: SeekTo, mode: SeekMode) -> VideoResult<i64> {
        if self.track(track).is_none() {
            return Err(VideoError::NoSuchTrack { id: track });
        }
        let (sym_mode, target) = bridge::map_seek(CONTAINER_NAME, track, to, mode)?;

        self.reader
            .seek(sym_mode, target)
            .map(|landed| landed.actual_ts.get())
            .map_err(|e| VideoError::container(CONTAINER_NAME, e))
    }
}
