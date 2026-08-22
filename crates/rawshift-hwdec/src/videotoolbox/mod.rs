//! VideoToolbox still-frame decode backend for macOS and iOS.
//!
//! ## Runtime model
//!
//! VideoToolbox, CoreMedia and CoreVideo are always-present Apple system
//! frameworks, so unlike the VAAPI backend there is nothing to `dlopen` and no
//! device node to find: availability is a pure capability question, answered
//! once per process by [`VTIsHardwareDecodeSupported`] for each codec.
//!
//! **Hardware only.** On macOS the session is created with
//! `kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder`, so a
//! machine without a decode block for the codec fails session creation instead
//! of silently falling back to VideoToolbox's *software* decoder — which would
//! make a crate called `rawshift-hwdec` quietly lie about what it did. That
//! key is macOS-only (see the `decoder_specification` docs); on iOS every
//! VideoToolbox decoder is hardware anyway.
//!
//! ## Codec scope
//!
//! - **HEVC** (HEIC) still pictures: `hvcC` parameter sets, length-prefixed
//!   NAL payload passed through byte for byte — no Annex-B conversion.
//! - **AV1** (AVIF) Profile 0 still pictures: `av1C` config atom, raw OBU
//!   temporal unit payload. Runtime-probed, because AV1 hardware decode
//!   arrived with the M3 and A17 Pro generations — on older Apple silicon and
//!   on Intel, [`available_codecs`] honestly omits [`HwCodec::Av1`] while HEVC
//!   keeps working.
//!
//! Both emit 8-bit or 10-bit 4:2:0 → `Nv12` / `P010` (see [`pixels`]).
//!
//! ## Binding choice
//!
//! This backend uses the generated [`objc2`](https://github.com/madsmtm/objc2)
//! framework crates rather than a hand-written `sys.rs`, the opposite call
//! from the VAAPI backend. The reason is that the two situations differ:
//! libva must be `dlopen`'d so a machine without it degrades to "no decoder"
//! instead of failing to start, which rules out generated bindings; the Apple
//! frameworks are guaranteed present and link normally. Given that, the
//! maintained bindings win on every axis that matters here — `CFRetained<T>`
//! gives us Create/Get-rule reference counting as RAII instead of hand-paired
//! `CFRetain`/`CFRelease`, and the `#[link(kind = "framework")]` attributes
//! live upstream. They are pulled in with `default-features = false` and only
//! the per-header features this file needs, so no Objective-C runtime, Metal,
//! OpenGL or CoreAudio code is compiled in. See `Cargo.toml`.
//!
//! ## Safety boundary
//!
//! All `unsafe` in this backend is FFI, plus the one `unsafe impl Send` on the
//! decoder. The bitstream parsers in [`crate::bitstream`] and the pixel
//! plumbing in [`pixels`] are safe Rust apart from the single documented slice
//! construction over a locked surface.

mod format;
mod pixels;
mod sample;
mod status;

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::OnceLock;

use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMFormatDescription, CMTime, kCMVideoCodecType_HEVC};
use objc2_core_video::CVImageBuffer;
use objc2_video_toolbox::{
    VTDecodeFrameFlags, VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord,
    VTDecompressionSession, VTIsHardwareDecodeSupported,
};

use self::status::{NO_ERR, OSStatus, decode_err, map_status};
use crate::{
    CodecConfig, DecodedFrame, HwBackend, HwCodec, HwDecodeError, HwStillDecoder,
    StillDecodeRequest,
};

// ── Public backend surface (called from lib.rs) ─────────────────────────────

/// Backend hook for [`crate::decoder`].
pub fn decoder(codec: HwCodec) -> Option<Box<dyn HwStillDecoder>> {
    let caps = probe();
    let supported = match codec {
        HwCodec::Hevc => caps.hevc,
        HwCodec::Av1 => caps.av1,
    };
    if !supported {
        return None;
    }
    // Nothing to acquire up front: the decompression session is created lazily
    // on the first decode, from that picture's format description. This keeps
    // `decoder(c).is_some()` exactly equal to `available_codecs().contains(&c)`,
    // which `crate::tests::discovery_entry_points_are_consistent` requires.
    Some(Box::new(VideoToolboxStillDecoder::new(codec)))
}

/// Backend hook for [`crate::backend`].
pub fn backend() -> Option<HwBackend> {
    let caps = probe();
    (caps.hevc || caps.av1).then_some(HwBackend::VideoToolbox)
}

