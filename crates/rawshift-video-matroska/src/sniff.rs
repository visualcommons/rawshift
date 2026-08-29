//! Signature detection for Matroska and WebM.

use rawshift_video_core::{ContainerId, ContainerSniffer};

/// The EBML magic every Matroska and WebM file starts with.
const EBML_MAGIC: [u8; 4] = [0x1a, 0x45, 0xdf, 0xa3];

/// Marker for Matroska / WebM detection and demuxing.
pub struct Matroska;

impl ContainerSniffer for Matroska {
    const CONTAINER: ContainerId = ContainerId::Matroska;

    fn matches(data: &[u8]) -> bool {
        detect(data).is_some()
    }
}

/// Which Matroska flavour `data` looks like, if any.
///
/// WebM is Matroska restricted to royalty-free codecs and declares itself in
/// the `DocType` string of the EBML header. That header is small and sits at
/// the very start, so scanning a short prefix for the string is enough and
/// avoids pulling in an EBML parser just to name the flavour.
#[must_use]
pub fn detect(data: &[u8]) -> Option<ContainerId> {
    if data.len() < 4 || data[..4] != EBML_MAGIC {
        return None;
    }

    // The DocType lives within the first few dozen bytes; bound the scan so a
    // crafted file cannot make this walk the whole buffer.
    let horizon = &data[..data.len().min(64)];
    if horizon.windows(4).any(|w| w == b"webm") {
        Some(ContainerId::WebM)
    } else {
        Some(ContainerId::Matroska)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ebml(doctype: &[u8]) -> Vec<u8> {
        let mut out = EBML_MAGIC.to_vec();
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1f]);
        out.extend_from_slice(&[0x42, 0x82]); // DocType element id
        out.push(doctype.len() as u8);
        out.extend_from_slice(doctype);
        out
    }

    #[test]
    fn a_matroska_doctype_is_matroska() {
        assert_eq!(detect(&ebml(b"matroska")), Some(ContainerId::Matroska));
    }

    #[test]
    fn a_webm_doctype_is_webm() {
        assert_eq!(detect(&ebml(b"webm")), Some(ContainerId::WebM));
    }

    #[test]
    fn the_ebml_magic_is_required() {
        // Right length, wrong magic.
        let mut wrong = ebml(b"webm");
        wrong[0] = 0x00;
        assert_eq!(detect(&wrong), None);
    }

    #[test]
    fn unrelated_formats_are_not_claimed() {
        assert_eq!(detect(b"\x89PNG\r\n\x1a\n"), None);
        assert_eq!(detect(b"\x00\x00\x00\x18ftypisom"), None);
        assert_eq!(detect(b"RIFF____AVI "), None);
    }

    #[test]
    fn short_input_is_rejected_rather_than_panicking() {
        for len in 0..4 {
            assert_eq!(detect(&vec![0x1au8; len]), None, "len {len}");
        }
        // Exactly the magic and nothing else: recognised, flavour unknown, so
        // the more general one.
        assert_eq!(detect(&EBML_MAGIC), Some(ContainerId::Matroska));
    }

    #[test]
    fn the_doctype_scan_is_bounded_and_does_not_match_late_content() {
        // "webm" appearing far into the file is track content, not a DocType,
        // and must not reclassify the container.
        let mut data = ebml(b"matroska");
        data.extend(vec![0u8; 4096]);
        data.extend_from_slice(b"webm");
        assert_eq!(detect(&data), Some(ContainerId::Matroska));
    }

    #[test]
    fn the_sniffer_trait_agrees_with_detect() {
        assert!(Matroska::matches(&ebml(b"webm")));
        assert!(!Matroska::matches(b"\x89PNG\r\n\x1a\n"));
    }
}
