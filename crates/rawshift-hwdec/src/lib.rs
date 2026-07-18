//! # rawshift-hwdec
//!
//! Hardware still-frame codestream decode for rawshift: HEVC (HEIC) and AV1
//! (AVIF) intra pictures through the platform decode APIs fixed in
//! [`docs/SUPPORT.md`] — VideoToolbox (macOS/iOS), VAAPI (linux-gnu, dlopen'd
//! at runtime), and NDK MediaCodec (Android).
//!
//! ## Safety boundary
//!
//! Per `PRINCIPLES.md`, **all** platform FFI for hardware decode lives in this
//! crate and nowhere else: `#![deny(unsafe_op_in_unsafe_fn)]`, every public
//! item is safe, and every future `unsafe` block must document its invariants
//! inside the platform backend module that owns it. This revision ships **no
//! platform code at all** (and therefore no `unsafe`): the backends land as
//! separate issues (VAAPI is next), and until one lands every entry point
//! reports "no decoder" — [`decoder`] returns `None`, [`backend`] returns
//! `None`, and [`available_codecs`] is empty.
//!
//! ## Verified feature flags
//!
//! - `hw` — portable: selects the native backend for the compile target via
//!   the build script; on a target with no hardware decode API
//!   (windows-msvc, linux-musl, wasm — see the SUPPORT.md matrix) it emits a
//!   build warning and compiles this stub. Valid everywhere.
//! - `videotoolbox` / `vaapi` / `mediacodec` — pin one explicit backend;
//!   **`compile_error!` on any other target**. For example, this fails to
//!   compile anywhere but Apple platforms (a compile-fail test matrix in CI
//!   is tracked separately):
//!
//!   ```text
//!   cargo check -p rawshift-hwdec --features videotoolbox   # non-Apple host
//!   error: rawshift-hwdec: the `videotoolbox` feature requires an Apple target ...
//!   ```
//!
//! ## Request/response contract
//!
//! The request is misuse-resistant by construction: the codec is implied by
//! the [`CodecConfig`] variant (an `hvcC` config with an OBU payload, or an
//! `av1C` config claiming to be HEVC, is unrepresentable), and
//! [`DecodedFrame::new`] validates plane geometry against the pixel format so
//! an internally inconsistent frame cannot be constructed.
//!
//! [`docs/SUPPORT.md`]: https://github.com/justin13888/rawshift/blob/master/docs/SUPPORT.md

#![deny(unsafe_op_in_unsafe_fn)]

// ── Verified feature boundaries ─────────────────────────────────────────────
// Explicit backend flags are hard errors on targets whose platform API does
// not exist — a mis-pinned build must fail at compile time, not degrade
// silently. The portable `hw` flag never errors; build.rs handles it.

#[cfg(all(
    feature = "videotoolbox",
    not(any(target_os = "macos", target_os = "ios"))
))]
compile_error!(
    "rawshift-hwdec: the `videotoolbox` feature requires an Apple target \
     (macOS or iOS). Use the portable `hw` feature to select the native \
     backend for the compile target, or see docs/SUPPORT.md."
);

#[cfg(all(feature = "vaapi", not(all(target_os = "linux", target_env = "gnu"))))]
compile_error!(
    "rawshift-hwdec: the `vaapi` feature requires a linux-gnu target (VAAPI \
     is dlopen'd; musl has no dlopen deployment story — see docs/SUPPORT.md). \
     Use the portable `hw` feature to select the native backend for the \
     compile target."
);

#[cfg(all(feature = "mediacodec", not(target_os = "android")))]
compile_error!(
    "rawshift-hwdec: the `mediacodec` feature requires an Android target \
     (NDK MediaCodec). Use the portable `hw` feature to select the native \
     backend for the compile target, or see docs/SUPPORT.md."
);

use thiserror::Error;

pub use gamut_color::{ChromaSubsampling, ColorRange};

// ── Codec / backend identity ────────────────────────────────────────────────

/// A codec this crate can decode still frames of.
///
/// The set is fixed at v1 (see `docs/SUPPORT.md`): HEVC for HEIC and AV1 for
/// AVIF. It is deliberately exhaustive — a new codec is a deliberate,
/// breaking decision, not an additive one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HwCodec {
    /// HEVC / H.265 intra (HEIC still images).
    Hevc,
    /// AV1 intra (AVIF still images).
    Av1,
}

impl HwCodec {
    /// The conventional display name of the codec (`"HEVC"` / `"AV1"`).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            HwCodec::Hevc => "HEVC",
            HwCodec::Av1 => "AV1",
        }
    }
}

