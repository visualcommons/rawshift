//! The MP4 / QuickTime demuxer.
//!
//! Track, packet and seek translation is shared with the Matroska crate via
//! `rawshift-video-symphonia`; what is specific to ISOBMFF is the header walk
//! in [`crate::boxes`] and the movie-level metadata built from it. No
//! symphonia type escapes this module.

use std::io::{Read, Seek, SeekFrom};

use symphonia_core::formats::{FormatOptions, FormatReader};
use symphonia_core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia_format_isomp4::IsoMp4Reader;

use rawshift_core::metadata::URational;
use rawshift_core::{MetadataNamespace, MetadataValue};
use rawshift_video_core::{
    CodecConfig, ContainerId, Demuxer, Packet, SeekMode, SeekTo, Track, TrackId, VideoCodecId,
    VideoError, VideoMetadata, VideoResult,
};
use rawshift_video_symphonia as bridge;

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
        // A structurally odd file should still demux: the survey supplies
        // supplementary facts, so its failure degrades rather than propagates.
        let survey = boxes::survey(&mut source).unwrap_or_default();
        source.seek(SeekFrom::Start(0))?;

        let stream = MediaSourceStream::new(
            Box::new(bridge::SeekableSource::new(source)),
            MediaSourceStreamOptions::default(),
        );
        let reader = IsoMp4Reader::try_new(stream, FormatOptions::default())
            .map_err(|e| VideoError::container(CONTAINER_NAME, e))?;

        let container = survey.container();
        let tracks = reader
            .tracks()
            .iter()
            .map(|t| bridge::map_track(t, survey.rotation(t.id)))
            .collect();
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
            .and_then(|ticks| bridge::ticks_to_duration(ticks, URational::new(1, scale)))
    });
    md.container.creation_time = survey.creation_time.map(format_iso_time);
    md.container.modification_time = survey.modification_time.map(format_iso_time);

    // Fall back to the media-level duration when the movie header had none.
    if md.container.duration.is_none() {
        let info = reader.media_info();
        if let (Some(tb), Some(dur)) = (info.time_base, info.duration) {
            md.container.duration =
                bridge::ticks_to_duration(dur.get(), bridge::map_time_base(Some(tb)));
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
/// ISOBMFF counts seconds from 1904-01-01, not the Unix epoch. Formatted here
/// rather than by pulling in a date library for one field.
fn format_iso_time(seconds_since_1904: u64) -> String {
    /// Seconds between 1904-01-01 and 1970-01-01.
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
/// Howard Hinnant's `civil_from_days`: exact over the whole representable
/// range and table-free.
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
        // 2024-02-29, a leap day in an ordinary leap year.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
    }
}
