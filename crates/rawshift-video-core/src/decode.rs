//! The decoding contracts and the backend registry.

use rawshift_core::CodecId;

use crate::error::{VideoError, VideoResult};
use crate::frame::VideoFrame;
use crate::id::VideoCodecId;
use crate::packet::Packet;
use crate::track::VideoTrack;

/// A decoder for one video track.
///
/// **Send-packet / receive-frame**, not `decode(packet) -> frame`. Coded order
/// and display order differ whenever a stream reorders (B pictures,
/// hierarchical GOPs), so one packet may make zero, one, or several frames
/// available. The split is also the shape every hardware decode API natively
/// has.
///
/// Drive it as:
///
/// ```text
/// while let Some(packet) = demuxer.next_packet()? {
///     decoder.send_packet(&packet)?;
///     while let Some(frame) = decoder.receive_frame()? { sink(frame); }
/// }
/// decoder.flush()?;
/// while let Some(frame) = decoder.receive_frame()? { sink(frame); }
/// ```
///
/// Frames arrive in **display order**; a caller must not reorder them.
pub trait VideoDecoder: Send {
    /// The codec this decoder decodes.
    fn codec(&self) -> VideoCodecId;

    /// Submit one access unit, making zero or more frames available to
    /// [`receive_frame`](Self::receive_frame).
    ///
    /// After [`reset`](Self::reset), packets before the next random access
    /// point reference pictures that are gone; they are dropped and `Ok(())`
    /// is returned, so seeking into the middle of a GOP is not an error.
    ///
    /// # Errors
    ///
    /// [`VideoError::Decode`] when the access unit is malformed or the backend
    /// fails.
    fn send_packet(&mut self, packet: &Packet) -> VideoResult<()>;

    /// Take the next frame in display order, or `None` when none is ready.
    ///
    /// `None` is the normal state while a reordering stream fills its buffer;
    /// it means "not yet", not "end of stream". Only
    /// [`flush`](Self::flush) establishes the latter.
    ///
    /// # Errors
    ///
    /// [`VideoError::Decode`] when reading the decoded picture back fails.
    fn receive_frame(&mut self) -> VideoResult<Option<VideoFrame>>;

    /// Signal end of stream: every buffered picture becomes available to
    /// [`receive_frame`](Self::receive_frame).
    ///
    /// Without this the last few frames of every reordered stream are never
    /// emitted. The decoder stays usable afterwards.
    ///
    /// # Errors
    ///
    /// As [`send_packet`](Self::send_packet).
    fn flush(&mut self) -> VideoResult<()>;

    /// Discard all state **without** emitting buffered frames — the seek path.
    fn reset(&mut self);
}

/// An encoder for one video track.
///
/// The seam only: rawshift ships no video encoder yet. It is defined now so
/// that adding one is additive, and so the shape is settled while the decode
/// side is fresh.
pub trait VideoEncoder: Send {
    /// The codec this encoder produces.
    fn codec(&self) -> VideoCodecId;

    /// Submit one raw frame for encoding.
    ///
    /// # Errors
    ///
    /// Backend-specific encode failures.
    fn send_frame(&mut self, frame: &VideoFrame) -> VideoResult<()>;

    /// Take the next coded access unit, or `None` when none is ready.
    ///
    /// # Errors
    ///
    /// Backend-specific encode failures.
    fn receive_packet(&mut self) -> VideoResult<Option<Packet>>;

    /// Signal end of stream and drain the encoder.
    ///
    /// # Errors
    ///
    /// Backend-specific encode failures.
    fn flush(&mut self) -> VideoResult<()>;
}

/// Opens decoders for tracks it supports.
///
/// One factory per backend per codec. Registered in a [`DecoderRegistry`],
/// which picks between them.
pub trait VideoDecoderFactory: Send + Sync {
    /// Stable `"{format}/{impl}"` identifier, e.g. `"h264/vaapi"`.
    fn id(&self) -> CodecId;

    /// The codec this factory decodes.
    fn codec(&self) -> VideoCodecId;

    /// Whether this backend can decode this specific track **right now**.
    ///
    /// Answers about the runtime too, not just the codec: a VAAPI factory
    /// returns `false` with no libva, no device, or no driver support for the
    /// track's profile. This is the **only** signal that lets the registry try
    /// the next backend — a factory that accepts a track and then fails in
    /// [`open`](Self::open) is reported to the caller rather than retried.
    fn supports(&self, track: &VideoTrack) -> bool;

    /// Open a decoder for `track`.
    ///
    /// # Errors
    ///
    /// Any failure to start a session. Reaching here after `supports` returned
    /// `true` is terminal for the whole open, by the contract above.
    fn open(&self, track: &VideoTrack) -> VideoResult<Box<dyn VideoDecoder>>;
}

