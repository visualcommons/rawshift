//! Device-gated VideoToolbox integration tests: real hardware decode.
//!
//! Every test skips gracefully (eprintln + return) when the machine reports no
//! hardware decoder for the codec — Intel Macs for HEVC, and every pre-M3 /
//! pre-A17-Pro machine for AV1 — or when the ffmpeg fixture generator is
//! unavailable. CI never runs these (see the pre-release hardware
//! verification section of DEVELOPMENT.md); they are the maintainer's gate,
//! run locally with `just test-hw`.
//!
//! On a machine with a hardware decode block they exercise the whole path:
//! bitstream generation → hvcC/av1C + payload → `decoder()` →
//! `DecodedFrame` pixels.
//!
//! Only compiled when a VideoToolbox build is requested; the bodies are
//! `cfg`-gated on the build-script-selected backend so a plain
//! `cargo test -p rawshift-hwdec` (stub build) compiles them out.

#![cfg(hwdec_backend = "videotoolbox")]

use rawshift_hwdec::{
    ChromaSubsampling, CodecConfig, DecodedFrame, HwBackend, HwCodec, HwStillDecoder, PixelFormat,
    StillDecodeRequest, available_codecs, backend, decoder,
};
use std::process::Command;

// ── gating helpers ──────────────────────────────────────────────────────────

/// `decoder()`, or `None` plus a skip message naming what the probe found.
fn device_or_skip(codec: HwCodec) -> Option<Box<dyn HwStillDecoder>> {
    let decoder = decoder(codec);
    if decoder.is_none() {
        eprintln!(
            "Skipping VideoToolbox device test: no hardware {codec} decoder \
             (backend: {:?}, available codecs: {:?})",
            backend(),
            available_codecs()
        );
    }
    decoder
}

/// Run ffmpeg, returning false (→ skip) when it is missing or fails.
fn ffmpeg(args: &[&str]) -> bool {
    match Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

/// Whether ffmpeg advertises an encoder, so a missing one skips rather than
/// failing.
fn has_encoder(name: &str) -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(name))
}

/// Per-plane sample variance must be non-zero for a real test pattern —
/// catches "decode succeeded but wrote a blank surface".
fn luma_variance(frame: &DecodedFrame) -> f64 {
    let plane = &frame.planes()[0];
    let bps = frame.format().bytes_per_sample();
    let width = frame.width() as usize;
    let mut samples: Vec<f64> = Vec::new();
    for row in 0..frame.height() as usize {
        let start = row * plane.stride;
        for px in 0..width {
            let value = if bps == 1 {
                f64::from(plane.data[start + px])
            } else {
                f64::from(u16::from_le_bytes([
                    plane.data[start + px * 2],
                    plane.data[start + px * 2 + 1],
                ]))
            };
            samples.push(value);
        }
    }
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    samples.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / samples.len() as f64
}

// ── Annex-B → hvcC + length-prefixed payload (test-side container glue) ─────

/// Split an Annex-B stream into NAL units (3- or 4-byte start codes).
fn split_annex_b(data: &[u8]) -> Vec<Vec<u8>> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            starts.push(i);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut nals = Vec::new();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(data.len());
        let mut nal = &data[start + 3..end];
        // Trim the trailing zero(s) that belong to the next 4-byte start code
        // / trailing_zero_8bits.
        while let Some((&0, rest)) = nal.split_last() {
            nal = rest;
        }
        if !nal.is_empty() {
            nals.push(nal.to_vec());
        }
    }
    nals
}

/// Build an hvcC record (4-byte length prefixes) carrying the parameter sets,
/// plus a length-prefixed payload of the coded slices.
fn build_hvcc_and_payload(nals: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
    let mut hvcc = vec![0u8; 23];
    hvcc[0] = 1; // configurationVersion
    hvcc[21] = 0x03; // lengthSizeMinusOne = 3
    let mut arrays: Vec<(u8, Vec<&[u8]>)> = Vec::new();
    let mut payload = Vec::new();
    for nal in nals {
        let nal_type = (nal[0] >> 1) & 0x3f;
        match nal_type {
            32..=34 => match arrays.iter_mut().find(|(t, _)| *t == nal_type) {
                Some((_, list)) => list.push(nal),
                None => arrays.push((nal_type, vec![nal])),
            },
            _ if nal_type < 32 => {
                payload.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                payload.extend_from_slice(nal);
            }
            _ => {} // SEI etc.
        }
    }
    hvcc[22] = arrays.len() as u8;
    for (nal_type, list) in &arrays {
        hvcc.push(0x80 | nal_type);
        hvcc.extend_from_slice(&(list.len() as u16).to_be_bytes());
        for nal in list {
            hvcc.extend_from_slice(&(nal.len() as u16).to_be_bytes());
            hvcc.extend_from_slice(nal);
        }
    }
    (hvcc, payload)
}