/// Backend hook for [`crate::available_codecs`].
pub fn available_codecs() -> &'static [HwCodec] {
    const NONE: &[HwCodec] = &[];
    const HEVC_ONLY: &[HwCodec] = &[HwCodec::Hevc];
    const AV1_ONLY: &[HwCodec] = &[HwCodec::Av1];
    const BOTH: &[HwCodec] = &[HwCodec::Hevc, HwCodec::Av1];
    let caps = probe();
    match (caps.hevc, caps.av1) {
        (true, true) => BOTH,
        (true, false) => HEVC_ONLY,
        (false, true) => AV1_ONLY,
        (false, false) => NONE,
    }
}

// ── Runtime probe ───────────────────────────────────────────────────────────

/// `kCMVideoCodecType_AV1` — `'av01'`.
///
/// Named locally rather than imported so the constant is greppable next to its
/// HEVC peer; the value is the one `objc2-core-media` generates.
const CODEC_TYPE_AV1: u32 = objc2_core_media::kCMVideoCodecType_AV1;

/// What this machine's VideoToolbox reports *hardware* decode support for.
#[derive(Debug, Clone, Copy, Default)]
struct Caps {
    hevc: bool,
    av1: bool,
}

/// Probe once per process: hardware does not change at runtime.
fn probe() -> Caps {
    static CAPS: OnceLock<Caps> = OnceLock::new();
    *CAPS.get_or_init(|| {
        // SAFETY: `VTIsHardwareDecodeSupported` takes a four-character codec
        // type by value and only reads static system capability — no pointers,
        // no session, no allocation. It is available since macOS 10.13 /
        // iOS 11, both below this crate's macOS 11 / iOS 14 floor (see
        // docs/SUPPORT.md), so the symbol is always present and needs no weak
        // linking. An unrecognised codec type returns false rather than
        // trapping.
        unsafe {
            Caps {
                hevc: VTIsHardwareDecodeSupported(kCMVideoCodecType_HEVC),
                av1: VTIsHardwareDecodeSupported(CODEC_TYPE_AV1),
            }
        }
    })
}

/// The decoder specification handed to `VTDecompressionSessionCreate`.
///
/// On macOS this pins the session to a hardware decoder: Apple documents
/// `RequireHardwareAcceleratedVideoDecoder` as making session creation *fail*
/// when hardware acceleration is not possible, which is what this crate wants
/// — a clean [`HwDecodeError::Unavailable`] instead of a silent software
/// decode. Because [`probe`] already gates on `VTIsHardwareDecodeSupported`,
/// it should never fire spuriously.
///
/// On iOS it returns `None`, and that is **not** cosmetic: the
/// `kVTVideoDecoderSpecification_*` symbols are annotated `ios(17.0)` and the
/// binding is a plain non-weak `extern` static, so merely referencing one in a
/// build targeting iOS 14–16 would be a dyld "symbol not found" *launch*
/// failure for the whole app. iOS has no software VideoToolbox decoder to
/// guard against in any case.
#[cfg(target_os = "macos")]
fn decoder_specification() -> Option<
    CFRetained<
        objc2_core_foundation::CFDictionary<
            objc2_core_foundation::CFString,
            objc2_core_foundation::CFType,
        >,
    >,
> {
    use objc2_core_foundation::{CFDictionary, CFType, kCFBooleanTrue};
    use objc2_video_toolbox::kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder;

    // SAFETY: both are immortal constant CF objects exported by VideoToolbox
    // and CoreFoundation; reading them performs no allocation and they are
    // valid for the life of the process.
    let (key, value) = unsafe {
        (
            kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder,
            kCFBooleanTrue?,
        )
    };
    Some(CFDictionary::from_slices(
        &[key],
        &[value.as_ref() as &CFType],
    ))
}

#[cfg(not(target_os = "macos"))]
fn decoder_specification() -> Option<
    CFRetained<
        objc2_core_foundation::CFDictionary<
            objc2_core_foundation::CFString,
            objc2_core_foundation::CFType,
        >,
    >,
> {
    None
}

// ── Output callback ─────────────────────────────────────────────────────────

/// Where the decompression callback deposits its result.
///
/// Heap-allocated and owned by the decoder so its address stays stable for the
/// life of the session that was created with a pointer to it.
struct DecodeSlot {
    status: OSStatus,
    info: VTDecodeInfoFlags,
    image: Option<CFRetained<objc2_core_video::CVPixelBuffer>>,
}

impl Default for DecodeSlot {
    fn default() -> Self {
        Self {
            status: NO_ERR,
            info: VTDecodeInfoFlags::empty(),
            image: None,
        }
    }
}

