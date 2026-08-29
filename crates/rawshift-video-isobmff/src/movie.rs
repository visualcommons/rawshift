//! Reading the movie header, and mapping its tracks onto rawshift's model.

use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

use mp4_atom::{Atom, Codec, Colr, Encode, Ftyp, Header, Moov, ReadAtom, ReadFrom, Trak};

use rawshift_core::metadata::URational;
use rawshift_core::{Dimensions, MetadataNamespace, MetadataValue};
use rawshift_video_core::{
    AudioCodecId, AudioTrack, CicpColor, CodecConfig, ContainerId, OtherTrack, Rotation, Track,
    TrackId, TrackKind, VideoCodecId, VideoError, VideoMetadata, VideoResult, VideoTrack,
};

use crate::index::SampleIndex;

/// The name that appears in [`VideoError::Container`] messages.
pub const CONTAINER_NAME: &str = "MP4";

/// The movie header, and everything derived from it.
pub struct Movie {
    /// One entry per track, in the file's own order.
    pub tracks: Vec<Track>,
    /// Sample indices, parallel to `tracks`.
    pub indices: Vec<SampleIndex>,
    /// The container flavour.
    pub container: ContainerId,
    /// File-level metadata.
    pub metadata: VideoMetadata,
}

impl Movie {
    /// Read `ftyp` and `moov`, and build the tracks and their sample indices.
    ///
    /// Walks the top-level atoms by header so that `mdat` — which is the whole
    /// media payload and routinely gigabytes — is skipped rather than read
    /// into memory.
    ///
    /// # Errors
    ///
    /// [`VideoError::Container`] when no `moov` is present or an atom is
    /// malformed, and [`VideoError::Io`] for a reader failure.
    pub fn read<R: Read + Seek>(reader: &mut R) -> VideoResult<Self> {
        let end = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;

        let mut ftyp: Option<Ftyp> = None;
        let mut moov: Option<Moov> = None;

        while reader.stream_position()? < end {
            let header = match Header::read_from(reader) {
                Ok(header) => header,
                // A trailing partial atom is where damaged files end; stop
                // rather than failing a file whose moov we may already have.
                Err(_) => break,
            };

            match header.kind {
                Ftyp::KIND => {
                    ftyp = Ftyp::read_atom(&header, reader).ok();
                }
                Moov::KIND => {
                    moov = Some(
                        Moov::read_atom(&header, reader)
                            .map_err(|e| VideoError::container(CONTAINER_NAME, e))?,
                    );
                }
                _ => {
                    // Skip the body. An atom with no stated size runs to the
                    // end of the file, so there is nothing after it.
                    match header.size {
                        Some(size) => {
                            reader.seek(SeekFrom::Current(size as i64))?;
                        }
                        None => break,
                    }
                }
            }

            if ftyp.is_some() && moov.is_some() {
                break;
            }
        }

        let Some(moov) = moov else {
            return Err(VideoError::container(
                CONTAINER_NAME,
                "no moov atom: not a playable ISOBMFF file",
            ));
        };

        // Fragmented files carry their sample tables in moof boxes rather than
        // in stbl, so the index built here would be empty. Say so plainly
        // instead of presenting a track with no samples.
        if moov.mvex.is_some() {
            return Err(VideoError::container(
                CONTAINER_NAME,
                "fragmented MP4 (mvex present) is not supported yet",
            ));
        }

        let container = container_of(ftyp.as_ref());
        let mut tracks = Vec::with_capacity(moov.trak.len());
        let mut indices = Vec::with_capacity(moov.trak.len());

        for trak in &moov.trak {
            tracks.push(map_track(trak));
            indices.push(SampleIndex::build(&trak.mdia.minf.stbl, trak.edts.as_ref()));
        }

        let metadata = build_metadata(&moov, ftyp.as_ref(), container);

        Ok(Self {
            tracks,
            indices,
            container,
            metadata,
        })
    }
}