/// Backends to try, in order.
///
/// **Push order is priority order**, and `supports() == false` is the only
/// thing that falls through to the next entry. A backend that accepts a track
/// and then fails produces an error rather than a silent retry, so a genuine
/// backend fault is never disguised as "unsupported".
///
/// This mirrors the fallback contract `gamut-codec-abi` documents for image
/// codestream backends, adopted deliberately so both media behave the same.
#[derive(Default)]
pub struct DecoderRegistry {
    factories: Vec<Box<dyn VideoDecoderFactory>>,
}

impl DecoderRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a factory, at lowest priority.
    pub fn push(&mut self, factory: Box<dyn VideoDecoderFactory>) {
        self.factories.push(factory);
    }

    /// Append a factory and return the registry, for chaining at startup.
    #[must_use]
    pub fn with(mut self, factory: Box<dyn VideoDecoderFactory>) -> Self {
        self.push(factory);
        self
    }

    /// Every registered factory, in priority order.
    #[must_use]
    pub fn factories(&self) -> &[Box<dyn VideoDecoderFactory>] {
        &self.factories
    }

    /// Whether any registered backend can decode `track` right now.
    #[must_use]
    pub fn supports(&self, track: &VideoTrack) -> bool {
        self.factories.iter().any(|f| f.supports(track))
    }

    /// Open a decoder for `track` using the first backend that supports it.
    ///
    /// # Errors
    ///
    /// [`VideoError::UnsupportedCodec`] when no factory for the codec is
    /// registered at all, and
    /// [`VideoError::HwDecoderUnavailable`] when factories for the codec exist
    /// but none supports this track on this machine — the distinction between
    /// "not built" and "no device", which callers act on differently.
    pub fn open(&self, track: &VideoTrack) -> VideoResult<Box<dyn VideoDecoder>> {
        let mut saw_codec = false;
        for factory in &self.factories {
            if factory.codec() != track.codec {
                continue;
            }
            saw_codec = true;
            if factory.supports(track) {
                return factory.open(track);
            }
        }

        if saw_codec {
            Err(VideoError::hw_unavailable(
                track.codec,
                "a decoder is compiled in, but no backend on this machine supports this track",
            ))
        } else {
            Err(VideoError::UnsupportedCodec { codec: track.codec })
        }
    }
}

impl std::fmt::Debug for DecoderRegistry {
    /// Lists the registered ids: the factories themselves are trait objects
    /// with nothing else printable, and the order is the interesting part.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecoderRegistry")
            .field(
                "factories",
                &self.factories.iter().map(|f| f.id().id).collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{CicpColor, CodecConfig, Rotation};
    use gamut_color::ChromaSubsampling;
    use rawshift_core::metadata::URational;
    use rawshift_core::{BitDepth, Dimensions};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn track(codec: VideoCodecId) -> VideoTrack {
        VideoTrack {
            id: crate::id::TrackId::new(1),
            codec,
            dimensions: Dimensions {
                width: 1920,
                height: 1080,
            },
            color: CicpColor::UNSPECIFIED,
            bit_depth: BitDepth::Eight,
            chroma: ChromaSubsampling::Cs420,
            frame_rate: None,
            time_base: URational::new(1, 1000),
            duration: None,
            frame_count: None,
            rotation: Rotation::None,
            codec_config: CodecConfig::default(),
        }
    }

    struct StubDecoder(VideoCodecId);
    impl VideoDecoder for StubDecoder {
        fn codec(&self) -> VideoCodecId {
            self.0
        }
        fn send_packet(&mut self, _p: &Packet) -> VideoResult<()> {
            Ok(())
        }
        fn receive_frame(&mut self) -> VideoResult<Option<VideoFrame>> {
            Ok(None)
        }
        fn flush(&mut self) -> VideoResult<()> {
            Ok(())
        }
        fn reset(&mut self) {}
    }

    struct StubFactory {
        id: &'static str,
        codec: VideoCodecId,
        supports: bool,
        opens: &'static AtomicUsize,
        open_fails: bool,
    }

    impl VideoDecoderFactory for StubFactory {
        fn id(&self) -> CodecId {
            CodecId::new(self.id)
        }
        fn codec(&self) -> VideoCodecId {
            self.codec
        }
        fn supports(&self, _t: &VideoTrack) -> bool {
            self.supports
        }
        fn open(&self, _t: &VideoTrack) -> VideoResult<Box<dyn VideoDecoder>> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            if self.open_fails {
                Err(VideoError::decode(self.codec, "backend fault"))
            } else {
                Ok(Box::new(StubDecoder(self.codec)))
            }
        }
    }

    /// `Box<dyn VideoDecoder>` is not `Debug`, so `expect_err` does not apply.
    fn expect_err(result: VideoResult<Box<dyn VideoDecoder>>, context: &str) -> VideoError {
        match result {
            Ok(_) => panic!("expected an error: {context}"),
            Err(e) => e,
        }
    }

    fn expect_ok(result: VideoResult<Box<dyn VideoDecoder>>, context: &str) {
        if let Err(e) = result {
            panic!("expected success ({context}), got {e}");
        }
    }

    static FIRST: AtomicUsize = AtomicUsize::new(0);
    static SECOND: AtomicUsize = AtomicUsize::new(0);

