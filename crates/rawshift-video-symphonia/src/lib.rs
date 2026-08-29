//! Internal bridge between symphonia and rawshift's video model.
//!
//! Not a container crate and not a public API: it exists so
//! `rawshift-video-isobmff` and `rawshift-video-matroska` share one translation
//! instead of two copies that drift apart.
//!
//! The translation is one-directional and total — **no symphonia type leaves
//! this crate**. That is what lets the container crates make the same promise,
//! and it is the property to preserve when editing here: every function takes
//! symphonia types and returns rawshift ones.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

use symphonia_core::codecs::CodecParameters;
use symphonia_core::codecs::video::well_known as vwk;
use symphonia_core::codecs::video::well_known::extra_data as vxd;
use symphonia_core::codecs::video::{VideoCodecParameters, VideoExtraData};
use symphonia_core::errors::Error as SymError;
use symphonia_core::formats::{SeekMode as SymSeekMode, SeekTo as SymSeekTo, Track as SymTrack};
use symphonia_core::io::MediaSource;
use symphonia_core::packet::Packet as SymPacket;
use symphonia_core::units::{Time, TimeBase, Timestamp};

use rawshift_core::Dimensions;
use rawshift_core::metadata::URational;
use rawshift_video_core::{
    AudioCodecId, AudioTrack, CodecConfig, OtherTrack, Packet, SeekMode, SeekTo, Track, TrackId,
    TrackKind, VideoCodecId, VideoError, VideoResult, VideoTrack, nal,
};

/// The time base rawshift assumes when a track states none: milliseconds.
const DEFAULT_TIME_BASE: URational = URational {
    numerator: 1,
    denominator: 1000,
};

/// Adapts any `Read + Seek` source to symphonia's `MediaSource`.
///
/// rawshift's public API requires `Seek`, so `is_seekable` is unconditionally
/// true. `byte_len` returns `None` rather than seeking to the end on every
/// call: symphonia treats an unknown length as "find out by reading", which is
/// correct, and probing it here would be a hidden syscall on a hot path.
pub struct SeekableSource<R>(R);

impl<R> SeekableSource<R> {
    /// Wrap a source.
    pub const fn new(source: R) -> Self {
        Self(source)
    }
}

impl<R: Read> Read for SeekableSource<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl<R: Seek> Seek for SeekableSource<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

impl<R: Read + Seek + Send + Sync> MediaSource for SeekableSource<R> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

/// Translate a symphonia track, using `rotation` for video tracks.
///
/// Rotation is a parameter because no container surfaces it through
/// symphonia; each container crate recovers it its own way, or passes the
/// default.
#[must_use]
pub fn map_track(track: &SymTrack, rotation: rawshift_video_core::Rotation) -> Track {
    let id = TrackId::new(track.id);
    let time_base = map_time_base(track.time_base);
    let duration = track
        .duration
        .and_then(|d| ticks_to_duration(d.get(), time_base));

    match &track.codec_params {
        Some(CodecParameters::Video(v)) => {
            let mut out = map_video_track(id, v, time_base);
            out.duration = duration;
            out.frame_count = track.num_frames;
            out.rotation = rotation;
            Track::Video(out)
        }
        Some(CodecParameters::Audio(a)) => {
            let mut out = AudioTrack::new(id, AudioCodecId::Other, time_base);
            out.sample_rate = a.sample_rate;
            out.channels = a.channels.as_ref().map(|c| c.count() as u16);
            out.duration = duration;
            out.language = track.language.clone();
            Track::Audio(out)
        }
        _ => {
            let mut out = OtherTrack::new(id, TrackKind::Data, time_base);
            out.duration = duration;
            out.language = track.language.clone();
            Track::Other(out)
        }
    }
}

fn map_video_track(id: TrackId, params: &VideoCodecParameters, time_base: URational) -> VideoTrack {
    let codec = map_video_codec(params.codec);
    let dimensions = Dimensions {
        width: u32::from(params.width.unwrap_or(0)),
        height: u32::from(params.height.unwrap_or(0)),
    };

    let mut track = VideoTrack::new(id, codec, dimensions, time_base);
    track.codec_config = CodecConfig::new(config_record(codec, &params.extra_data));
    track
}

/// Pick the decoder configuration record matching `codec`.
///
/// Selected by id rather than by position: a track may carry several extra
/// data blobs — a Dolby Vision configuration accompanies an HEVC base layer,
/// for instance — and taking the first would hand a decoder the wrong record.
#[must_use]
pub fn config_record(codec: VideoCodecId, extra: &[VideoExtraData]) -> Vec<u8> {
    let wanted = match codec {
        VideoCodecId::H264 => vxd::VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG,
        VideoCodecId::Hevc => vxd::VIDEO_EXTRA_DATA_ID_HEVC_DECODER_CONFIG,
        VideoCodecId::Av1 => vxd::VIDEO_EXTRA_DATA_ID_AV1_DECODER_CONFIG,
        VideoCodecId::Vp9 => vxd::VIDEO_EXTRA_DATA_ID_VP9_DECODER_CONFIG,
        _ => return Vec::new(),
    };
    extra
        .iter()
        .find(|e| e.id == wanted)
        .map(|e| e.data.to_vec())
        .unwrap_or_default()
}