/// `VTDecompressionOutputCallback`: called once per decoded frame.
///
/// This runs on the calling thread inside `VTDecompressionSessionDecodeFrame`
/// because the decode is submitted with neither
/// `kVTDecodeFrame_EnableAsynchronousDecompression` nor
/// `kVTDecodeFrame_EnableTemporalProcessing`.
///
/// The body must never unwind across the `extern "C-unwind"` boundary, so it
/// deliberately does no allocation beyond a single `CFRetain`, no formatting,
/// no indexing and no `unwrap`.
unsafe extern "C-unwind" fn output_callback(
    output_ref_con: *mut c_void,
    _source_ref_con: *mut c_void,
    status: OSStatus,
    info: VTDecodeInfoFlags,
    image: *mut CVImageBuffer,
    _pts: CMTime,
    _duration: CMTime,
) {
    // SAFETY: `output_ref_con` is the `DecodeSlot` pointer this decoder
    // registered in its `VTDecompressionOutputCallbackRecord`, and the slot's
    // heap allocation outlives the session (field order on
    // `VideoToolboxStillDecoder` drops the session first). The decode is
    // synchronous under `&mut self`, so this callback runs on the calling
    // thread inside `decode_frame` and no other reference to the slot is live.
    // `image` is a borrowed +0 buffer valid only for this call, so
    // `CFRetained::retain` takes the +1 needed to outlive it.
    unsafe {
        let slot = &mut *(output_ref_con.cast::<DecodeSlot>());
        slot.status = status;
        slot.info = info;
        slot.image = NonNull::new(image).map(|p| CFRetained::retain(p));
    }
}

// ── Session ─────────────────────────────────────────────────────────────────

/// A live decompression session and the format description it was created
/// from.
struct Session {
    /// The configuration record bytes this session was built for. Grid HEICs
    /// decode hundreds of tiles that share one `hvcC`, so a byte compare here
    /// skips both session *and* format-description construction.
    config: Vec<u8>,
    format: CFRetained<CMFormatDescription>,
    session: CFRetained<VTDecompressionSession>,
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: `session` was created successfully by this decoder and is
        // invalidated exactly once, here. VideoToolbox documents Invalidate as
        // the deterministic teardown to call before the last release, which
        // the `CFRetained` field performs immediately afterwards.
        unsafe { self.session.invalidate() };
    }
}

// ── Decoder ─────────────────────────────────────────────────────────────────

struct VideoToolboxStillDecoder {
    codec: HwCodec,
    /// Declared before `slot` so it is dropped first: invalidating the session
    /// severs the callback's reference to the slot before the slot's
    /// allocation goes away. **Reordering these two fields is a use-after-free.**
    session: Option<Session>,
    slot: Box<UnsafeCell<DecodeSlot>>,
}

// SAFETY: the decoder owns its decompression session, its format description
// and its callback slot exclusively — `decode_still` is the only entry point
// and takes `&mut self`, so no two threads can touch them at once.
// VideoToolbox sessions are not thread-affine: they require no run loop and no
// main thread, only that calls be externally serialised, which exclusive
// ownership guarantees. The output callback runs synchronously on the calling
// thread inside `decode_frame` (neither the asynchronous nor the
// temporal-processing flag is set, and every decode is followed by
// `wait_for_asynchronous_frames`), so the raw slot pointer is never
// dereferenced from another thread. `Send` is required by the
// [`HwStillDecoder`] contract; `Sync` is deliberately not claimed.
unsafe impl Send for VideoToolboxStillDecoder {}

impl VideoToolboxStillDecoder {
    fn new(codec: HwCodec) -> Self {
        Self {
            codec,
            session: None,
            slot: Box::new(UnsafeCell::new(DecodeSlot::default())),
        }
    }