    fn factory(
        id: &'static str,
        codec: VideoCodecId,
        supports: bool,
        opens: &'static AtomicUsize,
        open_fails: bool,
    ) -> Box<dyn VideoDecoderFactory> {
        Box::new(StubFactory {
            id,
            codec,
            supports,
            opens,
            open_fails,
        })
    }

    #[test]
    fn push_order_is_priority_order() {
        FIRST.store(0, Ordering::SeqCst);
        SECOND.store(0, Ordering::SeqCst);
        let reg = DecoderRegistry::new()
            .with(factory(
                "h264/first",
                VideoCodecId::H264,
                true,
                &FIRST,
                false,
            ))
            .with(factory(
                "h264/second",
                VideoCodecId::H264,
                true,
                &SECOND,
                false,
            ));

        expect_ok(reg.open(&track(VideoCodecId::H264)), "a supporting backend");
        assert_eq!(FIRST.load(Ordering::SeqCst), 1, "first should have won");
        assert_eq!(
            SECOND.load(Ordering::SeqCst),
            0,
            "second should not be tried"
        );
    }

    #[test]
    fn an_unsupporting_backend_falls_through_to_the_next() {
        FIRST.store(0, Ordering::SeqCst);
        SECOND.store(0, Ordering::SeqCst);
        let reg = DecoderRegistry::new()
            .with(factory(
                "h264/first",
                VideoCodecId::H264,
                false,
                &FIRST,
                false,
            ))
            .with(factory(
                "h264/second",
                VideoCodecId::H264,
                true,
                &SECOND,
                false,
            ));

        expect_ok(reg.open(&track(VideoCodecId::H264)), "a supporting backend");
        assert_eq!(
            FIRST.load(Ordering::SeqCst),
            0,
            "unsupporting must not open"
        );
        assert_eq!(SECOND.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_backend_that_accepts_then_fails_is_not_retried_elsewhere() {
        // The core of the fallback contract: only supports()==false falls
        // through, so a genuine backend fault surfaces instead of being
        // disguised as "unsupported" by a later backend succeeding.
        FIRST.store(0, Ordering::SeqCst);
        SECOND.store(0, Ordering::SeqCst);
        let reg = DecoderRegistry::new()
            .with(factory(
                "h264/first",
                VideoCodecId::H264,
                true,
                &FIRST,
                true,
            ))
            .with(factory(
                "h264/second",
                VideoCodecId::H264,
                true,
                &SECOND,
                false,
            ));

        let err = expect_err(
            reg.open(&track(VideoCodecId::H264)),
            "the fault must propagate",
        );
        assert!(err.to_string().contains("backend fault"), "{err}");
        assert_eq!(SECOND.load(Ordering::SeqCst), 0, "must not fall through");
    }

    #[test]
    fn no_factory_for_the_codec_is_unsupported_not_unavailable() {
        let reg = DecoderRegistry::new().with(factory(
            "h264/vaapi",
            VideoCodecId::H264,
            true,
            &FIRST,
            false,
        ));
        let err = expect_err(reg.open(&track(VideoCodecId::ProRes)), "no ProRes factory");
        assert!(
            matches!(err, VideoError::UnsupportedCodec { .. }),
            "expected UnsupportedCodec, got {err}"
        );
    }

    #[test]
    fn a_compiled_codec_with_no_usable_backend_is_unavailable_not_unsupported() {
        // The distinction a caller acts on: "rebuild with the feature" versus
        // "this machine has no GPU".
        let reg = DecoderRegistry::new().with(factory(
            "hevc/vaapi",
            VideoCodecId::Hevc,
            false,
            &FIRST,
            false,
        ));
        let err = expect_err(
            reg.open(&track(VideoCodecId::Hevc)),
            "no backend supports it",
        );
        assert!(
            matches!(err, VideoError::HwDecoderUnavailable { .. }),
            "expected HwDecoderUnavailable, got {err}"
        );
        assert!(err.is_capability_gap());
    }

    #[test]
    fn an_empty_registry_reports_unsupported() {
        let reg = DecoderRegistry::new();
        assert!(!reg.supports(&track(VideoCodecId::H264)));
        assert!(matches!(
            reg.open(&track(VideoCodecId::H264)),
            Err(VideoError::UnsupportedCodec { .. })
        ));
    }

    #[test]
    fn debug_lists_ids_in_priority_order() {
        let reg = DecoderRegistry::new()
            .with(factory(
                "hevc/vaapi",
                VideoCodecId::Hevc,
                true,
                &FIRST,
                false,
            ))
            .with(factory(
                "h264/vaapi",
                VideoCodecId::H264,
                true,
                &SECOND,
                false,
            ));
        let printed = format!("{reg:?}");
        assert!(printed.contains("hevc/vaapi"), "{printed}");
        assert!(
            printed.find("hevc/vaapi") < printed.find("h264/vaapi"),
            "order must be visible: {printed}"
        );
    }
}