/// The container flavour implied by the `ftyp` brands.
///
/// The QuickTime brand `qt  ` means [`ContainerId::Mov`]; anything else with a
/// `ftyp` is MP4. A file with no `ftyp` is old-style QuickTime.
fn container_of(ftyp: Option<&Ftyp>) -> ContainerId {
    let Some(ftyp) = ftyp else {
        return ContainerId::Mov;
    };
    let quicktime = ftyp.major_brand.as_ref() == b"qt  "
        || ftyp.compatible_brands.iter().any(|b| b.as_ref() == b"qt  ");
    if quicktime {
        ContainerId::Mov
    } else {
        ContainerId::Mp4
    }
}

fn map_track(trak: &Trak) -> Track {
    let id = TrackId::new(trak.tkhd.track_id);
    let mdhd = &trak.mdia.mdhd;
    // The media timescale is the unit of this track's timestamps.
    let time_base = URational::new(1, mdhd.timescale.max(1));
    let duration = ticks_to_duration(mdhd.duration, time_base);
    let language = (!mdhd.language.is_empty()).then(|| mdhd.language.clone());

    let stbl = &trak.mdia.minf.stbl;
    let Some(codec) = stbl.stsd.codecs.first() else {
        let mut other = OtherTrack::new(id, TrackKind::Data, time_base);
        other.duration = duration;
        other.language = language;
        return Track::Other(other);
    };

    match codec {
        Codec::Avc1(_) | Codec::Hvc1(_) | Codec::Hev1(_) | Codec::Av01(_) | Codec::Vp09(_) => {
            let mut video = map_video_track(id, codec, time_base);
            video.duration = duration;
            video.frame_count = Some(stbl_sample_count(stbl));
            video.rotation = rotation_of(trak);
            Track::Video(video)
        }
        Codec::Mp4a(_) | Codec::Opus(_) | Codec::Flac(_) | Codec::Ac3(_) | Codec::Eac3(_) => {
            let mut audio = AudioTrack::new(id, audio_codec_of(codec), time_base);
            audio.duration = duration;
            audio.language = language;
            Track::Audio(audio)
        }
        _ => {
            let mut other = OtherTrack::new(id, TrackKind::Data, time_base);
            other.duration = duration;
            other.language = language;
            Track::Other(other)
        }
    }
}

fn map_video_track(id: TrackId, codec: &Codec, time_base: URational) -> VideoTrack {
    let (video_codec, visual, colr, config) = match codec {
        Codec::Avc1(a) => (
            VideoCodecId::H264,
            &a.visual,
            a.colr.as_ref(),
            encode_body(&a.avcc),
        ),
        Codec::Hvc1(h) => (
            VideoCodecId::Hevc,
            &h.visual,
            h.colr.as_ref(),
            encode_body(&h.hvcc),
        ),
        Codec::Hev1(h) => (
            VideoCodecId::Hevc,
            &h.visual,
            h.colr.as_ref(),
            encode_body(&h.hvcc),
        ),
        Codec::Av01(a) => (
            VideoCodecId::Av1,
            &a.visual,
            a.colr.as_ref(),
            encode_body(&a.av1c),
        ),
        Codec::Vp09(v) => (VideoCodecId::Vp9, &v.visual, None, Vec::new()),
        // map_track only routes video sample entries here.
        _ => unreachable!("map_video_track called with a non-video sample entry"),
    };

    let mut track = VideoTrack::new(
        id,
        video_codec,
        Dimensions {
            width: u32::from(visual.width),
            height: u32::from(visual.height),
        },
        time_base,
    );
    track.color = colour_of(colr);
    track.codec_config = CodecConfig::new(config);
    track
}

