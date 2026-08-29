//! Demuxed packets and the seek vocabulary.

use std::time::Duration;

use rawshift_core::metadata::URational;

use crate::id::TrackId;

/// One demuxed access unit — the coded data for a single presentation time on
/// one track.
///
/// For H.264 and HEVC in MP4 or Matroska this is a **length-prefixed** NAL
/// stream, never Annex B; the prefix width comes from the track's
/// [`CodecConfig`](crate::track::CodecConfig).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Packet {
    /// The track this packet belongs to.
    pub track: TrackId,
    /// Presentation timestamp, in the track's
    /// [`time_base`](crate::track::VideoTrack::time_base) units.
    pub pts: Option<i64>,
    /// Decode timestamp, in the same units.
    ///
    /// Differs from `pts` exactly when the stream reorders. rawshift never
    /// orders output by `dts` — the bitstream's picture order is
    /// authoritative — so this is carried for diagnostics and for callers
    /// remuxing rather than decoding.
    pub dts: Option<i64>,
    /// How long this packet occupies, in the same units, when stated.
    pub duration: Option<u64>,
    /// Whether the container marks this as a sync sample (a random access
    /// point).
    ///
    /// Advisory. Containers get this wrong, and the bitstream is
    /// authoritative; a decoder uses it to find a starting point, not to
    /// decide how to decode.
    pub is_keyframe: bool,
    /// The coded bytes.
    pub data: Vec<u8>,
}

impl Packet {
    /// Convert a timestamp in `time_base` units to a [`Duration`].
    ///
    /// Returns `None` for an absent or negative timestamp: a `Duration` cannot
    /// be negative, and negative presentation times are real (an edit list can
    /// place a track's first samples before the presentation origin).
    #[must_use]
    pub fn timestamp_to_duration(timestamp: i64, time_base: URational) -> Option<Duration> {
        if timestamp < 0 || time_base.denominator == 0 {
            return None;
        }
        let seconds =
            timestamp as f64 * f64::from(time_base.numerator) / f64::from(time_base.denominator);
        Duration::try_from_secs_f64(seconds).ok()
    }

    /// This packet's presentation time, when it has one.
    #[must_use]
    pub fn presentation_time(&self, time_base: URational) -> Option<Duration> {
        self.pts
            .and_then(|pts| Self::timestamp_to_duration(pts, time_base))
    }
}

/// Where to seek to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SeekTo {
    /// A timestamp in the target track's `time_base` units.
    Timestamp(i64),
    /// A position from the start of the track.
    Time(Duration),
}

/// How precisely to seek.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SeekMode {
    /// Land at or before the request, on a random access point.
    ///
    /// The default, and the only mode that lets decoding resume: a decoder
    /// cannot start mid-GOP. Reaching the exact frame means decoding forward
    /// and discarding, which is the caller's choice to make.
    #[default]
    Precise,
    /// Land near the request, wherever the container's index is coarsest.
    ///
    /// May land *after* the requested position. For scrubbing, where latency
    /// matters more than landing before a specific frame.
    Coarse,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: URational = URational {
        numerator: 1,
        denominator: 1000,
    };

    #[test]
    fn timestamps_convert_against_the_time_base() {
        // 1500 ticks of 1/1000 s is 1.5 s.
        assert_eq!(
            Packet::timestamp_to_duration(1500, MS),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(Packet::timestamp_to_duration(0, MS), Some(Duration::ZERO));
    }

    #[test]
    fn negative_timestamps_have_no_duration() {
        // Edit lists legitimately place samples before the presentation
        // origin; a Duration cannot express that, so it is None rather than a
        // panic or a wrapped huge value.
        assert_eq!(Packet::timestamp_to_duration(-1, MS), None);
    }

    #[test]
    fn a_zero_denominator_time_base_does_not_divide_by_zero() {
        let bad = URational {
            numerator: 1,
            denominator: 0,
        };
        assert_eq!(Packet::timestamp_to_duration(1000, bad), None);
    }

    #[test]
    fn presentation_time_uses_pts_and_tolerates_its_absence() {
        let mut p = Packet {
            track: TrackId::new(1),
            pts: Some(90_000),
            dts: None,
            duration: None,
            is_keyframe: true,
            data: Vec::new(),
        };
        // A 90 kHz time base: 90000 ticks is one second.
        let hz90 = URational {
            numerator: 1,
            denominator: 90_000,
        };
        assert_eq!(p.presentation_time(hz90), Some(Duration::from_secs(1)));

        p.pts = None;
        assert_eq!(p.presentation_time(hz90), None);
    }

    #[test]
    fn precise_is_the_default_seek_mode() {
        assert_eq!(SeekMode::default(), SeekMode::Precise);
    }
}
