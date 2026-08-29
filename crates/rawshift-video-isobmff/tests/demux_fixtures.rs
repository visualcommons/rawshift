//! End-to-end demuxing of real files, checked against ffprobe.
//!
//! These assert the behaviours that unit tests over synthetic tables cannot:
//! that rawshift agrees with the rest of the world about a file ffmpeg wrote.
//! Every test skips cleanly when ffmpeg is unavailable.

mod common;

use std::fs::File;

use rawshift_video_core::{ContainerId, Demuxer, Track, VideoCodecId};
use rawshift_video_isobmff::IsoBmffDemuxer;

/// Skip the test when the fixture could not be generated.
macro_rules! fixture {
    ($maker:expr, $what:literal) => {
        match $maker {
            Some(path) => path,
            None => {
                eprintln!("skipping: cannot generate the {} fixture (ffmpeg missing or without the encoder)", $what);
                return;
            }
        }
    };
}

fn open(path: &std::path::Path) -> IsoBmffDemuxer<File> {
    IsoBmffDemuxer::open(File::open(path).expect("fixture is readable")).expect("file opens")
}

fn only_video_track(demuxer: &IsoBmffDemuxer<File>) -> &rawshift_video_core::VideoTrack {
    demuxer
        .tracks()
        .iter()
        .find_map(Track::as_video)
        .expect("the fixture has a video track")
}

#[test]
fn h264_in_mp4_reads_its_track_and_every_sample() {
    let path = fixture!(common::h264_mp4(), "H.264 MP4");
    let mut demuxer = open(&path);

    assert_eq!(demuxer.container(), ContainerId::Mp4);

    let video = only_video_track(&demuxer);
    assert_eq!(video.codec, VideoCodecId::H264);
    assert_eq!(video.dimensions.width, 128);
    assert_eq!(video.dimensions.height, 96);
    // An avcC must be present, or no decoder could be opened for the track.
    assert!(
        !video.codec_config.is_empty(),
        "the track must carry its avcC"
    );

    // 2 seconds at 25 fps.
    let mut packets = 0;
    while demuxer.next_packet().expect("packets read").is_some() {
        packets += 1;
    }
    assert_eq!(packets, 50, "every sample must be delivered exactly once");
}

#[test]
fn hevc_in_mp4_opens_despite_a_multi_kilobyte_hvcc() {
    // The regression this crate's rewrite exists for: the previous backend
    // capped hvcC at 1 KB, and every real HEVC file failed to open.
    let path = fixture!(common::hevc_mp4(), "HEVC MP4");
    let mut demuxer = open(&path);

    let video = only_video_track(&demuxer);
    assert_eq!(video.codec, VideoCodecId::Hevc);
    assert!(
        video.codec_config.as_bytes().len() > 1024,
        "this fixture's hvcC should exceed the 1 KB that used to be fatal, got {} bytes",
        video.codec_config.as_bytes().len()
    );

    let mut packets = 0;
    while demuxer.next_packet().expect("packets read").is_some() {
        packets += 1;
    }
    assert_eq!(packets, 50);
}

#[test]
fn quicktime_is_distinguished_from_mp4_by_its_brand() {
    let path = fixture!(common::h264_mov(), "H.264 MOV");
    let demuxer = open(&path);
    assert_eq!(demuxer.container(), ContainerId::Mov);
}

#[test]
fn presentation_timestamps_agree_with_ffprobe() {
    // The edit list must be applied: without it every timestamp carries the
    // encoder's reorder delay and rawshift disagrees with every other tool.
    let path = fixture!(common::h264_mp4(), "H.264 MP4");
    let mut demuxer = open(&path);

    let first = demuxer
        .next_packet()
        .expect("first packet")
        .expect("the file is not empty");

    let Some(expected) = common::ffprobe(&path, "packet=pts", &["-read_intervals", "%+#1"]) else {
        eprintln!("skipping the ffprobe comparison: ffprobe is unavailable");
        return;
    };
    let expected: i64 = expected.trim().parse().expect("ffprobe prints an integer");
    assert_eq!(
        first.pts,
        Some(expected),
        "rawshift's first PTS must match ffprobe's"
    );
}

