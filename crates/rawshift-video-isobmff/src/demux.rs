//! The MP4 / QuickTime demuxer.

use std::io::{Read, Seek, SeekFrom};

use rawshift_video_core::{
    ContainerId, Demuxer, Packet, SeekMode, SeekTo, Track, TrackId, VideoError, VideoMetadata,
    VideoResult, nal,
};

use crate::index::SampleIndex;
use crate::movie::{CONTAINER_NAME, Movie};

/// A demuxer for MP4, M4V and QuickTime files.
///
/// Samples are read on demand from the source rather than held in memory, so
/// opening a file costs only its headers however large the media is.
pub struct IsoBmffDemuxer<R> {
    source: R,
    tracks: Vec<Track>,
    indices: Vec<SampleIndex>,
    /// Read cursor into each track's sample index, parallel to `tracks`.
    cursors: Vec<usize>,
    container: ContainerId,
    metadata: VideoMetadata,
}

impl<R: Read + Seek + Send> IsoBmffDemuxer<R> {
    /// Open an ISOBMFF file.
    ///
    /// # Errors
    ///
    /// [`VideoError::Container`] for a file with no `moov`, a malformed atom,
    /// or a fragmented file, and [`VideoError::Io`] for a reader failure.
    pub fn open(mut source: R) -> VideoResult<Self> {
        let movie = Movie::read(&mut source)?;
        let cursors = vec![0; movie.tracks.len()];

        Ok(Self {
            source,
            tracks: movie.tracks,
            indices: movie.indices,
            cursors,
            container: movie.container,
            metadata: movie.metadata,
        })
    }

    /// The position of a track in the parallel `tracks`/`indices`/`cursors`
    /// vectors.
    ///
    /// Container-assigned track ids are 1-based and need not be contiguous, so
    /// this is a search rather than an index.
    fn slot_of(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| t.id() == id)
    }

    /// The track whose next unread sample decodes earliest.
    ///
    /// Packets are delivered in the file's own interleaving so that a caller
    /// reading every track streams the file in one forward pass. Ordering by
    /// decode timestamp reproduces that interleaving from the per-track
    /// indices.
    fn next_slot(&self) -> Option<usize> {
        self.cursors
            .iter()
            .enumerate()
            .filter_map(|(slot, &cursor)| {
                self.indices[slot]
                    .samples()
                    .get(cursor)
                    .map(|sample| (slot, sample.dts, sample.offset))
            })
            // Ties break on file offset, which keeps the order stable and
            // matches how the samples are actually laid out.
            .min_by_key(|&(_, dts, offset)| (dts, offset))
            .map(|(slot, _, _)| slot)
    }

    /// Read one sample's bytes.
    fn read_sample(&mut self, offset: u64, size: u32) -> VideoResult<Vec<u8>> {
        self.source.seek(SeekFrom::Start(offset))?;
        let mut data = vec![0u8; size as usize];
        self.source.read_exact(&mut data)?;
        Ok(data)
    }
}

impl<R: Read + Seek + Send> Demuxer for IsoBmffDemuxer<R> {
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
        let Some(slot) = self.next_slot() else {
            return Ok(None);
        };
        let sample = self.indices[slot].samples()[self.cursors[slot]];
        self.cursors[slot] += 1;

        let data = self.read_sample(sample.offset, sample.size)?;
        let track = self.tracks[slot].id();

        // The container's sync table is advisory; for the codecs rawshift
        // decodes, the bitstream is authoritative and disagrees often enough
        // to matter. Fall back to the table for anything else.
        let is_keyframe = match self.tracks[slot].as_video() {
            Some(video) if !video.codec_config.is_empty() => {
                nal::is_random_access_point(video.codec, &video.codec_config, &data)
            }
            _ => sample.is_sync,
        };

        let mut packet = Packet::new(track, data);
        packet.pts = Some(sample.pts);
        packet.dts = Some(sample.dts);
        packet.duration = Some(u64::from(sample.duration));
        packet.is_keyframe = is_keyframe;
        Ok(Some(packet))
    }

    fn seek(&mut self, track: TrackId, to: SeekTo, mode: SeekMode) -> VideoResult<i64> {
        let Some(slot) = self.slot_of(track) else {
            return Err(VideoError::NoSuchTrack { id: track });
        };

        let time_base = self.tracks[slot].time_base();
        let timestamp = match to {
            SeekTo::Timestamp(ts) => ts,
            SeekTo::Time(time) => {
                if time_base.numerator == 0 {
                    return Err(VideoError::container(
                        CONTAINER_NAME,
                        "track has a degenerate time base; seek by timestamp instead",
                    ));
                }
                // ticks = seconds * (denominator / numerator)
                (time.as_secs_f64() * f64::from(time_base.denominator)
                    / f64::from(time_base.numerator)) as i64
            }
            other => {
                return Err(VideoError::container(
                    CONTAINER_NAME,
                    format!("unsupported seek target: {other:?}"),
                ));
            }
        };

        // Both seek modes land on a random access point: a decoder cannot
        // resume anywhere else. Coarse differs only in that the caller has said
        // it will not decode forward to reach the exact frame, which changes
        // nothing about where the demuxer can put the cursor.
        let _ = mode;
        let index = &self.indices[slot];
        let Some(landing) = index.sync_sample_at_or_before(timestamp) else {
            return Err(VideoError::container(
                CONTAINER_NAME,
                "track has no samples to seek within",
            ));
        };

        self.cursors[slot] = landing;
        Ok(index.samples()[landing].pts)
    }
}