    /// Get a session that can decode `format`, reusing the cached one when it
    /// can accept the picture.
    fn session_for(
        &mut self,
        config_bytes: &[u8],
        format: CFRetained<CMFormatDescription>,
    ) -> Result<&Session, HwDecodeError> {
        let reusable = match self.session.as_ref() {
            // Same configuration record: the fast path a grid HEIC or an AVIF
            // batch takes on every tile after the first.
            Some(existing) if existing.config == config_bytes => true,
            // Different record, but some decoders accommodate minor format
            // changes without a new session.
            Some(existing) => {
                // SAFETY: `existing.session` is a live session created by this
                // decoder and `format` a live description; the call only
                // queries the decoder and mutates nothing.
                unsafe { existing.session.can_accept_format_description(&format) }
            }
            None => false,
        };

        if reusable {
            let existing = self.session.as_mut().expect("checked above");
            existing.config = config_bytes.to_vec();
            existing.format = format;
            return Ok(self.session.as_ref().expect("just set"));
        }

        // Drop any old session before creating a new one, so two sessions
        // never hold decode resources at the same time.
        self.session = None;

        let specification = decoder_specification();
        let callback = VTDecompressionOutputCallbackRecord {
            decompressionOutputCallback: Some(output_callback),
            decompressionOutputRefCon: self.slot.get().cast::<c_void>(),
        };

        let destination = pixels::destination_attributes();

        let create_session = |spec: Option<&objc2_core_foundation::CFDictionary>| {
            let mut raw: *mut VTDecompressionSession = ptr::null_mut();
            // SAFETY: `format` is a live video format description. The callback
            // record is a live local that the framework copies during the
            // call, and its refcon points at this decoder's heap-allocated
            // slot, which outlives the session (see the field-order note on
            // the struct). `spec` and `destination` are live dictionaries of
            // CF types. `raw` is a live local the framework writes exactly one
            // +1 reference into on success.
            let status = unsafe {
                VTDecompressionSession::create(
                    None,
                    &format,
                    spec,
                    Some(format::as_opaque_dict(&destination)),
                    &raw const callback,
                    NonNull::from(&mut raw),
                )
            };
            (status, raw)
        };

        // Hardware-pinned first (macOS; on iOS the specification is None and
        // there is nothing to retry).
        let (mut status, mut raw) =
            create_session(specification.as_deref().map(format::as_opaque_dict));

        // Apple's hardware HEVC block refuses pictures below roughly 64x64,
        // failing session creation with `kVTVideoDecoderMalfunctionErr`. Small
        // items are entirely ordinary in HEIF — a `thmb` thumbnail is
        // routinely 32x32 — so refusing them outright would be a worse answer
        // than letting VideoToolbox use its own decoder for that one picture.
        // A failed hardware-pinned create is therefore retried once without
        // the requirement.
        //
        // This never manufactures support for a codec the machine cannot
        // decode: availability is still gated on `VTIsHardwareDecodeSupported`,
        // so the retry only widens the picture *sizes* an already-available
        // codec accepts. If it fails too, the original hardware status is what
        // gets reported, because that is the one that describes the real
        // problem.
        if status != NO_ERR && specification.is_some() {
            let hardware_status = status;
            let (retry_status, retry_raw) = create_session(None);
            if retry_status == NO_ERR {
                status = retry_status;
                raw = retry_raw;
            } else {
                return Err(map_status(
                    self.codec,
                    "VTDecompressionSessionCreate",
                    hardware_status,
                ));
            }
        }
        if status != NO_ERR {
            return Err(map_status(
                self.codec,
                "VTDecompressionSessionCreate",
                status,
            ));
        }
        let session_ptr = NonNull::new(raw).ok_or_else(|| {
            decode_err(
                self.codec,
                "VTDecompressionSessionCreate returned no session",
            )
        })?;
        // SAFETY: the call succeeded, so `session_ptr` carries the +1
        // reference the Create rule granted; `from_raw` adopts it and the
        // `Drop` impl invalidates then releases it exactly once.
        let session = unsafe { CFRetained::from_raw(session_ptr) };

        self.session = Some(Session {
            config: config_bytes.to_vec(),
            format,
            session,
        });
        Ok(self.session.as_ref().expect("just set"))
    }

