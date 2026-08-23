//! The same round-trip matrix, without a window.
//!
//! Exists so CI can exercise the decode/encode/compare logic on a machine with
//! no display server, and so a failure can be reproduced from a terminal. It
//! drives [`crate::pipeline`] directly — the window has no logic of its own to
//! diverge from.

use std::path::PathBuf;

use crate::pipeline::{self, RawSettings, Source};
use crate::settings::EncodeSettings;

/// Run every selected format against every input and print a table.
///
/// Returns the number of failures: a source that would not decode, a variant
/// that errored, or a lossless configuration that did not come back bit-exact.
/// A hardware decoder that is simply absent is reported but **not** counted —
/// on a machine with no backend that is the honest result, not a regression.
pub fn run(paths: &[PathBuf], settings: &EncodeSettings, raw: RawSettings) -> usize {
    let configs = settings.selected();
    println!("{}", crate::backend_banner());
    println!();

    if configs.is_empty() {
        println!("No output formats selected.");
        return 0;
    }

    let mut failures = 0;
    for path in paths {
        println!("── {}", path.display());
        let source = match pipeline::load_source(path, raw) {
            Ok(source) => source,
            Err(e) => {
                println!("   decode failed: {e}");
                failures += 1;
                continue;
            }
        };

        let (_, report) = crate::color::to_display_rgba(&source.image, &source.metadata);
        println!(
            "   {} {}x{}, {} on disk",
            source.kind.label(),
            source.image.width(),
            source.image.height(),
            human_bytes(source.file_bytes as usize),
        );
        for (key, value) in crate::color::describe(&report) {
            println!("   {key:>18}: {value}");
        }

        println!(
            "   {:<8} {:>10} {:>9} {:>9} {:>10} {:>9}  settings",
            "format", "bytes", "encode", "decode", "PSNR", "max delta"
        );
        for config in &configs {
            let variant = pipeline::round_trip(&source, config);
            match &variant.outcome {
                Ok(ok) => {
                    let (psnr, delta) = match ok.comparison {
                        Some(c) if c.bit_exact => ("bit-exact".to_owned(), "0".to_owned()),
                        Some(c) => (
                            format!("{:.2} dB", c.psnr_db.unwrap_or(f64::NAN)),
                            c.max_delta.to_string(),
                        ),
                        None => ("size differs".to_owned(), "-".to_owned()),
                    };
                    println!(
                        "   {:<8} {:>10} {:>8.1}ms {:>8.1}ms {:>10} {:>9}  {}",
                        variant.format.name(),
                        human_bytes(ok.encoded_bytes),
                        ok.encode_ms,
                        ok.decode_ms,
                        psnr,
                        delta,
                        variant.summary,
                    );
                    if ok.lossless_violated {
                        println!(
                            "            ^ LOSSLESS VIOLATED: a lossless configuration must \
                             round-trip bit-exact"
                        );
                        failures += 1;
                    }
                }
                Err(e) => {
                    println!("   {:<8} {e}", variant.format.name());
                    if is_hardware_absence(e) {
                        println!(
                            "            (expected on a machine with no hardware decoder; \
                             not counted as a failure)"
                        );
                    } else {
                        failures += 1;
                    }
                }
            }
        }
        println!();
    }

    pipeline::clean_scratch();
    failures
}

/// Whether an error is "this machine has no hardware decoder" rather than a
/// defect.
///
/// Matched on the error text because the round trip flattens every stage's
/// error to a string for display. The pattern is `RawError::HwDecoderUnavailable`'s
/// own `Display`: `"no hardware decoder available for {codec}: {reason}"`.
///
/// Deliberately narrow. A decoder that exists but *rejects the bitstream* —
/// VideoToolbox refusing AV1 Profile 1, say — also mentions hardware, and is a
/// real interoperability failure that must be counted rather than excused.
fn is_hardware_absence(message: &str) -> bool {
    message.contains("no hardware decoder available for")
}

/// Byte counts at a glance.
fn human_bytes(bytes: usize) -> String {
    const KIB: f64 = 1024.0;
    let bytes = bytes as f64;
    if bytes < KIB {
        format!("{bytes:.0} B")
    } else if bytes < KIB * KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{:.1} MiB", bytes / (KIB * KIB))
    }
}

/// Describe a decoded source in one line, for the UI's file list.
pub fn summarise(source: &Source) -> String {
    format!(
        "{} · {}x{} · {}",
        source.kind.label(),
        source.image.width(),
        source.image.height(),
        human_bytes(source.file_bytes as usize)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_absence_is_recognised() {
        assert!(is_hardware_absence(
            "decode failed: no hardware decoder available for AV1: no hardware AV1 decode \
             backend is compiled in or usable at runtime on this target"
        ));
        assert!(!is_hardware_absence("encode failed: unsupported bit depth"));
    }

    /// A hardware decoder that exists but rejects the bitstream is a real
    /// failure, not an absent backend — VideoToolbox declining AV1 Profile 1
    /// is a genuine interoperability gap and must be counted.
    #[test]
    fn a_rejected_bitstream_is_not_treated_as_missing_hardware() {
        assert!(!is_hardware_absence(
            "decode failed: hardware AV1 decode failed: AV1 profile 1 is outside the \
             still-picture scope (VideoToolbox decodes Main / Profile 0)"
        ));
    }

    #[test]
    fn byte_counts_scale() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.0 MiB");
    }

    #[test]
    fn no_selected_formats_is_not_a_failure() {
        let settings = super::EncodeSettings {
            common: Default::default(),
            formats: Vec::new(),
        };
        assert_eq!(run(&[], &settings, RawSettings::default()), 0);
    }
}