/// Re-encode a configuration atom back to its record body.
///
/// The box layer parses `avcC`/`hvcC` into fields, but a decoder wants the
/// record exactly as the file stored it — that is what a hardware decoder's
/// configuration input is defined in terms of. Encoding the body back is the
/// faithful round trip, and it keeps rawshift from having to re-serialise the
/// record itself.
fn encode_body<A: Atom>(atom: &A) -> Vec<u8> {
    let mut buf = Vec::new();
    // Encoding into a Vec cannot fail for these small, fully-owned atoms.
    if atom.encode(&mut buf).is_err() {
        return Vec::new();
    }
    // `encode` writes the full box; the record body is what follows the
    // 8-byte size and type header.
    buf.split_off(8.min(buf.len()))
}

/// Translate a `colr` box into rawshift's colour signalling.
///
/// `nclx` carries the full CICP triple plus the range flag; the older `nclc`
/// has no range flag, and studio range is the correct reading for it. An
/// absent or ICC-profile-only `colr` leaves everything unspecified, which
/// resolves by picture height at decode time.
fn colour_of(colr: Option<&Colr>) -> CicpColor {
    match colr {
        Some(Colr::Nclx {
            colour_primaries,
            transfer_characteristics,
            matrix_coefficients,
            full_range_flag,
        }) => CicpColor {
            primaries: *colour_primaries,
            transfer: *transfer_characteristics,
            matrix: *matrix_coefficients,
            full_range: *full_range_flag,
        },
        Some(Colr::Nclc {
            colour_primaries,
            transfer_characteristics,
            matrix_coefficients,
        }) => CicpColor {
            primaries: *colour_primaries,
            transfer: *transfer_characteristics,
            matrix: *matrix_coefficients,
            full_range: false,
        },
        _ => CicpColor::UNSPECIFIED,
    }
}

/// Recover display rotation from the `tkhd` matrix.
///
/// The matrix values are 16.16 fixed point; only `a`, `b`, `c`, `d` carry
/// rotation.
fn rotation_of(trak: &Trak) -> Rotation {
    let m = &trak.tkhd.matrix;
    let fixed = |v: i32| f64::from(v) / 65536.0;
    Rotation::from_display_matrix(fixed(m.a), fixed(m.b), fixed(m.c), fixed(m.d))
}

fn audio_codec_of(codec: &Codec) -> AudioCodecId {
    match codec {
        Codec::Mp4a(_) => AudioCodecId::Aac,
        Codec::Opus(_) => AudioCodecId::Opus,
        Codec::Flac(_) => AudioCodecId::Flac,
        Codec::Ac3(_) | Codec::Eac3(_) => AudioCodecId::Ac3,
        _ => AudioCodecId::Other,
    }
}

fn stbl_sample_count(stbl: &mp4_atom::Stbl) -> u64 {
    match &stbl.stsz.samples {
        mp4_atom::StszSamples::Identical { count, .. } => u64::from(*count),
        mp4_atom::StszSamples::Different { sizes } => sizes.len() as u64,
    }
}

/// Convert a tick count in `time_base` units to a [`Duration`].
pub fn ticks_to_duration(ticks: u64, time_base: URational) -> Option<Duration> {
    if time_base.denominator == 0 {
        return None;
    }
    let seconds = ticks as f64 * f64::from(time_base.numerator) / f64::from(time_base.denominator);
    Duration::try_from_secs_f64(seconds).ok()
}

fn build_metadata(moov: &Moov, ftyp: Option<&Ftyp>, container: ContainerId) -> VideoMetadata {
    let mut md = VideoMetadata::default();
    let mvhd = &moov.mvhd;

    md.container.container = Some(container);
    md.container.timescale = Some(mvhd.timescale);
    md.container.duration =
        ticks_to_duration(mvhd.duration, URational::new(1, mvhd.timescale.max(1)));
    md.container.creation_time = Some(format_iso_time(mvhd.creation_time));
    md.container.modification_time = Some(format_iso_time(mvhd.modification_time));

    if let Some(ftyp) = ftyp {
        md.container.brands = std::iter::once(&ftyp.major_brand)
            .chain(ftyp.compatible_brands.iter())
            .map(|b| brand_string(b.as_ref()))
            .collect();
    }

    for brand in md.container.brands.clone() {
        md.push_container_entry(
            MetadataNamespace::Quicktime,
            "ftyp.brand",
            MetadataValue::Text(brand),
        );
    }
    md.push_container_entry(
        MetadataNamespace::Quicktime,
        "mvhd.timescale",
        MetadataValue::U64(u64::from(mvhd.timescale)),
    );

    md
}