/// Translate a symphonia video codec id.
///
/// A codec rawshift does not model becomes [`VideoCodecId::Other`] rather than
/// being dropped, so the track still appears in the track list.
#[must_use]
pub fn map_video_codec(codec: symphonia_core::codecs::video::VideoCodecId) -> VideoCodecId {
    match codec {
        vwk::CODEC_ID_H264 => VideoCodecId::H264,
        vwk::CODEC_ID_HEVC => VideoCodecId::Hevc,
        vwk::CODEC_ID_AV1 => VideoCodecId::Av1,
        vwk::CODEC_ID_VP9 => VideoCodecId::Vp9,
        _ => VideoCodecId::Other,
    }
}

/// Translate a symphonia time base, falling back to milliseconds.
#[must_use]
pub fn map_time_base(time_base: Option<TimeBase>) -> URational {
    time_base.map_or(DEFAULT_TIME_BASE, |tb| {
        URational::new(tb.numer.get(), tb.denom.get())
    })
}

/// Convert a tick count in `time_base` units to a [`Duration`].
///
/// `None` for a zero denominator or a span no `Duration` can hold.
#[must_use]
pub fn ticks_to_duration(ticks: u64, time_base: URational) -> Option<Duration> {
    if time_base.denominator == 0 {
        return None;
    }
    let seconds = ticks as f64 * f64::from(time_base.numerator) / f64::from(time_base.denominator);
    Duration::try_from_secs_f64(seconds).ok()
}

/// Translate a symphonia packet, deriving the keyframe flag from the
/// bitstream.
///
/// `codec` and `config` describe the packet's track; pass `None` for a
/// non-video track, whose keyframe flag is left false. symphonia exposes no
/// sync-sample flag, and the bitstream is the authoritative source anyway.
#[must_use]
pub fn map_packet(packet: SymPacket, video: Option<(VideoCodecId, &CodecConfig)>) -> Packet {
    let data = packet.data.into_vec();
    let is_keyframe =
        video.is_some_and(|(codec, config)| nal::is_random_access_point(codec, config, &data));

    let mut out = Packet::new(TrackId::new(packet.track_id), data);
    out.pts = Some(packet.pts.get());
    out.dts = Some(packet.dts.get());
    out.duration = Some(packet.dur.get());
    out.is_keyframe = is_keyframe;
    out
}

/// Translate a `next_packet` result into rawshift's end-of-stream convention.
///
/// The two demuxers disagree about how a stream ends: the ISOBMFF reader
/// returns `Ok(None)`, while the Matroska reader surfaces an
/// `UnexpectedEof` I/O error. Both mean the same thing, and a caller looping
/// until `Ok(None)` must not see one of them as a failure.
///
/// # Errors
///
/// Any genuine container or I/O failure, as [`VideoError`].
pub fn map_packet_result(
    container: &'static str,
    result: Result<Option<SymPacket>, SymError>,
    video: Option<(VideoCodecId, &CodecConfig)>,
) -> VideoResult<Option<Packet>> {
    match result {
        Ok(Some(packet)) => Ok(Some(map_packet(packet, video))),
        Ok(None) => Ok(None),
        // End of stream, spelled as an error by some readers.
        Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(VideoError::container(container, e)),
    }
}

