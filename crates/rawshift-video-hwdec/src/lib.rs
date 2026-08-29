//! H.264 and HEVC video decoders backed by platform hardware.
//!
//! Binds `rawshift-video-core`'s [`VideoDecoder`] contract to
//! `rawshift-hwdec`'s sequence seam, translating between rawshift's video
//! vocabulary and the hardware layer's. No `rawshift_hwdec` type appears in
//! this crate's public API.
//!
//! # No software fallback, deliberately
//!
//! `docs/SUPPORT.md` rules out shipping software H.264 or HEVC: both are
//! patent-encumbered independently of any implementation's own license, and
//! the OpenH264 royalty grant covers Cisco's prebuilt binary rather than a
//! from-source or clean-slate build. Hardware decoders carry the device OEM's
//! licence, so that is the only path offered.
//!
//! Where no backend exists — `wasm32`, `musl`, Windows, or a Linux host with
//! no libva — [`VideoError::HwDecoderUnavailable`] is reported, and container
//! parsing, track enumeration and metadata keep working regardless.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rawshift_core::CodecId;
use rawshift_hwdec as hw;
use rawshift_video_core::{
    ColorRange, FrameDesc, FramePixelFormat, MatrixCoefficients, Packet, Plane, VideoCodecId,
    VideoDecoder, VideoDecoderFactory, VideoError, VideoFrame, VideoResult, VideoTrack,
};

/// Register every hardware-backed decoder compiled into this build.
///
/// Push order is priority order; within a codec there is only one backend, so
/// the order between codecs is irrelevant.
pub fn register(registry: &mut rawshift_video_core::DecoderRegistry) {
    #[cfg(feature = "hevc")]
    registry.push(Box::new(HwDecoderFactory::new(VideoCodecId::Hevc)));
    #[cfg(feature = "h264")]
    registry.push(Box::new(HwDecoderFactory::new(VideoCodecId::H264)));
    let _ = registry;
}

/// Opens hardware-backed decoders for one codec.
#[derive(Debug, Clone, Copy)]
pub struct HwDecoderFactory {
    codec: VideoCodecId,
}

impl HwDecoderFactory {
    /// A factory for `codec`.
    #[must_use]
    pub const fn new(codec: VideoCodecId) -> Self {
        Self { codec }
    }

    /// The hardware layer's name for this codec, or `None` if it has no
    /// sequence path.
    fn hw_codec(self) -> Option<hw::HwCodec> {
        match self.codec {
            VideoCodecId::H264 => Some(hw::HwCodec::H264),
            VideoCodecId::Hevc => Some(hw::HwCodec::Hevc),
            _ => None,
        }
    }
}

/// Build the hardware configuration for a track.
///
/// The variant is the codec on both sides, so this cannot pair a record with
/// the wrong codec.
fn hw_config<'a>(codec: VideoCodecId, record: &'a [u8]) -> Option<hw::VideoConfig<'a>> {
    match codec {
        VideoCodecId::H264 => Some(hw::VideoConfig::Avcc(record)),
        VideoCodecId::Hevc => Some(hw::VideoConfig::Hvcc(record)),
        _ => None,
    }
}

impl VideoDecoderFactory for HwDecoderFactory {
    fn id(&self) -> CodecId {
        match self.codec {
            VideoCodecId::H264 => CodecId::new("h264/hwdec"),
            VideoCodecId::Hevc => CodecId::new("hevc/hwdec"),
            _ => CodecId::new("unknown/hwdec"),
        }
    }

    fn codec(&self) -> VideoCodecId {
        self.codec
    }

    fn supports(&self, track: &VideoTrack) -> bool {
        if track.codec != self.codec {
            return false;
        }
        // A track with no configuration record has no parameter sets and no
        // NAL length-prefix width, so no session could be opened for it.
        if track.codec_config.is_empty() {
            return false;
        }
        // Answer about this machine, not just the codec: the registry treats
        // `false` as "try the next backend", so claiming support without a
        // usable driver would strand the track here.
        self.hw_codec()
            .is_some_and(|codec| hw::available_video_codecs().contains(&codec))
    }

    fn open(&self, track: &VideoTrack) -> VideoResult<Box<dyn VideoDecoder>> {
        let record = track.codec_config.as_bytes();
        let config = hw_config(self.codec, record)
            .ok_or(VideoError::UnsupportedCodec { codec: self.codec })?;

        let inner = hw::video_decoder(config).map_err(|e| match e {
            hw::HwDecodeError::Unavailable { reason, .. } => {
                VideoError::hw_unavailable(self.codec, reason)
            }
            other => VideoError::decode(self.codec, other),
        })?;

        Ok(Box::new(HwVideoDecoder {
            codec: self.codec,
            inner,
            // Resolved once: the matrix a frame is decoded with cannot depend
            // on which frame it is.
            matrix: track.color.resolved_matrix(track.dimensions.height),
            range: track.color.range(),
        }))
    }
}

/// A decoder driving one hardware sequence session.
struct HwVideoDecoder {
    codec: VideoCodecId,
    inner: Box<dyn hw::HwVideoDecoder>,
    matrix: MatrixCoefficients,
    range: ColorRange,
}

impl VideoDecoder for HwVideoDecoder {
    fn codec(&self) -> VideoCodecId {
        self.codec
    }