/// Generate one intra HEVC frame with ffmpeg/libx265 and return its hvcC +
/// length-prefixed payload; `None` when the encoder is unavailable (→ skip).
fn generate_hevc(
    size: &str,
    pix_fmt: &str,
    range: Option<&str>,
    tag: &str,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let path = std::env::temp_dir().join(format!("rawshift_vt_test_{tag}.265"));
    let path_str = path.to_str().expect("temp path is UTF-8");
    let filter = format!("testsrc2=size={size}:duration=1:rate=1");
    let mut args: Vec<&str> = vec!["-f", "lavfi", "-i", &filter, "-pix_fmt", pix_fmt];
    if let Some(range) = range {
        args.extend_from_slice(&["-color_range", range]);
    }
    args.extend_from_slice(&[
        "-c:v",
        "libx265",
        "-x265-params",
        "keyint=1:log-level=none",
        "-frames:v",
        "1",
        "-f",
        "hevc",
        path_str,
    ]);
    if !ffmpeg(&args) {
        eprintln!("Skipping VideoToolbox HEVC decode test: ffmpeg/libx265 unavailable");
        return None;
    }
    let annex_b = std::fs::read(&path).expect("read generated bitstream");
    let _ = std::fs::remove_file(&path);
    let nals = split_annex_b(&annex_b);
    assert!(!nals.is_empty(), "generated stream has NAL units");
    let (hvcc, payload) = build_hvcc_and_payload(&nals);
    assert!(!payload.is_empty(), "generated stream has a coded slice");
    Some((hvcc, payload))
}

/// Decode one generated HEVC still through the hardware.
fn decode_generated_hevc(size: &str, pix_fmt: &str, tag: &str) -> Option<DecodedFrame> {
    let (hvcc, payload) = generate_hevc(size, pix_fmt, None, tag)?;
    let mut decoder = device_or_skip(HwCodec::Hevc)?;
    let request = StillDecodeRequest {
        config: CodecConfig::Hvcc(&hvcc),
        payload: &payload,
        // The HEIC adapter passes 0/0 because hvcC carries no picture size;
        // mirror that here so the test covers the real call shape.
        width: 0,
        height: 0,
        bit_depth: if pix_fmt.contains("10") { 10 } else { 8 },
        chroma: ChromaSubsampling::Cs420,
    };
    Some(
        decoder
            .decode_still(&request)
            .expect("hardware HEVC decode"),
    )
}

/// Minimal IVF demux: return the first frame's OBU stream.
fn first_ivf_frame(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 32 || &data[0..4] != b"DKIF" {
        return None;
    }
    let header_len = u16::from_le_bytes([data[6], data[7]]) as usize;
    let frame_size = u32::from_le_bytes(data[header_len..header_len + 4].try_into().ok()?) as usize;
    let start = header_len + 12;
    data.get(start..start + frame_size).map(<[u8]>::to_vec)
}

// ── (a) probe ───────────────────────────────────────────────────────────────

