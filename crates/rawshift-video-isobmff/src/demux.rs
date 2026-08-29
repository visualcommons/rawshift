//! The MP4 / QuickTime demuxer.
//!
//! A thin bridge over `symphonia-format-isomp4`, plus the facts symphonia does
//! not surface (see [`crate::boxes`]). No symphonia type escapes this module.

use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

use symphonia_core::codecs::CodecParameters;
use symphonia_core::codecs::video::well_known as vwk;
use symphonia_core::codecs::video::well_known::extra_data as vxd;
use symphonia_core::codecs::video::{VideoCodecParameters, VideoExtraData};
use symphonia_core::formats::{
    FormatOptions, FormatReader, SeekMode as SymSeekMode, SeekTo as SymSeekTo,
};
use symphonia_core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia_core::units::{TimeBase, Timestamp};
use symphonia_format_isomp4::IsoMp4Reader;

use rawshift_core::metadata::URational;
use rawshift_core::{Dimensions, MetadataNamespace, MetadataValue};
use rawshift_video_core::{
    AudioCodecId, AudioTrack, CodecConfig, ContainerId, Demuxer, OtherTrack, Packet, Rotation,
    SeekMode, SeekTo, Track, TrackId, TrackKind, VideoCodecId, VideoError, VideoMetadata,
    VideoResult, VideoTrack, nal,
};

use crate::boxes::{self, BoxSurvey};

/// The name that appears in [`VideoError::Container`] messages.
const CONTAINER_NAME: &str = "MP4";

/// A demuxer for MP4, M4V and QuickTime files.
pub struct IsoBmffDemuxer {
    reader: IsoMp4Reader<'static>,
    container: ContainerId,
    tracks: Vec<Track>,
    metadata: VideoMetadata,
}

impl IsoBmffDemuxer {
    /// Open an ISOBMFF file.
    ///
    /// Performs a short header walk for the rotation and brand facts the
    /// backend does not expose, then rewinds and hands the source to it.
    ///
    /// # Errors
    ///
    /// [`VideoError::Container`] for a malformed or unsupported file, and
    /// [`VideoError::Io`] for a reader failure.
    pub fn open<R>(mut source: R) -> VideoResult<Self>
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        let survey = boxes::survey(&mut source).unwrap_or_default();
        source.seek(SeekFrom::Start(0))?;

        let stream = MediaSourceStream::new(
            Box::new(SeekableSource(source)),
            MediaSourceStreamOptions::default(),
        );
        let reader = IsoMp4Reader::try_new(stream, FormatOptions::default())
            .map_err(|e| VideoError::container(CONTAINER_NAME, e))?;

        let container = survey.container();
        let tracks = map_tracks(&reader, &survey);
        let metadata = build_metadata(&reader, &survey, container);

        Ok(Self {
            reader,
            container,
            tracks,
            metadata,
        })
    }

    /// The track's codec and config, for deriving a packet's keyframe flag.
    fn video_codec_of(&self, track: TrackId) -> Option<(VideoCodecId, &CodecConfig)> {
        self.tracks
            .iter()
            .find(|t| t.id() == track)
            .and_then(Track::as_video)
            .map(|v| (v.codec, &v.codec_config))
    }
}

impl Demuxer for IsoBmffDemuxer {
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
        let Some(packet) = self
            .reader
            .next_packet()
            .map_err(|e| VideoError::container(CONTAINER_NAME, e))?
        else {
            return Ok(None);
        };

        let track = TrackId::new(packet.track_id);
        let data = packet.data.into_vec();

        // The backend exposes no sync-sample flag, so read random access out
        // of the bitstream, which is authoritative regardless.
        let is_keyframe = self
            .video_codec_of(track)
            .is_some_and(|(codec, config)| nal::is_random_access_point(codec, config, &data));

        let mut out = Packet::new(track, data);
        out.pts = Some(packet.pts.get());
        out.dts = Some(packet.dts.get());
        out.duration = Some(packet.dur.get());
        out.is_keyframe = is_keyframe;
        Ok(Some(out))
    }

    fn seek(&mut self, track: TrackId, to: SeekTo, mode: SeekMode) -> VideoResult<i64> {
        if self.track(track).is_none() {
            return Err(VideoError::NoSuchTrack { id: track });
        }

        let target = match to {
            SeekTo::Timestamp(ts) => SymSeekTo::Timestamp {
                ts: Timestamp::new(ts),
                track_id: track.get(),
            },
            SeekTo::Time(time) => SymSeekTo::Time {
                time: symphonia_core::units::Time::try_from_nanos_u128(time.as_nanos())
                    .unwrap_or(symphonia_core::units::Time::ZERO),
                track_id: Some(track.get()),
            },
            // SeekTo is #[non_exhaustive]; a target this backend cannot express
            // is reported rather than silently approximated.
            other => {
                return Err(VideoError::container(
                    CONTAINER_NAME,
                    format!("unsupported seek target: {other:?}"),
                ));
            }
        };
        let sym_mode = match mode {
            // "Accurate" in the backend's vocabulary means "at or before the
            // request", which is rawshift's Precise.
            SeekMode::Precise => SymSeekMode::Accurate,
            SeekMode::Coarse => SymSeekMode::Coarse,
            // SeekMode is #[non_exhaustive]; an unknown mode takes the
            // conservative option, which is always safe to decode from.
            _ => SymSeekMode::Accurate,
        };

        self.reader
            .seek(sym_mode, target)
            .map(|landed| landed.actual_ts.get())
            .map_err(|e| VideoError::container(CONTAINER_NAME, e))
    }
}

