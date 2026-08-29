//! Device-gated VAAPI sequence-decode tests.
//!
//! Skips gracefully (eprintln + return) when the machine has no render node,
//! libva cannot be dlopen'd, the driver lacks the codec, or ffmpeg is
//! unavailable — CI without a GPU stays green. On a machine with a working
//! driver these exercise the real path: an ffmpeg-generated bitstream through
//! `video_decoder()` to `VideoFrame` pixels.

#![cfg(hwdec_backend = "vaapi")]

mod common;

use common::{build_hvcc_and_payload, ffmpeg, has_render_node, luma_variance, split_annex_b};
use rawshift_hwdec::{HwCodec, VideoConfig, VideoPacket, available_video_codecs, video_decoder};

// ── gating helpers ──────────────────────────────────────────────────────────

/// Open a sequence decoder, or skip when the driver cannot provide one.
macro_rules! decoder_or_skip {
    ($config:expr) => {
        match video_decoder($config) {
            Ok(decoder) => decoder,
            Err(e) => {
                eprintln!(
                    "Skipping VAAPI sequence test: {e} (render node present: {})",
                    has_render_node()
                );
                return;
            }
        }
    };
}

/// A one-frame all-intra HEVC bitstream, or `None` to skip.
fn hevc_keyframe() -> Option<Vec<u8>> {
    let path = std::env::temp_dir().join("rawshift-hwdec-seq-intra.h265");
    if !path.exists() {
        let ok = ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=128x96:duration=1:rate=1",
            "-c:v",
            "libx265",
            "-x265-params",
            "log-level=error:keyint=1",
            "-pix_fmt",
            "yuv420p",
            "-f",
            "hevc",
            path.to_str()?,
        ]);
        if !ok {
            eprintln!("Skipping: ffmpeg could not produce an HEVC fixture (libx265 missing?)");
            return None;
        }
    }
    std::fs::read(&path).ok()
}

// ── tests ───────────────────────────────────────────────────────────────────

#[test]
fn a_keyframe_decodes_to_real_pixels() {
    let Some(stream) = hevc_keyframe() else {
        return;
    };
    let nals = split_annex_b(&stream);
    let (hvcc, payload) = build_hvcc_and_payload(&nals);

    let mut decoder = decoder_or_skip!(VideoConfig::Hvcc(&hvcc));
    assert_eq!(decoder.codec(), HwCodec::Hevc);

    decoder
        .send_packet(&VideoPacket {
            data: &payload,
            pts: Some(4_242),
            dts: Some(4_242),
            is_sync: true,
        })
        .expect("a random access point must decode");

    let frame = decoder
        .receive_frame()
        .expect("receive succeeds")
        .expect("a frame must be ready after a keyframe");

    assert_eq!(frame.pts(), Some(4_242), "the timestamp must be carried");
    assert_eq!(frame.frame().width(), 128);
    assert_eq!(frame.frame().height(), 96);
    assert!(
        luma_variance(frame.frame()) > 1.0,
        "decoded a blank surface, not the test pattern"
    );
}

#[test]
fn frames_arrive_only_after_a_packet_and_flush_leaves_nothing_behind() {
    let Some(stream) = hevc_keyframe() else {
        return;
    };
    let nals = split_annex_b(&stream);
    let (hvcc, payload) = build_hvcc_and_payload(&nals);

    let mut decoder = decoder_or_skip!(VideoConfig::Hvcc(&hvcc));

    // Nothing has been submitted: "not ready" is None, not an error.
    assert!(decoder.receive_frame().expect("receive succeeds").is_none());

    decoder
        .send_packet(&VideoPacket {
            data: &payload,
            pts: Some(0),
            dts: Some(0),
            is_sync: true,
        })
        .expect("decodes");
    assert!(decoder.receive_frame().expect("receive").is_some());

    // Drained: flush must not invent a frame, and the decoder stays usable.
    decoder.flush().expect("flush succeeds");
    assert!(decoder.receive_frame().expect("receive").is_none());
}

#[test]
fn reset_discards_pending_output_rather_than_emitting_it() {
    let Some(stream) = hevc_keyframe() else {
        return;
    };
    let nals = split_annex_b(&stream);
    let (hvcc, payload) = build_hvcc_and_payload(&nals);

    let mut decoder = decoder_or_skip!(VideoConfig::Hvcc(&hvcc));
    decoder
        .send_packet(&VideoPacket {
            data: &payload,
            pts: Some(0),
            dts: Some(0),
            is_sync: true,
        })
        .expect("decodes");

    // A seek must drop what was decoded, not play it out.
    decoder.reset();
    assert!(
        decoder.receive_frame().expect("receive").is_none(),
        "reset must discard pending output"
    );

    // ... and the session must still work afterwards.
    decoder
        .send_packet(&VideoPacket {
            data: &payload,
            pts: Some(99),
            dts: Some(99),
            is_sync: true,
        })
        .expect("decodes after reset");
    let frame = decoder.receive_frame().expect("receive").expect("a frame");
    assert_eq!(frame.pts(), Some(99));
}

#[test]
fn an_inter_access_unit_is_refused_rather_than_decoded_wrongly() {
    // Until the picture buffer lands, a packet that references other pictures
    // must produce a clear error, never a plausible-looking wrong picture.
    let Some(stream) = hevc_keyframe() else {
        return;
    };
    let nals = split_annex_b(&stream);
    let (hvcc, _) = build_hvcc_and_payload(&nals);

    let mut decoder = decoder_or_skip!(VideoConfig::Hvcc(&hvcc));

    // NAL type 1 (TRAIL_R) is a non-IRAP coded slice.
    let mut inter = Vec::new();
    let nal = [0x02u8, 0x01, 0xd0, 0x09];
    inter.extend_from_slice(&(nal.len() as u32).to_be_bytes());
    inter.extend_from_slice(&nal);

    let err = decoder
        .send_packet(&VideoPacket {
            data: &inter,
            pts: Some(0),
            dts: Some(0),
            is_sync: false,
        })
        .expect_err("an inter picture must be refused");
    assert!(
        err.to_string().contains("random access"),
        "the error must say why: {err}"
    );
}

#[test]
fn a_malformed_configuration_record_fails_at_open() {
    // Opening validates the record, so a stream that cannot work fails here
    // rather than on the first packet.
    // `Box<dyn HwVideoDecoder>` is not Debug, so expect_err does not apply.
    match video_decoder(VideoConfig::Hvcc(&[0x01, 0x02])) {
        Ok(_) => panic!("a truncated hvcC must not open"),
        Err(e) => assert!(!e.to_string().is_empty()),
    }

    // lengthSizeMinusOne = 2 means an illegal 3-byte prefix.
    let mut bad = vec![0u8; 23];
    bad[0] = 1;
    bad[21] = 0x02;
    assert!(video_decoder(VideoConfig::Hvcc(&bad)).is_err());
}

#[test]
fn discovery_agrees_with_what_can_actually_be_opened() {
    // available_video_codecs() must not promise a codec that fails to open.
    for &codec in available_video_codecs() {
        assert_ne!(
            codec,
            HwCodec::Av1,
            "AV1 has no sequence path and must not be advertised"
        );
    }
    // H.264 is sequence-only; it must never appear on the still seam.
    assert!(!rawshift_hwdec::available_codecs().contains(&HwCodec::H264));
}