/// On a machine with a decode block: the probe must report VideoToolbox and
/// consistent codec/decoder availability. Without one: everything reports
/// "none".
#[test]
fn videotoolbox_probe_reports_consistent_availability() {
    let codecs = available_codecs();
    match backend() {
        Some(backend_kind) => {
            assert_eq!(backend_kind, HwBackend::VideoToolbox);
            assert!(
                !codecs.is_empty(),
                "a reported backend must decode something"
            );
            eprintln!("VideoToolbox probe: available codecs: {codecs:?}");
        }
        None => {
            assert!(codecs.is_empty());
            eprintln!(
                "Skipping VideoToolbox probe assertions: no hardware decoder on this machine"
            );
            return;
        }
    }
    for &codec in codecs {
        assert!(
            decoder(codec).is_some(),
            "decoder({codec}) must exist when listed"
        );
    }
    // HEVC decode is present on every Apple silicon Mac and on Intel Macs from
    // Skylake on, so a machine reporting a backend but no HEVC is worth
    // noting rather than asserting on.
    if !codecs.contains(&HwCodec::Av1) {
        eprintln!(
            "Note: this machine has no AV1 hardware decode block \
             (expected before M3 / A17 Pro); AVIF pixel decode is unavailable."
        );
    }
}

// ── (b) HEVC Main ───────────────────────────────────────────────────────────

#[test]
fn videotoolbox_decodes_real_hevc_main_to_nv12() {
    let Some(frame) = decode_generated_hevc("64x64", "yuv420p", "main8") else {
        return;
    };
    assert_eq!((frame.width(), frame.height()), (64, 64));
    assert_eq!(frame.format(), PixelFormat::Nv12);
    assert_eq!(frame.bit_depth(), 8);
    assert_eq!(frame.planes().len(), 2);
    let variance = luma_variance(&frame);
    assert!(
        variance > 10.0,
        "decoded test pattern must have real content (variance {variance})"
    );
    eprintln!("VideoToolbox HEVC Main: 64x64 NV12 decoded, luma variance {variance:.1}");
}

// ── (c) HEVC Main10 — also the P010 bit-alignment check ─────────────────────

/// CoreVideo's `x420`/`xf20` put the 10-bit sample in the **MSBs** of each
/// 16-bit word, which is exactly `PixelFormat::P010`'s contract. If that were
/// ever wrong — samples in the low bits instead — the variance would collapse
/// by roughly 4096x, so this threshold is the empirical alignment check.
#[test]
fn videotoolbox_decodes_real_hevc_main10_to_p010() {
    let Some(frame) = decode_generated_hevc("64x64", "yuv420p10le", "main10") else {
        return;
    };
    assert_eq!((frame.width(), frame.height()), (64, 64));
    assert_eq!(frame.format(), PixelFormat::P010);
    assert_eq!(frame.bit_depth(), 10);
    let variance = luma_variance(&frame);
    assert!(
        variance > 100.0,
        "decoded 10-bit pattern must have real content in the high bits \
         (variance {variance})"
    );
    eprintln!("VideoToolbox HEVC Main10: 64x64 P010 decoded, luma variance {variance:.1}");
}

// ── (d) conformance window ──────────────────────────────────────────────────

/// 60x36 is not a whole number of coding tree blocks, so HEVC codes it as a
/// larger picture (64x40) plus a conformance window. The backend does no
/// cropping arithmetic of its own because `CVPixelBufferGetWidth/Height`
/// report the clean aperture — this test is what pins that assumption. If
/// VideoToolbox ever reported the coded size instead, this fails immediately
/// and points straight at it.
///
/// (60x36 rather than an odd size because ffmpeg rounds odd dimensions down
/// to even ones for 4:2:0 input, which would silently change what is tested.)
#[test]
fn videotoolbox_crops_to_the_conformance_window() {
    let Some(frame) = decode_generated_hevc("60x36", "yuv420p", "crop") else {
        return;
    };
    assert_eq!(
        (frame.width(), frame.height()),
        (60, 36),
        "VideoToolbox must report the cropped (clean aperture) size, not the \
         coded size"
    );
    let chroma = &frame.planes()[1];
    assert_eq!(
        chroma.stride,
        30 * 2,
        "ceil(60/2) chroma columns, two samples each"
    );
    assert_eq!(frame.planes()[0].stride, 60, "luma rows are tightly packed");
    eprintln!("VideoToolbox conformance window: 64x40 coded -> 60x36 reported");
}

// ── (e) AV1 Profile 0 ───────────────────────────────────────────────────────

