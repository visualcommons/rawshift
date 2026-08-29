//! Helpers shared by the VAAPI device tests.
//!
//! These live in one place because they encode fiddly, easy-to-get-wrong
//! details — the `hvcC` record layout in particular, where putting
//! `numOfArrays` one byte off yields a record that parses but carries no
//! parameter sets. Two copies of that drift, and the second copy is where the
//! bug hides.

#![allow(dead_code)]

use rawshift_hwdec::DecodedFrame;
use std::process::Command;

/// Whether any DRM render node exists (cheap pre-check for skip messages).
pub fn has_render_node() -> bool {
    (128..136).any(|minor| std::path::Path::new(&format!("/dev/dri/renderD{minor}")).exists())
}

/// Run ffmpeg, returning false (→ skip) when it is missing or fails.
pub fn ffmpeg(args: &[&str]) -> bool {
    match Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

/// Per-plane sample variance must be non-zero for a real test pattern —
/// catches "decode succeeded but wrote a blank surface".
pub fn luma_variance(frame: &DecodedFrame) -> f64 {
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

/// Split an Annex-B stream into NAL units (3- or 4-byte start codes).
pub fn split_annex_b(data: &[u8]) -> Vec<Vec<u8>> {
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
        // Trim the trailing zero(s) that belong to the next 4-byte start
        // code / trailing_zero_8bits.
        while let Some((&0, rest)) = nal.split_last() {
            nal = rest;
        }
        if !nal.is_empty() {
            nals.push(nal.to_vec());
        }
    }
    nals
}

/// Build an hvcC record (4-byte length prefixes) carrying `param_sets` and
/// a length-prefixed payload from `slices`.
pub fn build_hvcc_and_payload(nals: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
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