#[test]
fn random_access_points_match_the_encoders_gop_length() {
    // Derived from the bitstream, not from the container's sync table. A
    // 50-frame clip with -g 12 has keyframes at 0, 12, 24, 36 and 48.
    let path = fixture!(common::h264_mp4(), "H.264 MP4");
    let mut demuxer = open(&path);

    let mut keyframes = 0;
    while let Some(packet) = demuxer.next_packet().expect("packets read") {
        if packet.is_keyframe {
            keyframes += 1;
        }
    }
    assert_eq!(keyframes, 5, "one per 12-frame GOP across 50 frames");
}

#[test]
fn packets_are_delivered_in_decode_order() {
    // Decode order is what a decoder must be fed; presentation order is
    // recovered afterwards from the timestamps.
    let path = fixture!(common::h264_mp4(), "H.264 MP4");
    let mut demuxer = open(&path);

    let mut previous = i64::MIN;
    while let Some(packet) = demuxer.next_packet().expect("packets read") {
        let dts = packet.dts.expect("MP4 samples always carry a decode time");
        assert!(dts >= previous, "decode timestamps went backwards: {dts}");
        previous = dts;
    }
}

#[test]
fn a_reordered_stream_has_presentation_diverge_from_decode() {
    // With -bf 3 the encoder reorders, so at least one packet must be shown
    // later than it is decoded. If this ever stops holding, the ctts handling
    // has silently become a no-op.
    let path = fixture!(common::h264_mp4(), "H.264 MP4");
    let mut demuxer = open(&path);

    let mut reordered = false;
    while let Some(packet) = demuxer.next_packet().expect("packets read") {
        if packet.pts != packet.dts {
            reordered = true;
        }
    }
    assert!(
        reordered,
        "a B-frame stream must have composition offsets applied"
    );
}

#[test]
fn seeking_lands_on_a_random_access_point_at_or_before_the_request() {
    use rawshift_video_core::{SeekMode, SeekTo};

    let path = fixture!(common::h264_mp4(), "H.264 MP4");
    let mut demuxer = open(&path);
    let track = only_video_track(&demuxer).id;

    // Ask for a point mid-GOP; the demuxer must land at or before it.
    let requested = 20_000;
    let landed = demuxer
        .seek(track, SeekTo::Timestamp(requested), SeekMode::Precise)
        .expect("seek succeeds");
    assert!(
        landed <= requested,
        "landed at {landed}, after the requested {requested}"
    );

    // And the next packet must be decodable from cold, i.e. a keyframe.
    let packet = demuxer
        .next_packet()
        .expect("packet after seek")
        .expect("not at end of file");
    assert!(
        packet.is_keyframe,
        "a seek must land somewhere decoding can resume"
    );
}

#[test]
fn seeking_an_unknown_track_is_reported_rather_than_ignored() {
    use rawshift_video_core::{SeekMode, SeekTo, TrackId, VideoError};

    let path = fixture!(common::h264_mp4(), "H.264 MP4");
    let mut demuxer = open(&path);

    let err = demuxer
        .seek(TrackId::new(999), SeekTo::Timestamp(0), SeekMode::Precise)
        .expect_err("track 999 does not exist");
    assert!(matches!(err, VideoError::NoSuchTrack { .. }), "{err}");
}

#[test]
fn a_non_isobmff_file_is_rejected_rather_than_half_read() {
    use std::io::Cursor;

    let not_a_movie = Cursor::new(b"\x89PNG\r\n\x1a\n and then some bytes".to_vec());
    assert!(
        IsoBmffDemuxer::open(not_a_movie).is_err(),
        "a PNG must not open as a movie"
    );
}

#[test]
fn a_truncated_file_fails_to_open_rather_than_panicking() {
    use std::io::Cursor;

    let path = fixture!(common::h264_mp4(), "H.264 MP4");
    let whole = std::fs::read(&path).expect("fixture is readable");

    // Truncating mid-header is the shape of a partial download.
    for fraction in [1, 2, 8] {
        let cut = whole.len() / fraction / 2;
        let truncated = Cursor::new(whole[..cut].to_vec());
        // Either it opens with what it found or it errors; it must not panic.
        let _ = IsoBmffDemuxer::open(truncated);
    }
}