/// Translate a seek request.
///
/// # Errors
///
/// [`VideoError::Container`] for a `#[non_exhaustive]` target this bridge does
/// not know how to express, which is reported rather than silently
/// approximated to a different position.
pub fn map_seek(
    container: &'static str,
    track: TrackId,
    to: SeekTo,
    mode: SeekMode,
) -> VideoResult<(SymSeekMode, SymSeekTo)> {
    let target = match to {
        SeekTo::Timestamp(ts) => SymSeekTo::Timestamp {
            ts: Timestamp::new(ts),
            track_id: track.get(),
        },
        SeekTo::Time(time) => SymSeekTo::Time {
            time: Time::try_from_nanos_u128(time.as_nanos()).unwrap_or(Time::ZERO),
            track_id: Some(track.get()),
        },
        other => {
            return Err(VideoError::container(
                container,
                format!("unsupported seek target: {other:?}"),
            ));
        }
    };

    let sym_mode = match mode {
        // symphonia's "Accurate" means "at or before the request", which is
        // what rawshift calls Precise.
        SeekMode::Precise => SymSeekMode::Accurate,
        SeekMode::Coarse => SymSeekMode::Coarse,
        // SeekMode is #[non_exhaustive]; an unknown mode takes the
        // conservative option, which is always safe to resume decoding from.
        _ => SymSeekMode::Accurate,
    };

    Ok((sym_mode, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_time_base_falls_back_to_milliseconds() {
        assert_eq!(map_time_base(None), URational::new(1, 1000));
    }

    #[test]
    fn durations_convert_against_the_time_base() {
        // 90000 ticks of a 90 kHz base is one second.
        assert_eq!(
            ticks_to_duration(90_000, URational::new(1, 90_000)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            ticks_to_duration(1_500, URational::new(1, 1_000)),
            Some(Duration::from_millis(1_500))
        );
    }

    #[test]
    fn a_zero_denominator_does_not_divide_by_zero() {
        assert_eq!(ticks_to_duration(1_000, URational::new(1, 0)), None);
    }

    #[test]
    fn the_config_record_is_selected_by_codec_not_position() {
        // An HEVC track carrying a Dolby Vision blob first must still yield
        // the hvcC, not whatever sits at index zero.
        let extra = vec![
            VideoExtraData {
                id: vxd::VIDEO_EXTRA_DATA_ID_DOLBY_VISION_CONFIG,
                data: vec![0xDD; 4].into_boxed_slice(),
            },
            VideoExtraData {
                id: vxd::VIDEO_EXTRA_DATA_ID_HEVC_DECODER_CONFIG,
                data: vec![0xAA; 4].into_boxed_slice(),
            },
        ];
        assert_eq!(config_record(VideoCodecId::Hevc, &extra), vec![0xAA; 4]);
    }

    #[test]
    fn a_track_with_no_matching_record_yields_an_empty_one() {
        let extra = vec![VideoExtraData {
            id: vxd::VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG,
            data: vec![0x01; 4].into_boxed_slice(),
        }];
        // Asking for HEVC when only an avcC is present must not return it.
        assert!(config_record(VideoCodecId::Hevc, &extra).is_empty());
        assert!(config_record(VideoCodecId::ProRes, &extra).is_empty());
    }

    #[test]
    fn known_codecs_map_and_unknown_ones_degrade_to_other() {
        assert_eq!(map_video_codec(vwk::CODEC_ID_H264), VideoCodecId::H264);
        assert_eq!(map_video_codec(vwk::CODEC_ID_HEVC), VideoCodecId::Hevc);
        assert_eq!(map_video_codec(vwk::CODEC_ID_AV1), VideoCodecId::Av1);
        assert_eq!(map_video_codec(vwk::CODEC_ID_VP9), VideoCodecId::Vp9);
        assert_eq!(map_video_codec(vwk::CODEC_ID_MJPEG), VideoCodecId::Other);
    }

    #[test]
    fn end_of_stream_is_ok_none_however_the_reader_spells_it() {
        // The ISOBMFF reader says Ok(None); the Matroska reader raises
        // UnexpectedEof. A caller looping until Ok(None) must not see the
        // second as a failure.
        let explicit: Result<Option<SymPacket>, SymError> = Ok(None);
        assert!(matches!(map_packet_result("MP4", explicit, None), Ok(None)));

        let as_error: Result<Option<SymPacket>, SymError> = Err(SymError::IoError(
            std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
        ));
        assert!(matches!(
            map_packet_result("Matroska", as_error, None),
            Ok(None)
        ));
    }

    #[test]
    fn a_genuine_io_failure_is_still_an_error() {
        // Only UnexpectedEof means end of stream; a permission error must not
        // be silently swallowed into a clean end.
        let denied: Result<Option<SymPacket>, SymError> = Err(SymError::IoError(
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        ));
        assert!(map_packet_result("MP4", denied, None).is_err());

        let malformed: Result<Option<SymPacket>, SymError> = Err(SymError::DecodeError("bad atom"));
        assert!(map_packet_result("MP4", malformed, None).is_err());
    }

    #[test]
    fn seek_modes_translate_with_precise_meaning_at_or_before() {
        let (mode, _) = map_seek(
            "MP4",
            TrackId::new(1),
            SeekTo::Timestamp(0),
            SeekMode::Precise,
        )
        .expect("precise");
        assert_eq!(mode, SymSeekMode::Accurate);

        let (mode, _) = map_seek(
            "MP4",
            TrackId::new(1),
            SeekTo::Timestamp(0),
            SeekMode::Coarse,
        )
        .expect("coarse");
        assert_eq!(mode, SymSeekMode::Coarse);
    }

    #[test]
    fn a_time_seek_carries_sub_second_precision() {
        let (_, target) = map_seek(
            "MP4",
            TrackId::new(3),
            SeekTo::Time(Duration::from_millis(1_500)),
            SeekMode::Precise,
        )
        .expect("time seek");

        match target {
            SymSeekTo::Time { time, track_id } => {
                assert_eq!(track_id, Some(3));
                // 1.5 s must survive as 1.5 s, not truncate to 1 s.
                assert!((time.as_secs_f64() - 1.5).abs() < 1e-9, "{time:?}");
            }
            _ => panic!("expected a time seek"),
        }
    }

    #[test]
    fn a_timestamp_seek_is_relative_to_the_named_track() {
        let (_, target) = map_seek(
            "MKV",
            TrackId::new(7),
            SeekTo::Timestamp(4_242),
            SeekMode::Precise,
        )
        .expect("timestamp seek");

        match target {
            SymSeekTo::Timestamp { ts, track_id } => {
                assert_eq!(ts.get(), 4_242);
                assert_eq!(track_id, 7);
            }
            _ => panic!("expected a timestamp seek"),
        }
    }
}