    fn send_packet(&mut self, packet: &Packet) -> VideoResult<()> {
        self.inner
            .send_packet(&hw::VideoPacket {
                data: &packet.data,
                pts: packet.pts,
                dts: packet.dts,
                is_sync: packet.is_keyframe,
            })
            .map_err(|e| VideoError::decode(self.codec, e))
    }

    fn receive_frame(&mut self) -> VideoResult<Option<VideoFrame>> {
        let Some(frame) = self
            .inner
            .receive_frame()
            .map_err(|e| VideoError::decode(self.codec, e))?
        else {
            return Ok(None);
        };
        self.lift(frame).map(Some)
    }

    fn flush(&mut self) -> VideoResult<()> {
        self.inner
            .flush()
            .map_err(|e| VideoError::decode(self.codec, e))
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl HwVideoDecoder {
    /// Translate a hardware frame into rawshift's own, so no backend type
    /// crosses the boundary.
    fn lift(&self, frame: hw::VideoFrame) -> VideoResult<VideoFrame> {
        let (pts, poc) = (frame.pts(), frame.poc());
        let decoded = frame.into_frame();

        let format = match decoded.format() {
            hw::PixelFormat::Nv12 => FramePixelFormat::Nv12,
            hw::PixelFormat::P010 => FramePixelFormat::P010,
            hw::PixelFormat::I420 => FramePixelFormat::I420,
            hw::PixelFormat::I010 => FramePixelFormat::I010,
        };

        let dimensions = rawshift_core::Dimensions {
            width: decoded.width(),
            height: decoded.height(),
        };
        // The decoded frame's own range wins over the container's: the
        // bitstream's VUI is what the samples were actually coded against,
        // and a container that disagrees is mistagged.
        let range = match decoded.range() {
            hw::ColorRange::Full => ColorRange::Full,
            _ => self.range,
        };

        let planes = decoded
            .planes()
            .iter()
            .map(|p| Plane {
                data: p.data.clone(),
                stride: p.stride,
            })
            .collect();

        let desc = FrameDesc::new(format, dimensions, decoded.bit_depth(), range, self.matrix)
            .with_timing(pts, poc);

        VideoFrame::new(self.codec, desc, planes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawshift_video_core::{CicpColor, CodecConfig, DecoderRegistry, TrackId};

    fn track(codec: VideoCodecId, config: Vec<u8>) -> VideoTrack {
        let mut t = VideoTrack::new(
            TrackId::new(1),
            codec,
            rawshift_core::Dimensions {
                width: 1920,
                height: 1080,
            },
            rawshift_core::metadata::URational::new(1, 1000),
        );
        t.codec_config = CodecConfig::new(config);
        t.color = CicpColor::UNSPECIFIED;
        t
    }

    #[test]
    fn a_factory_only_claims_its_own_codec() {
        let hevc = HwDecoderFactory::new(VideoCodecId::Hevc);
        assert!(!hevc.supports(&track(VideoCodecId::H264, vec![0u8; 32])));
    }

    #[test]
    fn a_track_without_a_configuration_record_is_never_claimed() {
        // No parameter sets and no NAL length-prefix width means no session
        // could be opened, so claiming it would strand the track.
        let hevc = HwDecoderFactory::new(VideoCodecId::Hevc);
        assert!(!hevc.supports(&track(VideoCodecId::Hevc, Vec::new())));
    }

    #[test]
    fn support_tracks_what_the_machine_can_actually_do() {
        // supports() must answer about this machine, since the registry reads
        // `false` as "try the next backend".
        let hevc = HwDecoderFactory::new(VideoCodecId::Hevc);
        let claimed = hevc.supports(&track(VideoCodecId::Hevc, vec![0u8; 32]));
        let available = hw::available_video_codecs().contains(&hw::HwCodec::Hevc);
        assert_eq!(claimed, available);
    }

    #[test]
    fn codec_ids_are_stable_and_namespaced() {
        assert_eq!(
            HwDecoderFactory::new(VideoCodecId::H264).id().id,
            "h264/hwdec"
        );
        assert_eq!(
            HwDecoderFactory::new(VideoCodecId::Hevc).id().id,
            "hevc/hwdec"
        );
    }

    #[test]
    fn registration_matches_the_compiled_features() {
        let mut registry = DecoderRegistry::new();
        register(&mut registry);

        let expected = usize::from(cfg!(feature = "hevc")) + usize::from(cfg!(feature = "h264"));
        assert_eq!(registry.factories().len(), expected);
    }

    #[test]
    fn opening_without_a_backend_reports_unavailable_not_unsupported() {
        // On a machine or build with no hardware decoder this must be a
        // capability gap a caller can act on, never a claim the codec is
        // unknown.
        let hevc = HwDecoderFactory::new(VideoCodecId::Hevc);
        if hw::available_video_codecs().contains(&hw::HwCodec::Hevc) {
            eprintln!("skipping: this machine has an HEVC decoder");
            return;
        }
        match hevc.open(&track(VideoCodecId::Hevc, vec![0u8; 32])) {
            Ok(_) => panic!("no backend, yet a decoder opened"),
            Err(e) => assert!(e.is_capability_gap(), "{e}"),
        }
    }
}