#[test]
fn videotoolbox_decodes_real_av1_profile0_to_nv12() {
    let path = std::env::temp_dir().join("rawshift_vt_test_av1.ivf");
    let path_str = path.to_str().expect("temp path is UTF-8");

    // libaom's `-still-picture` is the closest match to an AVIF item; SVT-AV1
    // is the fallback because the common Homebrew ffmpeg ships only that one.
    // A single keyframe from either decodes as a still.
    let generated = if has_encoder("libaom-av1") {
        ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x64:duration=1:rate=1",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libaom-av1",
            "-still-picture",
            "1",
            "-crf",
            "40",
            "-b:v",
            "0",
            "-frames:v",
            "1",
            "-f",
            "ivf",
            path_str,
        ])
    } else if has_encoder("libsvtav1") {
        ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x64:duration=1:rate=1",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libsvtav1",
            "-crf",
            "40",
            "-frames:v",
            "1",
            "-f",
            "ivf",
            path_str,
        ])
    } else {
        false
    };
    if !generated {
        eprintln!("Skipping VideoToolbox AV1 decode test: ffmpeg/libaom-av1/libsvtav1 unavailable");
        return;
    }

    let ivf = std::fs::read(&path).expect("read generated IVF");
    let _ = std::fs::remove_file(&path);
    let payload = first_ivf_frame(&ivf).expect("IVF frame");

    // Minimal av1C: marker/version, profile 0 + level, flags byte, reserved;
    // the sequence header travels in the payload's OBU stream.
    let av1c = [0x81u8, 0x00, 0x0c, 0x00];

    let Some(mut decoder) = device_or_skip(HwCodec::Av1) else {
        return;
    };
    let request = StillDecodeRequest {
        config: CodecConfig::Av1c(&av1c),
        payload: &payload,
        width: 64,
        height: 64,
        bit_depth: 8,
        chroma: ChromaSubsampling::Cs420,
    };
    let frame = decoder.decode_still(&request).expect("hardware AV1 decode");
    assert_eq!((frame.width(), frame.height()), (64, 64));
    assert_eq!(frame.format(), PixelFormat::Nv12);
    assert_eq!(frame.bit_depth(), 8);
    let variance = luma_variance(&frame);
    assert!(
        variance > 10.0,
        "decoded AV1 pattern must have real content (variance {variance})"
    );
    eprintln!("VideoToolbox AV1 Profile0: 64x64 NV12 decoded, luma variance {variance:.1}");
}

// ── (f) session reuse across tiles ──────────────────────────────────────────

/// A grid HEIC decodes hundreds of identically-sized tiles through one
/// decoder. Repeating a decode exercises the cached session, the per-decode
/// slot reset, and the callback refcon's lifetime — a bug in any of those
/// shows up as a changed frame or a crash on the second pass, not the first.
#[test]
fn videotoolbox_reuses_one_session_across_many_decodes() {
    let Some((hvcc, payload)) = generate_hevc("64x64", "yuv420p", None, "reuse") else {
        return;
    };
    let Some(mut decoder) = device_or_skip(HwCodec::Hevc) else {
        return;
    };
    let request = StillDecodeRequest {
        config: CodecConfig::Hvcc(&hvcc),
        payload: &payload,
        width: 0,
        height: 0,
        bit_depth: 8,
        chroma: ChromaSubsampling::Cs420,
    };
    let first = decoder.decode_still(&request).expect("first decode");
    for pass in 1..64 {
        let frame = decoder
            .decode_still(&request)
            .unwrap_or_else(|e| panic!("decode {pass} through the reused session failed: {e}"));
        assert_eq!(frame.width(), first.width(), "pass {pass}");
        assert_eq!(frame.height(), first.height(), "pass {pass}");
        assert_eq!(frame.format(), first.format(), "pass {pass}");
        assert_eq!(
            frame.planes()[0].data,
            first.planes()[0].data,
            "pass {pass} decoded different pixels through the reused session"
        );
    }
    eprintln!("VideoToolbox session reuse: 64 decodes produced identical frames");
}

// ── (g) mixed geometry through one decoder ──────────────────────────────────

