//! [`VideoFile`]: opening a file and working with what is in it.

use std::io::{Read, Seek};

use rawshift_video_core::{
    ContainerId, Demuxer, Packet, SeekMode, SeekTo, Track, TrackId, VideoDecoder, VideoError,
    VideoFrame, VideoMetadata, VideoResult, VideoTrack,
};

/// A source [`VideoFile`] can read.
///
/// Blanket-implemented for every `Read + Seek + Send + Sync + 'static`, which
/// covers `File`, `&[u8]` and `Cursor<Vec<u8>>`.
///
/// It exists as a **sealed** trait rather than as a bare set of bounds because
/// the bounds are not rawshift's own: the Matroska backend needs `Sync` and
/// `'static`, while the ISOBMFF one does not. Naming the requirement once
/// keeps that backend detail out of rawshift's public contract, so relaxing it
/// later is not a breaking change.
pub trait VideoSource: sealed::Sealed + Read + Seek + Send + Sync + 'static {}

impl<T: Read + Seek + Send + Sync + 'static> VideoSource for T {}

mod sealed {
    /// Prevents outside implementations, so the supertrait bounds above stay
    /// an implementation detail rather than a promise.
    pub trait Sealed {}
    impl<T: std::io::Read + std::io::Seek + Send + Sync + 'static> Sealed for T {}
}

/// How many leading bytes container detection needs.
///
/// Enough for an ISOBMFF `ftyp` header and brand, and for the EBML header a
/// Matroska `DocType` sits in.
const SNIFF_BYTES: usize = 64;

/// Identify the container from a prefix of a file.
///
/// Returns `None` when nothing matches, and may name a container this build
/// cannot open — [`ContainerId`] is a vocabulary of what rawshift can
/// *recognise*, which is deliberately wider than what it can demux.
#[must_use]
pub fn detect_container(data: &[u8]) -> Option<ContainerId> {
    #[cfg(feature = "mp4")]
    if let Some(container) = rawshift_video_isobmff::detect(data) {
        return Some(container);
    }
    #[cfg(feature = "mkv")]
    if let Some(container) = rawshift_video_matroska::detect(data) {
        return Some(container);
    }
    let _ = data;
    None
}

/// What [`VideoFile::probe`] reports without committing to a full open.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct VideoProbe {
    /// The container.
    pub container: ContainerId,
    /// Every track in the file.
    pub tracks: Vec<Track>,
    /// File-level metadata.
    pub metadata: VideoMetadata,
}

/// An open video file.
///
/// Not generic over the reader: the containers box their sources internally,
/// so carrying `R` in the type would constrain callers without buying
/// anything. Sources need `Read + Seek + Send`, which `File`, `&[u8]` and
/// `Cursor<Vec<u8>>` all satisfy.
pub struct VideoFile {
    demuxer: Box<dyn Demuxer>,
}

