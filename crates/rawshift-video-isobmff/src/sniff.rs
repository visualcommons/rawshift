//! Signature detection for the ISOBMFF family.

use rawshift_video_core::{ContainerId, ContainerSniffer};

/// Marker for MP4 / M4V / QuickTime detection and demuxing.
///
/// One type for the whole family: MP4 and QuickTime share a box grammar and a
/// demuxer, and differ only in the `ftyp` brand, so splitting them into two
/// sniffers would duplicate every byte of the logic.
pub struct IsoBmff;

impl ContainerSniffer for IsoBmff {
    // The family's representative. Use [`detect`] for the MP4/MOV
    // distinction, which needs the brand rather than just the signature.
    const CONTAINER: ContainerId = ContainerId::Mp4;

    fn matches(data: &[u8]) -> bool {
        detect(data).is_some()
    }
}

/// Which ISOBMFF flavour `data` looks like, if any.
///
/// Recognises a leading `ftyp` box, and the legacy QuickTime layout that
/// leads with `moov`, `mdat`, `wide`, `free`, or `skip` instead — old `.mov`
/// files have no `ftyp` at all.
#[must_use]
pub fn detect(data: &[u8]) -> Option<ContainerId> {
    let kind = data.get(4..8)?;

    match kind {
        b"ftyp" => {
            // The major brand follows the box header. QuickTime's is `qt  `;
            // every other brand in practice means MP4.
            let brand = data.get(8..12);
            if brand == Some(b"qt  ") {
                Some(ContainerId::Mov)
            } else {
                Some(ContainerId::Mp4)
            }
        }
        // No ftyp: legacy QuickTime.
        b"moov" | b"mdat" | b"wide" | b"skip" | b"free" => Some(ContainerId::Mov),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_header(kind: &[u8; 4], rest: &[u8]) -> Vec<u8> {
        let mut out = ((rest.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(rest);
        out
    }

    #[test]
    fn an_mp4_brand_is_mp4() {
        for brand in [b"isom", b"mp42", b"avc1", b"iso5", b"M4V "] {
            let data = with_header(b"ftyp", brand);
            assert_eq!(detect(&data), Some(ContainerId::Mp4), "{brand:?}");
        }
    }

    #[test]
    fn the_quicktime_brand_is_mov() {
        let data = with_header(b"ftyp", b"qt  ");
        assert_eq!(detect(&data), Some(ContainerId::Mov));
    }

    #[test]
    fn legacy_quicktime_without_an_ftyp_is_still_recognised() {
        for kind in [b"moov", b"mdat", b"wide", b"free", b"skip"] {
            let data = with_header(kind, &[0u8; 8]);
            assert_eq!(detect(&data), Some(ContainerId::Mov), "{kind:?}");
        }
    }

    #[test]
    fn unrelated_formats_are_not_claimed() {
        // PNG, JPEG, Matroska, RIFF, and plain text must all be rejected.
        assert_eq!(detect(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x00"), None);
        assert_eq!(detect(&[0xff, 0xd8, 0xff, 0xe0, 0, 0, 0, 0]), None);
        assert_eq!(detect(&[0x1a, 0x45, 0xdf, 0xa3, 0, 0, 0, 0]), None);
        assert_eq!(detect(b"RIFF____AVI LIST"), None);
        assert_eq!(detect(b"this is not a video"), None);
    }

    #[test]
    fn short_input_is_rejected_rather_than_panicking() {
        for len in 0..8 {
            assert_eq!(detect(&vec![0u8; len]), None, "len {len}");
        }
    }

    #[test]
    fn an_ftyp_truncated_before_its_brand_still_reads_as_mp4() {
        // The signature is present even though the brand is not; treating it
        // as MP4 lets the demuxer produce the real error.
        let data = with_header(b"ftyp", &[]);
        assert_eq!(detect(&data), Some(ContainerId::Mp4));
    }

    #[test]
    fn the_sniffer_trait_agrees_with_detect() {
        let mp4 = with_header(b"ftyp", b"isom");
        assert!(IsoBmff::matches(&mp4));
        assert!(!IsoBmff::matches(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x00"));
    }
}