    /// Submit one coded picture and copy the decoded surface out.
    fn decode(
        &mut self,
        config_bytes: &[u8],
        format: CFRetained<CMFormatDescription>,
        payload: &[u8],
    ) -> Result<DecodedFrame, HwDecodeError> {
        let codec = self.codec;

        // Reset the slot before submitting so a decoder that never calls back
        // cannot leave us reading the previous picture.
        // SAFETY: `&mut self` is exclusive and the callback has not run yet.
        unsafe { *self.slot.get() = DecodeSlot::default() };

        let session = self.session_for(config_bytes, format)?;
        let sample = sample::wrap(codec, &session.format, payload)?;

        // SAFETY: `session` was created with this decoder's callback record,
        // and the sample buffer is ready and described by the very format
        // description the session holds. A null source refcon and a null
        // info-flags out-pointer are both explicitly permitted. No
        // asynchronous or temporal-processing flag is set, so the output
        // callback runs before this returns.
        let status = unsafe {
            session.session.decode_frame(
                &sample.buffer,
                VTDecodeFrameFlags::empty(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status != NO_ERR {
            return Err(map_status(
                codec,
                "VTDecompressionSessionDecodeFrame",
                status,
            ));
        }

        // A no-op for a synchronous decode, and a hard barrier for a decoder
        // that went asynchronous anyway. This is what makes the zero-copy
        // block buffer in `sample::wrap` sound.
        // SAFETY: `session` is live and owned by this decoder.
        let wait = unsafe { session.session.wait_for_asynchronous_frames() };
        if wait != NO_ERR {
            return Err(map_status(
                codec,
                "VTDecompressionSessionWaitForAsynchronousFrames",
                wait,
            ));
        }

        // SAFETY: `&mut self` is exclusive and, per the flags above, the
        // callback has finished running on this thread.
        let slot = unsafe { &mut *self.slot.get() };
        if slot.status != NO_ERR {
            return Err(map_status(
                codec,
                "VideoToolbox decode callback",
                slot.status,
            ));
        }
        if slot
            .info
            .intersects(VTDecodeInfoFlags::FrameDropped | VTDecodeInfoFlags::FrameInterrupted)
        {
            return Err(decode_err(
                codec,
                "VideoToolbox dropped or interrupted the frame without producing pixels",
            ));
        }
        let image = slot
            .image
            .take()
            .ok_or_else(|| decode_err(codec, "VideoToolbox produced no image buffer"))?;

        pixels::to_decoded_frame(codec, &image)
    }
}

impl HwStillDecoder for VideoToolboxStillDecoder {
    fn decode_still(
        &mut self,
        request: &StillDecodeRequest<'_>,
    ) -> Result<DecodedFrame, HwDecodeError> {
        if request.codec() != self.codec {
            return Err(HwDecodeError::Decode {
                codec: request.codec(),
                message: format!("decoder was opened for {}", self.codec),
            });
        }
        // Cheapest check first: without a payload there is nothing to describe
        // a format for, let alone decode.
        if request.payload.is_empty() {
            return Err(decode_err(self.codec, "coded picture payload is empty"));
        }
        match request.config {
            CodecConfig::Hvcc(hvcc) => {
                let format = format::hevc_format_description(hvcc, request)?;
                self.decode(hvcc, format, request.payload)
            }
            CodecConfig::Av1c(av1c) => {
                let format = format::av1_format_description(av1c, request)?;
                self.decode(av1c, format, request.payload)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probe must never panic and must agree with itself across calls,
    /// on any Apple machine including one with no decode block at all.
    #[test]
    fn probe_is_infallible_and_cached() {
        let first = probe();
        let second = probe();
        assert_eq!(first.hevc, second.hevc);
        assert_eq!(first.av1, second.av1);
    }

    /// The invariant `crate::tests::discovery_entry_points_are_consistent`
    /// enforces, checked against this backend's own `Caps` so a future edit to
    /// one of the three hooks cannot drift from the others.
    #[test]
    fn discovery_hooks_agree_with_caps() {
        let caps = probe();
        let codecs = available_codecs();
        assert_eq!(codecs.contains(&HwCodec::Hevc), caps.hevc);
        assert_eq!(codecs.contains(&HwCodec::Av1), caps.av1);
        assert_eq!(backend().is_some(), !codecs.is_empty());
        assert_eq!(decoder(HwCodec::Hevc).is_some(), caps.hevc);
        assert_eq!(decoder(HwCodec::Av1).is_some(), caps.av1);
    }

    /// A decoder opened for one codec must reject the other rather than
    /// building a format description for a session that cannot take it.
    #[test]
    fn decoder_rejects_the_other_codec() {
        let mut decoder = VideoToolboxStillDecoder::new(HwCodec::Hevc);
        let request = StillDecodeRequest {
            config: CodecConfig::Av1c(&[0x81, 0x00, 0x0c, 0x00]),
            payload: &[0x12, 0x00],
            width: 64,
            height: 64,
            bit_depth: 8,
            chroma: crate::ChromaSubsampling::Cs420,
        };
        let err = decoder
            .decode_still(&request)
            .expect_err("codec mismatch must be rejected");
        assert!(err.to_string().contains("opened for HEVC"), "{err}");
    }

    /// An empty payload must be refused before any framework call.
    #[test]
    fn empty_payload_is_rejected() {
        let mut decoder = VideoToolboxStillDecoder::new(HwCodec::Av1);
        let request = StillDecodeRequest {
            config: CodecConfig::Av1c(&[0x81, 0x00, 0x0c, 0x00]),
            payload: &[],
            width: 64,
            height: 64,
            bit_depth: 8,
            chroma: crate::ChromaSubsampling::Cs420,
        };
        let err = decoder
            .decode_still(&request)
            .expect_err("an empty payload must be rejected");
        assert!(err.to_string().contains("empty"), "{err}");
    }
}
