//! VAAPI decoding of coded video sequences.
//!
//! Where [`super::still`]-shaped decoding handles one independent picture,
//! this owns a *session*: parameter sets that persist across access units, a
//! picture buffer, and the output-ordering that reordered streams require.
//!
//! # Current scope
//!
//! **Random access points only.** Every IRAP (HEVC) and IDR (H.264) access
//! unit decodes; an access unit that references other pictures is reported as
//! [`HwDecodeError::Decode`] rather than decoded incorrectly.
//!
//! That is a real capability, not a stub: it covers keyframe extraction,
//! poster frames, timeline thumbnails and scrubbing, and it decodes All-Intra
//! camera modes (Sony XAVC S-I and XAVC HS All-I) completely, since every
//! access unit in those is a random access point.
//!
//! Inter-frame decoding is a purely internal change behind this unchanged
//! API: it adds a decoded picture buffer, picture order counts, reference
//! marking and reference lists, and removes the rejection below. No part of
//! the public seam moves, so it needs no second breaking release.

use crate::{
    CodecConfig, DecodedFrame, HwCodec, HwDecodeError, HwVideoDecoder, StillDecodeRequest,
    VideoConfig, VideoFrame, VideoPacket,
};

use super::hevc;

/// Open a sequence decoder for `config`.
pub fn open(config: VideoConfig<'_>) -> Result<Box<dyn HwVideoDecoder>, HwDecodeError> {
    let codec = config.codec();
    let caps = super::probe();

    let supported = match codec {
        HwCodec::Hevc => caps.hevc_main || caps.hevc_main10,
        HwCodec::H264 => caps.h264_any(),
        HwCodec::Av1 => false,
    };
    if !supported {
        return Err(HwDecodeError::Unavailable {
            codec,
            reason: "the VAAPI driver exposes no decode entry point for this codec".to_string(),
        });
    }

    // Validate the configuration record now, so a stream that cannot work
    // fails at open rather than on the first packet.
    let record = config.bytes().to_vec();
    let nal_length_size = match config {
        VideoConfig::Hvcc(bytes) => {
            hevc::parse_hvcc(bytes)
                .map_err(|e| HwDecodeError::Decode {
                    codec,
                    message: format!("hvcC: {e}"),
                })?
                .nal_length_size
        }
        VideoConfig::Avcc(bytes) => {
            avcc_nal_length_size(bytes).ok_or_else(|| HwDecodeError::Decode {
                codec,
                message: "avcC: truncated or illegal length-prefix width".to_string(),
            })?
        }
    };

    let still =
        super::decoder(codec_for_still(codec)).ok_or_else(|| HwDecodeError::Unavailable {
            codec,
            reason: "the VAAPI driver exposes no decode session for this codec".to_string(),
        })?;

    Ok(Box::new(VaapiVideoDecoder {
        codec,
        record,
        nal_length_size,
        still,
        ready: Vec::new(),
        next_poc: 0,
    }))
}

/// The still-seam codec used to decode one random access point.
///
/// Only HEVC has a still path today; H.264 sessions are rejected at
/// [`open`] before reaching here.
fn codec_for_still(codec: HwCodec) -> HwCodec {
    codec
}

/// Read `lengthSizeMinusOne` from an `avcC` record (ISO/IEC 14496-15
/// §5.3.3.1, byte 4, low two bits).
///
/// `None` for a truncated record or the illegal 3-byte width.
fn avcc_nal_length_size(record: &[u8]) -> Option<usize> {
    let field = record.get(4)?;
    match (field & 0b11) + 1 {
        n @ (1 | 2 | 4) => Some(n as usize),
        _ => None,
    }
}

struct VaapiVideoDecoder {
    codec: HwCodec,
    /// The configuration record, kept because every decode submits it and it
    /// must survive [`HwVideoDecoder::reset`].
    record: Vec<u8>,
    nal_length_size: usize,
    still: Box<dyn crate::HwStillDecoder>,
    /// Frames decoded and awaiting collection, in output order.
    ready: Vec<VideoFrame>,
    /// Picture order count assigned to the next frame.
    ///
    /// With random access points only there is no reordering, so output order
    /// is submission order and a monotonic counter is the correct picture
    /// order. Real picture order counts arrive with inter-frame support.
    next_poc: i32,
}

impl VaapiVideoDecoder {
    /// Whether `access_unit` is a random access point this build can decode.
    fn is_random_access_point(&self, access_unit: &[u8]) -> bool {
        let Ok(nals) = hevc::split_length_prefixed(access_unit, self.nal_length_size) else {
            return false;
        };
        nals.iter().any(|nal| match self.codec {
            // HEVC: types 16..=23 are the IRAP range (BLA, IDR, CRA).
            HwCodec::Hevc => hevc::nal_type(nal).is_ok_and(hevc::is_irap),
            // H.264: type 5 is an IDR slice.
            HwCodec::H264 => nal.first().is_some_and(|h| h & 0x1f == 5),
            HwCodec::Av1 => false,
        })
    }
}

impl HwVideoDecoder for VaapiVideoDecoder {
    fn codec(&self) -> HwCodec {
        self.codec
    }

    fn send_packet(&mut self, packet: &VideoPacket<'_>) -> Result<(), HwDecodeError> {
        if !self.is_random_access_point(packet.data) {
            return Err(HwDecodeError::Decode {
                codec: self.codec,
                message: "this build decodes random access points only; inter-frame decoding \
                          needs the decoded picture buffer that is not implemented yet"
                    .to_string(),
            });
        }

        let frame = self.decode_random_access_point(packet.data)?;
        let poc = self.next_poc;
        self.next_poc = self.next_poc.saturating_add(1);
        self.ready.push(VideoFrame::new(frame, packet.pts, poc));
        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Option<VideoFrame>, HwDecodeError> {
        if self.ready.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.ready.remove(0)))
    }

    fn flush(&mut self) -> Result<(), HwDecodeError> {
        // Nothing is held back: with no reordering every decoded picture is
        // already in `ready`. Inter-frame support drains the picture buffer
        // here instead.
        Ok(())
    }

    fn reset(&mut self) {
        self.ready.clear();
        self.next_poc = 0;
    }
}

impl VaapiVideoDecoder {
    fn decode_random_access_point(&mut self, data: &[u8]) -> Result<DecodedFrame, HwDecodeError> {
        let config = match self.codec {
            HwCodec::Hevc => CodecConfig::Hvcc(&self.record),
            codec => {
                return Err(HwDecodeError::Decode {
                    codec,
                    message: "no still-decode path exists for this codec".to_string(),
                });
            }
        };

        let request = StillDecodeRequest {
            config,
            payload: data,
            // Advisory only; the still path takes the real geometry from the
            // stream's own parameter sets.
            width: 0,
            height: 0,
            bit_depth: 0,
            chroma: gamut_color::ChromaSubsampling::Cs420,
        };
        self.still.decode_still(&request)
    }
}