impl std::fmt::Display for HwCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A platform hardware-decode API — the fixed set from `docs/SUPPORT.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HwBackend {
    /// Apple VideoToolbox (macOS 11+, iOS 14+).
    VideoToolbox,
    /// VAAPI / libva on linux-gnu, dlopen'd at runtime (absence degrades to
    /// "no decoder", never a link failure).
    Vaapi,
    /// Android NDK MediaCodec.
    MediaCodec,
}

impl HwBackend {
    /// The conventional display name of the backend.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            HwBackend::VideoToolbox => "VideoToolbox",
            HwBackend::Vaapi => "VAAPI",
            HwBackend::MediaCodec => "MediaCodec",
        }
    }
}

impl std::fmt::Display for HwBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

// ── Decode request ──────────────────────────────────────────────────────────

/// The codec configuration record of a still-decode request. **The variant is
/// the codec**: an [`Hvcc`](CodecConfig::Hvcc) request is an HEVC request and
/// an [`Av1c`](CodecConfig::Av1c) request is an AV1 request, so a mismatched
/// codec/config pair is unrepresentable.
///
/// The bytes are the raw record **body** as stored in the container property
/// (the `hvcC` HEVCDecoderConfigurationRecord per ISO/IEC 14496-15 §8.3.3.1,
/// or the `av1C` AV1CodecConfigurationRecord), without any box header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecConfig<'a> {
    /// An `hvcC` HEVCDecoderConfigurationRecord body. The matching
    /// [`StillDecodeRequest::payload`] is the length-prefixed NAL stream of
    /// the coded item.
    Hvcc(&'a [u8]),
    /// An `av1C` AV1CodecConfigurationRecord body. The matching
    /// [`StillDecodeRequest::payload`] is the OBU stream of the coded item.
    Av1c(&'a [u8]),
}

impl<'a> CodecConfig<'a> {
    /// The codec this configuration record belongs to — implied by the
    /// variant, never stored separately.
    #[must_use]
    pub fn codec(&self) -> HwCodec {
        match self {
            CodecConfig::Hvcc(_) => HwCodec::Hevc,
            CodecConfig::Av1c(_) => HwCodec::Av1,
        }
    }

    /// The raw configuration-record body bytes.
    #[must_use]
    pub fn bytes(&self) -> &'a [u8] {
        match self {
            CodecConfig::Hvcc(bytes) | CodecConfig::Av1c(bytes) => bytes,
        }
    }
}

/// One still-frame decode request: the codec configuration (which implies the
/// codec), the coded payload, and the caller's advisory picture description.
///
/// `payload` framing follows the config variant: a length-prefixed NAL stream
/// for [`CodecConfig::Hvcc`] (the prefix width is inside the `hvcC` record),
/// an OBU stream for [`CodecConfig::Av1c`].
///
/// `width`/`height`/`bit_depth`/`chroma` are **advisory**: they describe what
/// the container claims (`ispe`, `hvcC`/`av1C` fields) so a backend can size
/// its output surfaces up front, but the coded bitstream is authoritative and
/// the returned [`DecodedFrame`] carries the real geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StillDecodeRequest<'a> {
    /// The codec configuration record; its variant is the codec.
    pub config: CodecConfig<'a>,
    /// The coded picture payload (length-prefixed NAL units or OBUs — see the
    /// type-level docs).
    pub payload: &'a [u8],
    /// Advisory coded picture width in pixels (0 = unknown).
    pub width: u32,
    /// Advisory coded picture height in pixels (0 = unknown).
    pub height: u32,
    /// Advisory per-sample bit depth (from the configuration record).
    pub bit_depth: u8,
    /// Advisory chroma subsampling (from the configuration record).
    pub chroma: ChromaSubsampling,
}

impl StillDecodeRequest<'_> {
    /// The codec of this request — implied by the configuration variant.
    #[must_use]
    pub fn codec(&self) -> HwCodec {
        self.config.codec()
    }
}

// ── Decoded frame ───────────────────────────────────────────────────────────

/// The pixel layout of a [`DecodedFrame`] — the four surface formats the
/// platform decoders emit for 4:2:0 content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// 8-bit 4:2:0, biplanar: full-size Y plane + half-size interleaved CbCr
    /// plane.
    Nv12,
    /// 10..16-bit 4:2:0, biplanar: like [`Nv12`](Self::Nv12) but 16-bit
    /// little-endian words with the sample value in the **most** significant
    /// bits (the VideoToolbox / DXGI `P010` convention).
    P010,
    /// 8-bit 4:2:0, planar: full-size Y plane + half-size Cb and Cr planes.
    I420,
    /// 10..16-bit 4:2:0, planar: like [`I420`](Self::I420) but 16-bit
    /// little-endian words with the sample value in the **least** significant
    /// bits.
    I010,
}

impl PixelFormat {
    /// The number of planes this format carries (2 for biplanar NV12/P010,
    /// 3 for planar I420/I010).
    #[must_use]
    pub fn plane_count(self) -> usize {
        match self {
            PixelFormat::Nv12 | PixelFormat::P010 => 2,
            PixelFormat::I420 | PixelFormat::I010 => 3,
        }
    }

    /// The storage size of one sample in bytes (1 for the 8-bit formats, 2
    /// for the 16-bit-word formats).
    #[must_use]
    pub fn bytes_per_sample(self) -> usize {
        match self {
            PixelFormat::Nv12 | PixelFormat::I420 => 1,
            PixelFormat::P010 | PixelFormat::I010 => 2,
        }
    }

    /// Whether `bit_depth` is representable in this format: exactly 8 for the
    /// 8-bit formats, `9..=16` for the 16-bit-word formats.
    #[must_use]
    pub fn supports_bit_depth(self, bit_depth: u8) -> bool {
        match self {
            PixelFormat::Nv12 | PixelFormat::I420 => bit_depth == 8,
            PixelFormat::P010 | PixelFormat::I010 => (9..=16).contains(&bit_depth),
        }
    }
}

/// One image plane of a [`DecodedFrame`]: raw bytes plus the row stride.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plane {
    /// The raw plane bytes, row-major. Sample encoding follows the frame's
    /// [`PixelFormat`].
    pub data: Vec<u8>,
    /// The distance between the starts of two consecutive rows, in **bytes**
    /// (at least the row's used width; may be larger for aligned surfaces).
    pub stride: usize,
}

/// The validated output of a [`HwStillDecoder`]: a decoded 4:2:0 (or
/// monochrome-in-4:2:0) picture in one of the four platform surface layouts.
///
/// Construct with [`DecodedFrame::new`], which checks the plane count and
/// every plane's geometry against `format`/`width`/`height`, and the
/// `bit_depth` against the format's sample width — an internally inconsistent
/// frame is unrepresentable, so consumers can trust the plane sizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    format: PixelFormat,
    width: u32,
    height: u32,
    bit_depth: u8,
    range: ColorRange,
    planes: Vec<Plane>,
}

impl DecodedFrame {
    /// Builds a frame from its planes, validating internal consistency.
    ///
    /// `width`/`height` are the luma dimensions (post conformance-window
    /// crop). Chroma planes use **ceiling** division on the subsampled axes.
    ///
    /// # Errors
    ///
    /// Returns [`HwDecodeError::InvalidFrame`] if either dimension is zero,
    /// `bit_depth` does not fit `format` (see
    /// [`PixelFormat::supports_bit_depth`]), the plane count is wrong, a
    /// stride is smaller than the row's used width, or a plane buffer is too
    /// short for its rows.
    pub fn new(
        format: PixelFormat,
        width: u32,
        height: u32,
        bit_depth: u8,
        range: ColorRange,
        planes: Vec<Plane>,
    ) -> Result<Self, HwDecodeError> {
        let invalid = |message: &str| HwDecodeError::InvalidFrame {
            message: message.to_string(),
        };
        if width == 0 || height == 0 {
            return Err(invalid("frame dimensions must be non-zero"));
        }
        if !format.supports_bit_depth(bit_depth) {
            return Err(invalid(
                "bit depth is not representable in the pixel format",
            ));
        }
        if planes.len() != format.plane_count() {
            return Err(invalid("plane count does not match the pixel format"));
        }

        let bps = format.bytes_per_sample();
        let (cw, ch) = (width.div_ceil(2) as usize, height.div_ceil(2) as usize);
        for (index, plane) in planes.iter().enumerate() {
            // Plane geometry: index 0 is luma; for biplanar formats index 1 is
            // the interleaved CbCr plane (two samples per chroma column); for
            // planar formats indexes 1/2 are the Cb/Cr planes.
            let (row_samples, rows) = match (format.plane_count(), index) {
                (_, 0) => (width as usize, height as usize),
                (2, 1) => (cw * 2, ch),
                (3, 1 | 2) => (cw, ch),
                // Unreachable: plane count was validated above.
                _ => unreachable!("plane index out of range for format"),
            };
            let row_bytes = row_samples
                .checked_mul(bps)
                .ok_or_else(|| invalid("plane row size overflows"))?;
            if plane.stride < row_bytes {
                return Err(invalid("plane stride is smaller than the row width"));
            }
            let required = plane
                .stride
                .checked_mul(rows - 1)
                .and_then(|n| n.checked_add(row_bytes))
                .ok_or_else(|| invalid("plane size overflows"))?;
            if plane.data.len() < required {
                return Err(invalid("plane buffer is shorter than its rows"));
            }
        }

        Ok(Self {
            format,
            width,
            height,
            bit_depth,
            range,
            planes,
        })
    }

    /// The pixel layout of the planes.
    #[must_use]
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// The luma width in pixels (post conformance-window crop).
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The luma height in pixels (post conformance-window crop).
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The per-sample bit depth (8 for NV12/I420, 9..=16 for P010/I010).
    #[must_use]
    pub fn bit_depth(&self) -> u8 {
        self.bit_depth
    }

    /// The signal range of the samples (limited/studio vs full swing).
    #[must_use]
    pub fn range(&self) -> ColorRange {
        self.range
    }

    /// The planes, in format order (Y, then CbCr or Cb/Cr).
    #[must_use]
    pub fn planes(&self) -> &[Plane] {
        &self.planes
    }
}

// ── Decoder seam ────────────────────────────────────────────────────────────

/// A hardware still-frame decoder for one codec, obtained from [`decoder`].
///
/// `&mut self`: platform decoder sessions are stateful (reused contexts,
/// surface pools). `Send`: a decoder may be moved to a worker thread; none of
/// the planned backends hand out thread-affine handles.
pub trait HwStillDecoder: Send {
    /// Decodes one coded still picture to a [`DecodedFrame`].
    ///
    /// # Errors
    ///
    /// Returns [`HwDecodeError`] when the backend rejects the configuration,
    /// the bitstream is malformed, or the platform decoder fails.
    fn decode_still(
        &mut self,
        request: &StillDecodeRequest<'_>,
    ) -> Result<DecodedFrame, HwDecodeError>;
}

/// Errors reported by hardware still-frame decode.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HwDecodeError {
    /// No hardware decoder is available for the codec — none compiled in, or
    /// the runtime probe (driver, entry point, codec support) failed.
    #[error("no hardware decoder available for {codec}: {reason}")]
    Unavailable {
        /// The codec that was requested.
        codec: HwCodec,
        /// Why no decoder is available (which probe failed, or that no
        /// backend is compiled in).
        reason: String,
    },
    /// The platform decoder accepted the job and then failed.
    #[error("hardware {codec} decode failed: {message}")]
    Decode {
        /// The codec being decoded.
        codec: HwCodec,
        /// The backend's failure description.
        message: String,
    },
    /// A [`DecodedFrame`] failed validation (backend bug or corrupted
    /// output).
    #[error("invalid decoded frame: {message}")]
    InvalidFrame {
        /// What was inconsistent.
        message: String,
    },
}

// ── Backend discovery (stub until the platform backends land) ───────────────

/// Returns a decoder for `codec`, or `None` when no compiled-in backend can
/// decode it at runtime.
///
/// No platform backend is implemented yet (they land as separate issues;
/// VAAPI is next), so this currently returns `None` on every target and
/// feature combination.
#[must_use]
pub fn decoder(codec: HwCodec) -> Option<Box<dyn HwStillDecoder>> {
    let _ = codec;
    None
}

/// The platform backend compiled into this build and usable at runtime, or
/// `None`.
///
/// No platform backend is implemented yet, so this currently returns `None`
/// everywhere; once a backend lands it reports `Some` only when the runtime
/// probe succeeds (e.g. VAAPI's dlopen finding a usable driver).
#[must_use]
pub fn backend() -> Option<HwBackend> {
    None
}

/// The codecs [`decoder`] can currently return a decoder for.
///
/// Empty until a platform backend lands.
#[must_use]
pub fn available_codecs() -> &'static [HwCodec] {
    &[]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── stub behaviour ──────────────────────────────────────────────────────

    #[test]
    fn stub_has_no_decoder_for_any_codec() {
        assert!(decoder(HwCodec::Hevc).is_none());
        assert!(decoder(HwCodec::Av1).is_none());
    }

    #[test]
    fn stub_reports_no_backend_and_no_codecs() {
        assert_eq!(backend(), None);
        assert!(available_codecs().is_empty());
    }

    // ── request construction ────────────────────────────────────────────────

    #[test]
    fn codec_is_implied_by_the_config_variant() {
        let hvcc = [0x01u8, 0x02];
        let av1c = [0x81u8];
        assert_eq!(CodecConfig::Hvcc(&hvcc).codec(), HwCodec::Hevc);
        assert_eq!(CodecConfig::Av1c(&av1c).codec(), HwCodec::Av1);
        assert_eq!(CodecConfig::Hvcc(&hvcc).bytes(), &hvcc);

        let request = StillDecodeRequest {
            config: CodecConfig::Hvcc(&hvcc),
            payload: &[0, 0, 0, 1, 0x26],
            width: 64,
            height: 48,
            bit_depth: 8,
            chroma: ChromaSubsampling::Cs420,
        };
        assert_eq!(request.codec(), HwCodec::Hevc);
    }

    // ── frame validation ────────────────────────────────────────────────────

    fn plane(len: usize, stride: usize) -> Plane {
        Plane {
            data: vec![0u8; len],
            stride,
        }
    }

    #[test]
    fn i420_frame_validates() {
        // 5x3 luma → 3x2 chroma (ceiling division).
        let frame = DecodedFrame::new(
            PixelFormat::I420,
            5,
            3,
            8,
            ColorRange::Limited,
            vec![plane(15, 5), plane(6, 3), plane(6, 3)],
        )
        .expect("valid I420 frame");
        assert_eq!(frame.format(), PixelFormat::I420);
        assert_eq!((frame.width(), frame.height()), (5, 3));
        assert_eq!(frame.planes().len(), 3);
    }

    #[test]
    fn nv12_frame_validates_with_padded_stride() {
        // 4x4 luma with a 8-byte stride; interleaved CbCr plane is 2x2 pairs.
        let frame = DecodedFrame::new(
            PixelFormat::Nv12,
            4,
            4,
            8,
            ColorRange::Full,
            vec![plane(8 * 3 + 4, 8), plane(4 * 2, 4)],
        )
        .expect("valid NV12 frame");
        assert_eq!(frame.range(), ColorRange::Full);
        assert_eq!(frame.planes()[0].stride, 8);
    }

    #[test]
    fn p010_requires_high_bit_depth_and_i420_requires_8() {
        // P010 with bit depth 8 is inconsistent.
        let err = DecodedFrame::new(
            PixelFormat::P010,
            2,
            2,
            8,
            ColorRange::Limited,
            vec![plane(8, 4), plane(4, 4)],
        )
        .unwrap_err();
        assert!(matches!(err, HwDecodeError::InvalidFrame { .. }));

        // I420 with bit depth 10 is inconsistent.
        assert!(
            DecodedFrame::new(
                PixelFormat::I420,
                2,
                2,
                10,
                ColorRange::Limited,
                vec![plane(4, 2), plane(1, 1), plane(1, 1)],
            )
            .is_err()
        );

        // P010 at 10 bits with correctly sized 16-bit planes is fine.
        assert!(
            DecodedFrame::new(
                PixelFormat::P010,
                2,
                2,
                10,
                ColorRange::Limited,
                vec![plane(8, 4), plane(4, 4)],
            )
            .is_ok()
        );
    }

    #[test]
    fn frame_rejects_wrong_plane_count_and_short_planes() {
        // I420 with two planes.
        assert!(
            DecodedFrame::new(
                PixelFormat::I420,
                2,
                2,
                8,
                ColorRange::Limited,
                vec![plane(4, 2), plane(1, 1)],
            )
            .is_err()
        );
        // Luma plane one byte short.
        assert!(
            DecodedFrame::new(
                PixelFormat::I420,
                2,
                2,
                8,
                ColorRange::Limited,
                vec![plane(3, 2), plane(1, 1), plane(1, 1)],
            )
            .is_err()
        );
        // Stride smaller than the row width.
        assert!(
            DecodedFrame::new(
                PixelFormat::I420,
                4,
                2,
                8,
                ColorRange::Limited,
                vec![plane(8, 3), plane(2, 2), plane(2, 2)],
            )
            .is_err()
        );
        // Zero dimensions.
        assert!(
            DecodedFrame::new(
                PixelFormat::I420,
                0,
                2,
                8,
                ColorRange::Limited,
                vec![plane(0, 0), plane(0, 0), plane(0, 0)],
            )
            .is_err()
        );
    }

    // ── error surface ───────────────────────────────────────────────────────

    #[test]
    fn errors_name_the_codec() {
        let err = HwDecodeError::Unavailable {
            codec: HwCodec::Hevc,
            reason: "no backend compiled in".to_string(),
        };
        let text = err.to_string();
        assert!(text.contains("HEVC"), "{text}");
        assert!(text.contains("no backend compiled in"), "{text}");

        assert_eq!(HwCodec::Av1.to_string(), "AV1");
        assert_eq!(HwBackend::Vaapi.to_string(), "VAAPI");
    }
}