impl std::fmt::Debug for VideoFile {
    /// Summarises the file rather than dumping it: the demuxer is a trait
    /// object with nothing printable, and the track list can be long.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoFile")
            .field("container", &self.container())
            .field("tracks", &self.tracks().len())
            .field(
                "video_tracks",
                &self.video_tracks().map(|t| t.codec).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl VideoFile {
    /// Open a video file.
    ///
    /// The container is detected from the file's leading bytes, so the caller
    /// does not name it.
    ///
    /// # Errors
    ///
    /// [`VideoError::UnsupportedContainer`] when the bytes are not a container
    /// this build can open — with `detected` naming it when rawshift
    /// recognised the format but has no demuxer compiled in, which is the
    /// difference between "rebuild with the feature" and "not a video file".
    /// [`VideoError::Container`] for a malformed file, and
    /// [`VideoError::Io`] for a reader failure.
    pub fn open<R: VideoSource>(mut source: R) -> VideoResult<Self> {
        let mut header = [0u8; SNIFF_BYTES];
        let read = read_prefix(&mut source, &mut header)?;
        let detected = detect_container(&header[..read]);

        match detected {
            #[cfg(feature = "mp4")]
            Some(container) if container.is_isobmff() => {
                let demuxer = rawshift_video_isobmff::IsoBmffDemuxer::open(source)?;
                Ok(Self {
                    demuxer: Box::new(demuxer),
                })
            }
            #[cfg(feature = "mkv")]
            Some(container) if container.is_matroska() => {
                let demuxer = rawshift_video_matroska::MatroskaDemuxer::open(source, container)?;
                Ok(Self {
                    demuxer: Box::new(demuxer),
                })
            }
            // Recognised, but this build has no demuxer for it — or rawshift
            // names it and implements nothing, as with AVI and MXF.
            other => Err(VideoError::UnsupportedContainer { detected: other }),
        }
    }

    /// Open a file just far enough to report its tracks and metadata.
    ///
    /// # Errors
    ///
    /// As [`open`](Self::open).
    pub fn probe<R: VideoSource>(source: R) -> VideoResult<VideoProbe> {
        let file = Self::open(source)?;
        Ok(VideoProbe {
            container: file.container(),
            tracks: file.tracks().to_vec(),
            metadata: file.metadata().clone(),
        })
    }

    /// The container this file is in.
    #[must_use]
    pub fn container(&self) -> ContainerId {
        self.demuxer.container()
    }

    /// Every track, in the container's own order.
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        self.demuxer.tracks()
    }

    /// The video tracks.
    pub fn video_tracks(&self) -> impl Iterator<Item = &VideoTrack> {
        self.tracks().iter().filter_map(Track::as_video)
    }

    /// The first video track, which is what "the video" means for the
    /// single-stream files cameras produce.
    #[must_use]
    pub fn primary_video_track(&self) -> Option<&VideoTrack> {
        self.video_tracks().next()
    }

    /// File-level metadata.
    #[must_use]
    pub fn metadata(&self) -> &VideoMetadata {
        self.demuxer.metadata()
    }

    /// The next packet from any track, in stored order.
    ///
    /// # Errors
    ///
    /// Container and I/O failures.
    pub fn next_packet(&mut self) -> VideoResult<Option<Packet>> {
        self.demuxer.next_packet()
    }

    /// Seek `track` to `to`, returning the timestamp landed on.
    ///
    /// # Errors
    ///
    /// [`VideoError::NoSuchTrack`] for an unknown track, plus container and
    /// I/O failures.
    pub fn seek(&mut self, track: TrackId, to: SeekTo, mode: SeekMode) -> VideoResult<i64> {
        self.demuxer.seek(track, to, mode)
    }

    /// Open a decoder for `track`.
    ///
    /// # Errors
    ///
    /// [`VideoError::NoSuchTrack`] when the track does not exist or is not
    /// video, [`VideoError::UnsupportedCodec`] when no decoder for its codec
    /// was compiled in, and [`VideoError::HwDecoderUnavailable`] when one was
    /// but no backend on this machine can drive it.
    pub fn decoder(&self, track: TrackId) -> VideoResult<Box<dyn VideoDecoder>> {
        let video = self
            .tracks()
            .iter()
            .find(|t| t.id() == track)
            .and_then(Track::as_video)
            .ok_or(VideoError::NoSuchTrack { id: track })?;
        crate::registry().open(video)
    }

    /// Decode the file's first frame.
    ///
    /// The video analogue of a still's embedded thumbnail: the cheapest
    /// representative image, decoded from the first random access point rather
    /// than read from a preview the container may not carry.
    ///
    /// `Ok(None)` when the file has no video track or no decodable frame.
    ///
    /// # Errors
    ///
    /// As [`decoder`](Self::decoder), plus decode failures.
    pub fn thumbnail(&mut self) -> VideoResult<Option<VideoFrame>> {
        let Some(track) = self.primary_video_track().map(|t| t.id) else {
            return Ok(None);
        };
        let mut decoder = self.decoder(track)?;

        while let Some(packet) = self.demuxer.next_packet()? {
            if packet.track != track || !packet.is_keyframe {
                continue;
            }
            decoder.send_packet(&packet)?;
            if let Some(frame) = decoder.receive_frame()? {
                return Ok(Some(frame));
            }
            // A decoder that buffered the first picture has nothing more
            // coming without an end-of-stream signal.
            decoder.flush()?;
            if let Some(frame) = decoder.receive_frame()? {
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }
}

/// Read up to `buf.len()` bytes, then rewind.
///
/// `read` may return fewer bytes than asked for without being at end of file,
/// so this loops; a single call would misdetect a container on a reader that
/// returns short reads.
fn read_prefix<R: Read + Seek>(source: &mut R, buf: &mut [u8]) -> VideoResult<usize> {
    use std::io::SeekFrom;

    let mut filled = 0;
    while filled < buf.len() {
        match source.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    source.seek(SeekFrom::Start(0))?;
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawshift_video_core::VideoCodecId;
    use std::io::Cursor;

    #[test]
    fn unrelated_bytes_are_not_a_container() {
        assert_eq!(detect_container(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x00"), None);
        assert_eq!(detect_container(b"not a video at all"), None);
        assert_eq!(detect_container(&[]), None);
    }

    #[test]
    fn opening_a_non_container_reports_unsupported_with_nothing_detected() {
        let err = VideoFile::open(Cursor::new(b"\x89PNG\r\n\x1a\n----".to_vec()))
            .expect_err("a PNG is not a video");
        match err {
            VideoError::UnsupportedContainer { detected } => {
                assert_eq!(detected, None, "nothing should have been recognised");
            }
            other => panic!("expected UnsupportedContainer, got {other}"),
        }
    }

    #[test]
    fn a_recognised_but_unimplemented_container_names_itself() {
        // The distinction a caller acts on: "rebuild with the feature" or
        // "rawshift will never read this", versus "not a video file".
        let err = VideoFile::open(Cursor::new(b"RIFF\x00\x00\x00\x00AVI LIST".to_vec()))
            .expect_err("AVI is named but not implemented");
        assert!(matches!(err, VideoError::UnsupportedContainer { .. }));
    }

    #[test]
    fn a_short_reader_does_not_break_detection() {
        /// Returns one byte at a time, as a pipe or a slow socket would.
        struct Dribble(Cursor<Vec<u8>>);
        impl Read for Dribble {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if buf.is_empty() {
                    return Ok(0);
                }
                self.0.read(&mut buf[..1])
            }
        }
        impl Seek for Dribble {
            fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
                self.0.seek(pos)
            }
        }

        let mut source = Dribble(Cursor::new(
            b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00mp41 padding padding".to_vec(),
        ));
        let mut buf = [0u8; SNIFF_BYTES];
        let read = read_prefix(&mut source, &mut buf).expect("prefix read");
        assert!(
            read > 12,
            "a one-byte-at-a-time reader must still fill the buffer"
        );
        assert_eq!(detect_container(&buf[..read]), Some(ContainerId::Mp4));
    }

    #[test]
    fn reading_the_prefix_leaves_the_source_rewound() {
        // The demuxer that follows starts from byte zero, so detection must
        // not consume the header.
        let mut source = Cursor::new(b"\x00\x00\x00\x18ftypisom".to_vec());
        let mut buf = [0u8; SNIFF_BYTES];
        read_prefix(&mut source, &mut buf).expect("prefix read");
        assert_eq!(source.position(), 0);
    }

    #[test]
    fn hw_decode_availability_is_answerable_for_every_codec() {
        // Must never panic, whatever this machine has, and must be false for
        // codecs rawshift has no decoder for at all.
        for codec in [VideoCodecId::H264, VideoCodecId::Hevc] {
            let _ = crate::hw_decode_available(codec);
        }
        assert!(!crate::hw_decode_available(VideoCodecId::ProRes));
        assert!(!crate::hw_decode_available(VideoCodecId::Vp9));
    }

    #[test]
    fn the_decoder_registry_only_reports_decode_direction() {
        for info in crate::available_decoders() {
            assert_eq!(info.direction, rawshift_core::CodecDirection::Decode);
            assert!(info.id.id.contains('/'), "ids are \"format/impl\"");
        }
    }
}
