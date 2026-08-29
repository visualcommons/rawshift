//! The demuxing contract.

use crate::error::VideoResult;
use crate::id::{ContainerId, TrackId};
use crate::metadata::VideoMetadata;
use crate::packet::{Packet, SeekMode, SeekTo};
use crate::track::Track;

/// Cheap signature detection for one container.
///
/// Deliberately a static method on a marker type rather than a method on a
/// demuxer: detection must be answerable from a handful of leading bytes,
/// before anything is opened or allocated.
pub trait ContainerSniffer {
    /// The container this sniffer recognises.
    const CONTAINER: ContainerId;

    /// Whether `data` looks like this container.
    ///
    /// `data` may be short; return `false` rather than panicking. A `true`
    /// here is a claim about the signature only, not that the file is valid.
    fn matches(data: &[u8]) -> bool;
}

/// A container reader: tracks, metadata, and a packet stream.
///
/// Object-safe, and held behind `Box<dyn Demuxer>` by the aggregator, so the
/// set of compiled-in containers is a runtime property rather than a type
/// parameter that would leak into every caller's signature.
pub trait Demuxer: Send {
    /// Which container this is reading.
    fn container(&self) -> ContainerId;

    /// Every track in the file, in the container's own order.
    ///
    /// Available immediately after opening, on every target, with no decoder:
    /// track enumeration never needs a codec.
    fn tracks(&self) -> &[Track];

    /// The file's container-level metadata.
    fn metadata(&self) -> &VideoMetadata;

    /// The next packet from any track, in stored order, or `None` at end of
    /// file.
    ///
    /// Packets from every track are interleaved as the container stores them.
    /// Filter on [`Packet::track`] to follow one track; a caller that reads
    /// only one track must still drain the others or they accumulate.
    ///
    /// # Errors
    ///
    /// Propagates I/O failures and container errors. An error is not
    /// necessarily terminal, but a caller that cannot re-synchronise should
    /// stop.
    fn next_packet(&mut self) -> VideoResult<Option<Packet>>;

    /// Seek `track` to `to`, returning the timestamp actually landed on.
    ///
    /// With [`SeekMode::Precise`] the landing point is at or before the
    /// request and always a random access point, so decoding can resume;
    /// reaching an exact frame means decoding forward from there and
    /// discarding. Any decoder consuming this demuxer must be
    /// [`reset`](crate::decode::VideoDecoder::reset) afterwards.
    ///
    /// # Errors
    ///
    /// [`VideoError::NoSuchTrack`](crate::error::VideoError::NoSuchTrack) for
    /// an unknown track, plus I/O and container errors. Seeking a
    /// non-seekable source fails rather than silently reading forward.
    fn seek(&mut self, track: TrackId, to: SeekTo, mode: SeekMode) -> VideoResult<i64>;

    /// The track with this identifier, if the file has one.
    ///
    /// Provided: a linear scan over [`tracks`](Self::tracks). Track counts are
    /// single digits in practice, so no index is worth maintaining.
    fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks().iter().find(|t| t.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::VideoError;
    use crate::id::TrackKind;
    use crate::track::OtherTrack;
    use rawshift_core::metadata::URational;

    struct StubDemuxer {
        tracks: Vec<Track>,
        metadata: VideoMetadata,
    }

    fn other_track(id: u32) -> Track {
        Track::Other(OtherTrack {
            id: TrackId::new(id),
            kind: TrackKind::Data,
            time_base: URational::new(1, 1000),
            duration: None,
            language: None,
        })
    }

    impl Demuxer for StubDemuxer {
        fn container(&self) -> ContainerId {
            ContainerId::Mp4
        }
        fn tracks(&self) -> &[Track] {
            &self.tracks
        }
        fn metadata(&self) -> &VideoMetadata {
            &self.metadata
        }
        fn next_packet(&mut self) -> VideoResult<Option<Packet>> {
            Ok(None)
        }
        fn seek(&mut self, track: TrackId, _to: SeekTo, _mode: SeekMode) -> VideoResult<i64> {
            Err(VideoError::NoSuchTrack { id: track })
        }
    }

    #[test]
    fn the_provided_track_lookup_matches_on_container_id_not_index() {
        // Container-assigned ids are 1-based and need not be contiguous, so a
        // lookup must never be an index into the slice.
        let d = StubDemuxer {
            tracks: vec![other_track(3), other_track(9)],
            metadata: VideoMetadata::default(),
        };

        assert_eq!(
            d.track(TrackId::new(3)).map(Track::id),
            Some(TrackId::new(3))
        );
        assert_eq!(
            d.track(TrackId::new(9)).map(Track::id),
            Some(TrackId::new(9))
        );
        // Id 1 exists as neither a track id nor a valid index shortcut.
        assert!(d.track(TrackId::new(1)).is_none());
        assert!(d.track(TrackId::new(0)).is_none());
    }

    #[test]
    fn a_demuxer_is_object_safe() {
        // The aggregator stores Box<dyn Demuxer>; this fails to compile if a
        // signature above ever stops being object-safe.
        let d: Box<dyn Demuxer> = Box::new(StubDemuxer {
            tracks: Vec::new(),
            metadata: VideoMetadata::default(),
        });
        assert_eq!(d.container(), ContainerId::Mp4);
    }
}