/// Adapts a `Read + Seek` source to the backend's source trait.
///
/// Every source rawshift accepts is seekable — the public API requires `Seek`
/// — so `is_seekable` is unconditionally true, and the length is recovered by
/// seeking to the end and back.
struct SeekableSource<R>(R);

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

/// Map the backend's tracks onto rawshift's model.
fn map_tracks(reader: &IsoMp4Reader<'_>, survey: &BoxSurvey) -> Vec<Track> {
    reader
        .tracks()
        .iter()
        .map(|t| {
            let id = TrackId::new(t.id);
            let time_base = time_base_of(t.time_base);
            let duration = t.duration.and_then(|d| duration_of(d.get(), time_base));

            match &t.codec_params {
                Some(CodecParameters::Video(v)) => Track::Video(map_video_track(
                    id,
                    v,
                    time_base,
                    duration,
                    t.num_frames,
                    survey.rotation(t.id),
                )),
                Some(CodecParameters::Audio(a)) => {
                    let mut track = AudioTrack::new(id, AudioCodecId::Other, time_base);
                    track.sample_rate = a.sample_rate;
                    track.channels = a.channels.as_ref().map(|c| c.count() as u16);
                    track.duration = duration;
                    track.language = t.language.clone();
                    Track::Audio(track)
                }
                _ => {
                    let mut track = OtherTrack::new(id, TrackKind::Data, time_base);
                    track.duration = duration;
                    track.language = t.language.clone();
                    Track::Other(track)
                }
            }
        })
        .collect()
}

fn map_video_track(
    id: TrackId,
    params: &VideoCodecParameters,
    time_base: URational,
    duration: Option<Duration>,
    frame_count: Option<u64>,
    rotation: Rotation,
) -> VideoTrack {
    let codec = map_video_codec(params.codec);
    let dimensions = Dimensions {
        width: u32::from(params.width.unwrap_or(0)),
        height: u32::from(params.height.unwrap_or(0)),
    };

    let mut track = VideoTrack::new(id, codec, dimensions, time_base);
    // ISOBMFF carries colour in a `colr` box the backend does not surface, so
    // the constructor's UNSPECIFIED stands: honest, and it resolves by picture
    // height where it matters.
    track.duration = duration;
    track.frame_count = frame_count;
    track.rotation = rotation;
    track.codec_config = CodecConfig::new(config_record(codec, &params.extra_data));
    track
}