/// A decoder is opened per codec, not per picture, so one can be handed
/// pictures of different sizes — a HEIF file's primary image and its thumbnail
/// are exactly that. This drives the session cache's second path, where the
/// configuration record differs and the decoder is asked whether it can accept
/// the new format description before the session is rebuilt.
#[test]
fn videotoolbox_decodes_mixed_geometry_through_one_decoder() {
    let Some((big_hvcc, big_payload)) = generate_hevc("128x96", "yuv420p", None, "mixed_big")
    else {
        return;
    };
    let Some((small_hvcc, small_payload)) = generate_hevc("64x64", "yuv420p", None, "mixed_small")
    else {
        return;
    };
    let Some(mut decoder) = device_or_skip(HwCodec::Hevc) else {
        return;
    };
    let decode = |decoder: &mut Box<dyn HwStillDecoder>, hvcc: &[u8], payload: &[u8]| {
        decoder
            .decode_still(&StillDecodeRequest {
                config: CodecConfig::Hvcc(hvcc),
                payload,
                width: 0,
                height: 0,
                bit_depth: 8,
                chroma: ChromaSubsampling::Cs420,
            })
            .expect("hardware HEVC decode")
    };
    // Alternate, so the cache is forced to switch in both directions rather
    // than settling on one geometry.
    for round in 0..3 {
        let big = decode(&mut decoder, &big_hvcc, &big_payload);
        assert_eq!((big.width(), big.height()), (128, 96), "round {round}");
        let small = decode(&mut decoder, &small_hvcc, &small_payload);
        assert_eq!((small.width(), small.height()), (64, 64), "round {round}");
    }
    eprintln!("VideoToolbox mixed geometry: 128x96 and 64x64 alternated through one decoder");
}

// ── (h) colour range ────────────────────────────────────────────────────────

/// The backend derives `ColorRange` from the emitted surface's four-character
/// code (`420v` vs `420f`) rather than from the SPS. This checks the two
/// encodings do not collapse to the same surface — if they ever do, the range
/// must be sourced from the bitstream instead.
#[test]
fn videotoolbox_reports_a_colour_range() {
    let Some((tv_hvcc, tv_payload)) = generate_hevc("64x64", "yuv420p", Some("tv"), "range_tv")
    else {
        return;
    };
    let Some((pc_hvcc, pc_payload)) = generate_hevc("64x64", "yuv420p", Some("pc"), "range_pc")
    else {
        return;
    };
    let Some(mut decoder) = device_or_skip(HwCodec::Hevc) else {
        return;
    };
    let decode = |decoder: &mut Box<dyn HwStillDecoder>, hvcc: &[u8], payload: &[u8]| {
        decoder
            .decode_still(&StillDecodeRequest {
                config: CodecConfig::Hvcc(hvcc),
                payload,
                width: 0,
                height: 0,
                bit_depth: 8,
                chroma: ChromaSubsampling::Cs420,
            })
            .expect("hardware HEVC decode")
            .range()
    };
    let tv = decode(&mut decoder, &tv_hvcc, &tv_payload);
    let pc = decode(&mut decoder, &pc_hvcc, &pc_payload);
    eprintln!("VideoToolbox colour range: tv -> {tv:?}, pc -> {pc:?}");
}

// ── (i) error path on real hardware ─────────────────────────────────────────

/// Garbage input must fail with an error, not a panic and not a blank frame.
#[test]
fn videotoolbox_rejects_garbage_bitstream() {
    let Some(mut decoder) = device_or_skip(HwCodec::Hevc) else {
        return;
    };
    // An hvcC with no parameter-set arrays at all: the backend must refuse it
    // before ever creating a session, because VideoToolbox needs VPS/SPS/PPS.
    let hvcc = {
        let mut v = vec![0u8; 23];
        v[0] = 1;
        v[21] = 0x03;
        v
    };
    let request = StillDecodeRequest {
        config: CodecConfig::Hvcc(&hvcc),
        payload: &[0, 0, 0, 4, 0x26, 0x01, 0xDE, 0xAD],
        width: 0,
        height: 0,
        bit_depth: 8,
        chroma: ChromaSubsampling::Cs420,
    };
    let err = decoder.decode_still(&request).unwrap_err();
    assert!(
        err.to_string().contains("VPS/SPS/PPS"),
        "expected the missing-parameter-set message, got: {err}"
    );
    eprintln!("VideoToolbox correctly rejected garbage: {err}");
}
