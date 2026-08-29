//! NAL-unit framing for the H.264 and HEVC families.
//!
//! MP4 and Matroska store H.264/HEVC access units as **length-prefixed** NAL
//! units — never Annex B start codes — with the prefix width declared once, in
//! the track's `avcC`/`hvcC` configuration record. Both the demuxers and the
//! decoders need to walk that framing, so it lives here rather than in either.
//!
//! This module reads *framing and NAL headers only*. It never parses a slice
//! header, a parameter set, or anything else that would make it a decoder.

use crate::id::VideoCodecId;
use crate::track::CodecConfig;

/// The width, in bytes, of the length prefix before each NAL unit.
///
/// Legal values are 1, 2 and 4. Three-byte prefixes are representable in the
/// configuration record's two-bit field but are not legal, and are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NalLengthSize(u8);

impl NalLengthSize {
    /// Wrap a prefix width, rejecting anything but 1, 2, or 4.
    #[must_use]
    pub const fn new(bytes: u8) -> Option<Self> {
        match bytes {
            1 | 2 | 4 => Some(Self(bytes)),
            _ => None,
        }
    }

    /// The width in bytes.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0 as usize
    }

    /// Read the prefix width from an `avcC` or `hvcC` record body.
    ///
    /// Both records store `lengthSizeMinusOne` in the low two bits of a byte:
    /// offset 4 for `avcC` (ISO/IEC 14496-15 §5.3.3.1), offset 21 for `hvcC`
    /// (§8.3.3.1).
    ///
    /// Returns `None` for a record that is too short, states an illegal
    /// 3-byte prefix, or belongs to a codec with no such record.
    #[must_use]
    pub fn from_config(codec: VideoCodecId, config: &CodecConfig) -> Option<Self> {
        let bytes = config.as_bytes();
        let offset = match codec {
            VideoCodecId::H264 => 4,
            VideoCodecId::Hevc => 21,
            _ => return None,
        };
        let field = bytes.get(offset)?;
        Self::new((field & 0b11) + 1)
    }
}

/// Walk the length-prefixed NAL units of one access unit.
///
/// Yields each NAL's payload, prefix excluded. Stops at the first prefix that
/// overruns the buffer rather than panicking or yielding a truncated unit: a
/// container is untrusted input, and a truncated tail is the common shape of
/// a damaged file.
pub fn split_length_prefixed(data: &[u8], length_size: NalLengthSize) -> NalUnits<'_> {
    NalUnits {
        data,
        offset: 0,
        length_size: length_size.get(),
    }
}

/// Iterator over the NAL units of one access unit. See
/// [`split_length_prefixed`].
#[derive(Debug, Clone)]
pub struct NalUnits<'a> {
    data: &'a [u8],
    offset: usize,
    length_size: usize,
}

impl<'a> Iterator for NalUnits<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let header_end = self.offset.checked_add(self.length_size)?;
        if header_end > self.data.len() {
            return None;
        }

        let mut length = 0usize;
        for &byte in &self.data[self.offset..header_end] {
            // At most 4 bytes, so this cannot overflow a usize on any target
            // rawshift supports (all are 32-bit or wider).
            length = (length << 8) | byte as usize;
        }

        let payload_end = header_end.checked_add(length)?;
        if payload_end > self.data.len() {
            // Truncated: stop cleanly rather than yielding a short NAL.
            return None;
        }

        self.offset = payload_end;
        Some(&self.data[header_end..payload_end])
    }
}