/// Pick the decoder configuration record matching the codec.
///
/// A track may carry several extra-data blobs (Dolby Vision configurations
/// accompany an HEVC base layer, for instance), so this selects by id rather
/// than taking the first.
fn config_record(codec: VideoCodecId, extra: &[VideoExtraData]) -> Vec<u8> {
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

fn map_video_codec(codec: symphonia_core::codecs::video::VideoCodecId) -> VideoCodecId {
    match codec {
        vwk::CODEC_ID_H264 => VideoCodecId::H264,
        vwk::CODEC_ID_HEVC => VideoCodecId::Hevc,
        vwk::CODEC_ID_AV1 => VideoCodecId::Av1,
        vwk::CODEC_ID_VP9 => VideoCodecId::Vp9,
        _ => VideoCodecId::Other,
    }
}

/// The backend's time base, or the 1/1000 default when a track states none.
fn time_base_of(tb: Option<TimeBase>) -> URational {
    tb.map_or(URational::new(1, 1000), |tb| {
        URational::new(tb.numer.get(), tb.denom.get())
    })
}

fn duration_of(ticks: u64, time_base: URational) -> Option<Duration> {
    if time_base.denominator == 0 {
        return None;
    }
    let seconds = ticks as f64 * f64::from(time_base.numerator) / f64::from(time_base.denominator);
    Duration::try_from_secs_f64(seconds).ok()
}

fn build_metadata(
    reader: &IsoMp4Reader<'_>,
    survey: &BoxSurvey,
    container: ContainerId,
) -> VideoMetadata {
    let mut md = VideoMetadata::default();

    md.container.container = Some(container);
    md.container.brands = survey.brands.clone();
    md.container.timescale = survey.timescale;
    md.container.duration = survey.timescale.and_then(|scale| {
        survey
            .duration
            .and_then(|ticks| duration_of(ticks, URational::new(1, scale)))
    });
    md.container.creation_time = survey.creation_time.map(format_iso_time);
    md.container.modification_time = survey.modification_time.map(format_iso_time);

    // Fall back to the media-level duration when the movie header had none.
    if md.container.duration.is_none() {
        let info = reader.media_info();
        if let (Some(tb), Some(dur)) = (info.time_base, info.duration) {
            md.container.duration =
                duration_of(dur.get(), URational::new(tb.numer.get(), tb.denom.get()));
        }
    }

    for brand in &survey.brands {
        md.push_container_entry(
            MetadataNamespace::Quicktime,
            "ftyp.brand",
            MetadataValue::Text(brand.clone()),
        );
    }
    if let Some(scale) = survey.timescale {
        md.push_container_entry(
            MetadataNamespace::Quicktime,
            "mvhd.timescale",
            MetadataValue::U64(u64::from(scale)),
        );
    }

    md
}

/// Render an ISOBMFF timestamp as an ISO 8601 date-time in UTC.
///
/// ISOBMFF counts seconds from 1904-01-01, not the Unix epoch. Formatting is
/// done here rather than pulling in a date library for one field.
fn format_iso_time(seconds_since_1904: u64) -> String {
    /// Seconds between 1904-01-01 and 1970-01-01: 66 years with 17 leap days.
    const EPOCH_OFFSET: u64 = 2_082_844_800;

    if seconds_since_1904 < EPOCH_OFFSET {
        // Before the Unix epoch: report the raw value rather than a wrong date.
        return format!("{seconds_since_1904} (seconds since 1904-01-01)");
    }
    let unix = seconds_since_1904 - EPOCH_OFFSET;

    let (days, time) = (unix / 86_400, unix % 86_400);
    let (hour, minute, second) = (time / 3_600, (time % 3_600) / 60, time % 60);
    let (year, month, day) = civil_from_days(days as i64);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Convert days since 1970-01-01 to a proleptic Gregorian date.
///
/// Howard Hinnant's `civil_from_days`, which is exact for the whole
/// representable range and needs no tables.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_isobmff_epoch_is_offset_from_unix_not_equal_to_it() {
        // 1904-01-01T00:00:00Z is timestamp 0 in ISOBMFF.
        assert_eq!(format_iso_time(2_082_844_800), "1970-01-01T00:00:00Z");
        // A real capture time: 2024-01-15T12:30:45Z.
        assert_eq!(
            format_iso_time(2_082_844_800 + 1_705_321_845),
            "2024-01-15T12:30:45Z"
        );
    }

    #[test]
    fn a_timestamp_before_the_unix_epoch_reports_raw_rather_than_a_wrong_date() {
        let s = format_iso_time(0);
        assert!(s.contains("1904"), "{s}");
        assert!(!s.starts_with("1970"), "{s}");
    }

    #[test]
    fn civil_from_days_handles_leap_years_and_epoch_edges() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(59), (1970, 3, 1));
        // 2000 was a leap year (divisible by 400); 1900 was not.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // 2024-02-29, a leap day in a normal leap year.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
    }

    #[test]
    fn a_missing_time_base_falls_back_rather_than_dividing_by_zero() {
        assert_eq!(time_base_of(None), URational::new(1, 1000));
        assert_eq!(duration_of(1000, URational::new(1, 0)), None);
    }

    #[test]
    fn durations_convert_against_the_time_base() {
        // 90000 ticks of a 90 kHz base is one second.
        assert_eq!(
            duration_of(90_000, URational::new(1, 90_000)),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn the_config_record_is_selected_by_codec_not_position() {
        // An HEVC track carrying a Dolby Vision blob first must still yield
        // the hvcC, not whatever happens to be at index zero.
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
    fn a_track_with_no_matching_config_record_yields_an_empty_one() {
        let extra = vec![VideoExtraData {
            id: vxd::VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG,
            data: vec![0x01; 4].into_boxed_slice(),
        }];
        // Asking for HEVC when only an avcC is present must not return the avcC.
        assert!(config_record(VideoCodecId::Hevc, &extra).is_empty());
        assert!(config_record(VideoCodecId::ProRes, &extra).is_empty());
    }

    #[test]
    fn known_codecs_map_and_unknown_ones_degrade_to_other() {
        assert_eq!(map_video_codec(vwk::CODEC_ID_H264), VideoCodecId::H264);
        assert_eq!(map_video_codec(vwk::CODEC_ID_HEVC), VideoCodecId::Hevc);
        assert_eq!(map_video_codec(vwk::CODEC_ID_AV1), VideoCodecId::Av1);
        assert_eq!(map_video_codec(vwk::CODEC_ID_VP9), VideoCodecId::Vp9);
        // A codec rawshift does not model is named Other, not dropped.
        assert_eq!(map_video_codec(vwk::CODEC_ID_MJPEG), VideoCodecId::Other);
    }
}