/// Render a four-character brand, replacing non-printable bytes.
///
/// A crafted brand must not smuggle control characters into a log line or an
/// error message.
fn brand_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '?'
            }
        })
        .collect()
}

/// Render an ISOBMFF timestamp as an ISO 8601 date-time in UTC.
///
/// ISOBMFF counts seconds from 1904-01-01, not the Unix epoch. Formatted here
/// rather than by pulling in a date library for one field.
pub fn format_iso_time(seconds_since_1904: u64) -> String {
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
    use mp4_atom::FourCC;

    #[test]
    fn the_isobmff_epoch_is_offset_from_unix_not_equal_to_it() {
        assert_eq!(format_iso_time(2_082_844_800), "1970-01-01T00:00:00Z");
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
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
    }

    #[test]
    fn the_quicktime_brand_selects_mov_from_either_position() {
        let major = Ftyp {
            major_brand: FourCC::new(b"qt  "),
            minor_version: 0,
            compatible_brands: vec![],
        };
        assert_eq!(container_of(Some(&major)), ContainerId::Mov);

        let compatible = Ftyp {
            major_brand: FourCC::new(b"isom"),
            minor_version: 0,
            compatible_brands: vec![FourCC::new(b"qt  ")],
        };
        assert_eq!(container_of(Some(&compatible)), ContainerId::Mov);
    }

    #[test]
    fn other_brands_are_mp4_and_a_missing_ftyp_is_legacy_quicktime() {
        let mp4 = Ftyp {
            major_brand: FourCC::new(b"isom"),
            minor_version: 512,
            compatible_brands: vec![FourCC::new(b"mp41")],
        };
        assert_eq!(container_of(Some(&mp4)), ContainerId::Mp4);
        assert_eq!(container_of(None), ContainerId::Mov);
    }

    #[test]
    fn nclx_colour_carries_the_full_cicp_triple_and_range() {
        let colr = Colr::Nclx {
            colour_primaries: 9,
            transfer_characteristics: 16,
            matrix_coefficients: 9,
            full_range_flag: true,
        };
        let c = colour_of(Some(&colr));
        assert_eq!((c.primaries, c.transfer, c.matrix), (9, 16, 9));
        assert!(c.full_range, "the range flag must survive");
    }

    #[test]
    fn nclc_colour_has_no_range_flag_and_reads_as_studio() {
        let colr = Colr::Nclc {
            colour_primaries: 1,
            transfer_characteristics: 1,
            matrix_coefficients: 1,
        };
        let c = colour_of(Some(&colr));
        assert_eq!(c.primaries, 1);
        assert!(
            !c.full_range,
            "nclc has no range flag; studio is the reading"
        );
    }

    #[test]
    fn an_absent_or_icc_only_colr_leaves_colour_unspecified() {
        assert_eq!(colour_of(None), CicpColor::UNSPECIFIED);
        let icc = Colr::Prof {
            profile: vec![0u8; 16],
        };
        assert_eq!(colour_of(Some(&icc)), CicpColor::UNSPECIFIED);
    }

    #[test]
    fn brands_are_sanitised_of_control_bytes() {
        assert_eq!(brand_string(b"isom"), "isom");
        assert_eq!(brand_string(b"qt  "), "qt  ");
        assert_eq!(brand_string(&[0x00, 0x1b, b'[', b'A']), "??[A");
    }
}