/// Whether an access unit is a random access point — a picture a decoder can
/// start on with no earlier data.
///
/// Read from the bitstream rather than from the container's sync-sample table,
/// because the bitstream is authoritative and container indices are a
/// well-known source of wrong keyframe flags. The chosen demux backend also
/// exposes no sync-sample flag at all, so this is the only source available.
///
/// Returns `false` for codecs whose NAL framing this module does not model,
/// and for a malformed or empty access unit.
#[must_use]
pub fn is_random_access_point(
    codec: VideoCodecId,
    config: &CodecConfig,
    access_unit: &[u8],
) -> bool {
    let Some(length_size) = NalLengthSize::from_config(codec, config) else {
        return false;
    };

    split_length_prefixed(access_unit, length_size).any(|nal| match codec {
        // H.264 (ITU-T H.264 Table 7-1): the type is the low 5 bits of the
        // one-byte header, and type 5 is an IDR slice.
        VideoCodecId::H264 => nal.first().is_some_and(|h| h & 0x1f == 5),
        // HEVC (ITU-T H.265 Table 7-1): the type is bits 1..6 of the
        // two-byte header. Types 16..=23 are the IRAP range — BLA, IDR and
        // CRA — every one of which is a random access point.
        VideoCodecId::Hevc => nal
            .first()
            .is_some_and(|h| matches!((h >> 1) & 0x3f, 16..=23)),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frame `nals` with 4-byte length prefixes, as MP4 stores them.
    fn framed(nals: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for nal in nals {
            out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            out.extend_from_slice(nal);
        }
        out
    }

    fn four() -> NalLengthSize {
        NalLengthSize::new(4).expect("4 is legal")
    }

    #[test]
    fn only_legal_prefix_widths_are_accepted() {
        assert!(NalLengthSize::new(1).is_some());
        assert!(NalLengthSize::new(2).is_some());
        assert!(NalLengthSize::new(4).is_some());
        // 3 is representable in the two-bit field but is not legal.
        assert!(NalLengthSize::new(3).is_none());
        assert!(NalLengthSize::new(0).is_none());
        assert!(NalLengthSize::new(8).is_none());
    }

    #[test]
    fn avcc_length_size_comes_from_byte_four() {
        // lengthSizeMinusOne = 3 -> 4-byte prefixes. 0xff sets the reserved
        // high bits too, as real records do.
        let cfg = CodecConfig::new(vec![0x01, 0x64, 0x00, 0x28, 0xff]);
        assert_eq!(
            NalLengthSize::from_config(VideoCodecId::H264, &cfg).map(NalLengthSize::get),
            Some(4)
        );

        // lengthSizeMinusOne = 1 -> 2-byte prefixes.
        let cfg = CodecConfig::new(vec![0x01, 0x64, 0x00, 0x28, 0xfd]);
        assert_eq!(
            NalLengthSize::from_config(VideoCodecId::H264, &cfg).map(NalLengthSize::get),
            Some(2)
        );
    }

    #[test]
    fn hvcc_length_size_comes_from_byte_twenty_one() {
        let mut bytes = vec![0u8; 22];
        bytes[21] = 0xff; // lengthSizeMinusOne = 3
        let cfg = CodecConfig::new(bytes);
        assert_eq!(
            NalLengthSize::from_config(VideoCodecId::Hevc, &cfg).map(NalLengthSize::get),
            Some(4)
        );
    }

    #[test]
    fn an_illegal_three_byte_prefix_is_rejected_not_rounded() {
        // lengthSizeMinusOne = 2 means a 3-byte prefix, which is illegal.
        let cfg = CodecConfig::new(vec![0x01, 0x64, 0x00, 0x28, 0xfe]);
        assert_eq!(NalLengthSize::from_config(VideoCodecId::H264, &cfg), None);
    }

    #[test]
    fn a_truncated_config_record_yields_no_length_size() {
        assert_eq!(
            NalLengthSize::from_config(VideoCodecId::H264, &CodecConfig::new(vec![0x01, 0x64])),
            None
        );
        assert_eq!(
            NalLengthSize::from_config(VideoCodecId::Hevc, &CodecConfig::new(vec![0u8; 10])),
            None
        );
        assert_eq!(
            NalLengthSize::from_config(VideoCodecId::H264, &CodecConfig::default()),
            None
        );
    }

    #[test]
    fn codecs_without_nal_framing_have_no_length_size() {
        let cfg = CodecConfig::new(vec![0xff; 32]);
        assert_eq!(NalLengthSize::from_config(VideoCodecId::Av1, &cfg), None);
        assert_eq!(NalLengthSize::from_config(VideoCodecId::ProRes, &cfg), None);
    }

    #[test]
    fn splitting_yields_each_nal_without_its_prefix() {
        let au = framed(&[&[0x65, 0xaa], &[0x41, 0xbb, 0xcc]]);
        let nals: Vec<_> = split_length_prefixed(&au, four()).collect();
        assert_eq!(nals, vec![&[0x65, 0xaa][..], &[0x41, 0xbb, 0xcc][..]]);
    }

    #[test]
    fn splitting_handles_every_legal_prefix_width() {
        for width in [1u8, 2, 4] {
            let size = NalLengthSize::new(width).expect("legal");
            let mut au = Vec::new();
            let payload = [0x65, 0x01, 0x02];
            au.extend_from_slice(&(payload.len() as u32).to_be_bytes()[4 - size.get()..]);
            au.extend_from_slice(&payload);

            let nals: Vec<_> = split_length_prefixed(&au, size).collect();
            assert_eq!(nals, vec![&payload[..]], "width {width}");
        }
    }

    #[test]
    fn a_truncated_trailing_nal_is_dropped_not_yielded_short() {
        // A well-formed NAL, then a prefix claiming 100 bytes with 2 present.
        let mut au = framed(&[&[0x65, 0xaa]]);
        au.extend_from_slice(&100u32.to_be_bytes());
        au.extend_from_slice(&[0xde, 0xad]);

        let nals: Vec<_> = split_length_prefixed(&au, four()).collect();
        assert_eq!(nals, vec![&[0x65, 0xaa][..]], "the good NAL must survive");
    }

    #[test]
    fn a_truncated_prefix_terminates_cleanly() {
        let au = vec![0x00, 0x00]; // two bytes of a four-byte prefix
        assert_eq!(split_length_prefixed(&au, four()).count(), 0);
    }

    #[test]
    fn empty_input_yields_nothing() {
        assert_eq!(split_length_prefixed(&[], four()).count(), 0);
    }

    #[test]
    fn a_zero_length_nal_does_not_stall_the_iterator() {
        // A zero-length NAL is degenerate but must not loop forever.
        let au = framed(&[&[], &[0x65]]);
        let nals: Vec<_> = split_length_prefixed(&au, four()).collect();
        assert_eq!(nals.len(), 2);
        assert!(nals[0].is_empty());
    }

    fn avcc() -> CodecConfig {
        CodecConfig::new(vec![0x01, 0x64, 0x00, 0x28, 0xff])
    }

    fn hvcc() -> CodecConfig {
        let mut bytes = vec![0u8; 22];
        bytes[21] = 0xff;
        CodecConfig::new(bytes)
    }

    #[test]
    fn h264_idr_slices_are_random_access_points() {
        // NAL type 5 = IDR.
        let idr = framed(&[&[0x65, 0x88]]);
        assert!(is_random_access_point(VideoCodecId::H264, &avcc(), &idr));

        // NAL type 1 = a non-IDR slice.
        let inter = framed(&[&[0x41, 0x9a]]);
        assert!(!is_random_access_point(VideoCodecId::H264, &avcc(), &inter));
    }

    #[test]
    fn h264_parameter_sets_before_the_idr_do_not_hide_it() {
        // Real keyframe access units lead with SPS (7) and PPS (8).
        let au = framed(&[&[0x67, 0x64], &[0x68, 0xee], &[0x65, 0x88]]);
        assert!(is_random_access_point(VideoCodecId::H264, &avcc(), &au));
    }

    #[test]
    fn hevc_irap_types_are_random_access_points() {
        // Types 16..=23 are the IRAP range; check the edges and the middle.
        for nal_type in [16u8, 19, 20, 21, 23] {
            let au = framed(&[&[nal_type << 1, 0x01]]);
            assert!(
                is_random_access_point(VideoCodecId::Hevc, &hvcc(), &au),
                "type {nal_type} should be IRAP"
            );
        }
    }

    #[test]
    fn hevc_trailing_pictures_are_not_random_access_points() {
        // Types 0..=15 are the non-IRAP leading/trailing range; 24+ is
        // reserved or non-VCL.
        for nal_type in [0u8, 1, 15, 24, 32] {
            let au = framed(&[&[nal_type << 1, 0x01]]);
            assert!(
                !is_random_access_point(VideoCodecId::Hevc, &hvcc(), &au),
                "type {nal_type} should not be IRAP"
            );
        }
    }

    #[test]
    fn a_codec_without_framing_is_never_a_random_access_point() {
        let au = framed(&[&[0x65, 0x88]]);
        assert!(!is_random_access_point(VideoCodecId::ProRes, &avcc(), &au));
    }

    #[test]
    fn a_malformed_access_unit_is_not_a_random_access_point() {
        assert!(!is_random_access_point(VideoCodecId::H264, &avcc(), &[]));
        assert!(!is_random_access_point(
            VideoCodecId::H264,
            &avcc(),
            &[0x00, 0x00]
        ));
    }
}
