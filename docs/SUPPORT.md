# Support Matrix (Permanent)

This document fixes rawshift's supported compilation targets and hardware
decode APIs. It was decided once, at v1, and is **not** expected to change:
do not add or remove targets or APIs. Every exclusion carries its
justification.

## MSRV

The minimum supported Rust version tracks the minimum required by upstream
dependencies (currently **1.92.0**, set by [gamut]) and stays as low as the
upstream dependencies require — it is never raised independently.

[gamut]: https://github.com/visualcommons/gamut

## Compilation targets

| Target | Tier | Hardware decode | Notes |
| --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | 1 (CI build + test) | VAAPI (runtime dlopen) | primary development target |
| `aarch64-unknown-linux-gnu` | 1 (CI build + test) | VAAPI (runtime dlopen) | |
| `aarch64-apple-darwin` | 1 (CI build + test) | VideoToolbox | |
| `aarch64-apple-ios` | 1 (CI build) | VideoToolbox | device tests are manual |
| `aarch64-linux-android` | 1 (CI build) | MediaCodec | minimum API level fixed when the backend lands |
| `x86_64-pc-windows-msvc` | 1 (CI build + test) | none (justified below) | HEIC/AVIF pixel decode unavailable until a software AV1 decoder lands upstream; everything else works |
| `x86_64-unknown-linux-musl` | 2 (CI build) | none | static deploys; no dlopen |
| `wasm32-unknown-unknown` | 2 (CI build) | none | in-memory API only; no hardware decode, threads, or file IO |

Anything not listed is unsupported: it may work, but carries no commitment.
The list is intentionally minimal.

## Hardware decode APIs

rawshift decodes codestreams through platform hardware decoders in the
`rawshift-hwdec` crate, on two seams: **still frames** (HEVC for HEIC, AV1 for
AVIF) and **coded video sequences** (HEVC and H.264, for `rawshift-video`).
The API set is fixed:

| API | Status | Platforms | HEVC | AV1 | H.264 | Linking |
| --- | --- | --- | --- | --- | --- | --- |
| VideoToolbox | ✅ in | macOS 11+, iOS 14+ | ✅ | ✅ runtime-probed (M3+ / A17 Pro+ hardware) | still: n/a · sequence: pending | system framework |
| VAAPI (libva) | ✅ in | Linux (gnu) | ✅ Main / Main10 | ✅ AV1 Main (driver-dependent) | ✅ sequence only (Constrained Baseline / Main / High, 8-bit 4:2:0) | dlopen at runtime — absence degrades to "no decoder", never a link failure |
| MediaCodec (NDK) | ✅ in | Android | ✅ | ✅ (device codec; mandated on newer API levels) | still: n/a · sequence: pending | NDK |

**Seam scope.** H.264 exists only on the sequence seam: `decoder(HwCodec::H264)`
returns `None` and `available_codecs()` never lists it, because no rawshift
still-image format uses H.264 and shipping an untested path would be dishonest.
Conversely AV1 exists only on the still seam today; AV1 video sequences are a
post-v1 follow-up over the same backend.

**Progressive frames only.** The sequence seam rejects H.264 `field_pic_flag = 1`
(PAFF / interlaced field coding) with a clear error. Every camera-origin source
in rawshift's device list shoots progressive frames, and paired-field reference
management roughly doubles the decoder state machine for no covered use case.
MBAFF is accepted — it is frame coding and does not affect the picture buffer.

VAAPI covers Intel and AMD natively, **and NVIDIA via the maintained
[`nvidia-vaapi-driver`](https://github.com/elFarto/nvidia-vaapi-driver)
translation layer over NVDEC**.

### Excluded APIs, with justification

- **Windows Media Foundation** — deliberate scope decision: HEVC decode
  depends on the paid "HEVC Video Extensions" store codec pack, and the
  COM/MFT integration surface is disproportionate for a single target.
  Windows AVIF decode recovers post-v1 through gamut's planned pure-Rust AV1
  decoder.
- **NVDEC (NVIDIA Video Codec SDK)** — (1) on Linux, NVDEC is already
  reachable through our VAAPI entry point via `nvidia-vaapi-driver`, so a
  native backend duplicates coverage; (2) on Windows the SDK would be the
  only path (MF is excluded) and its proprietary EULA sits poorly with
  MPL-2.0 redistribution, while adding CUDA/driver FFI for one vendor;
  (3) still-image decode is single-frame — vendor-specific throughput gains
  do not materialize. Net new coverage would be NVIDIA-on-Windows only,
  which the software AV1 fallback serves post-v1.
- **D3D12 / Vulkan Video** — pre-1.0 API churn (Vulkan Video), the largest
  implementation surface of all options, and no coverage gain over the three
  chosen APIs on any tier-1 target.
- **FFmpeg / libav** — excluded as a dependency in any form, including as an
  optional feature. (1) **License:** FFmpeg is LGPL-2.1+ (GPL once `x264`/`x265`
  are enabled). rawshift is MPL-2.0, and static linking an LGPL library into an
  iOS application binary leaves the end user no relinking path — the same
  redistribution concern that excluded NVDEC above. (2) **Portability:** it
  needs autotools plus `nasm`/`yasm` at build time, which cannot be satisfied on
  `wasm32-unknown-unknown` at all and would require cross C toolchains in the
  `aarch64-apple-ios` and `aarch64-linux-android` build lanes, breaking the
  "builds anywhere `cargo` does" property the target list above depends on.
  (3) **Coverage:** the codecs rawshift's device list actually produces
  (H.264, HEVC, ProRes) are served by platform hardware decoders that the OS
  already licenses. Demuxing is served by `symphonia`, which is pure Rust and
  MPL-2.0. A `ffmpeg-sidecar`-style subprocess is likewise out of scope: it
  moves the license and distribution problem onto the consuming application
  rather than solving it.

### Software decode

- **HEVC: never.** No acceptable pure-Rust implementation exists, and the
  patent posture rules out shipping one. This covers video sequences as well as
  still frames.
- **H.264: never**, for the same reason. H.264 is patent-encumbered
  independently of any implementation's own license, and the Cisco OpenH264
  royalty grant covers Cisco's *prebuilt binary*, not a from-source build and
  not a clean-slate reimplementation. The pure-Rust `rusty_h264` and `rust_h265`
  crates were evaluated in August 2026 and rejected on this ground and on
  maturity (both are months old and pre-1.0). Hardware decoders are the answer:
  the device OEM already holds the licence.
- **AV1: post-v1.** gamut's planned pure-Rust AV1 still decoder will restore
  AVIF decode on Windows, musl, and wasm as a software fallback.
- **ProRes: undecided.** Unlike H.264/HEVC the bitstream is publicly specified
  (SMPTE RDD 36) and intra-only, so a clean-slate safe-Rust decoder is
  technically open. Not scoped for v1.

### Behavior without a hardware decoder

Container parsing, metadata, and auxiliary-image enumeration always work on
every target. Pixel decode without a compiled-in, runtime-probed backend
fails with the matchable `RawError::HwDecoderUnavailable`; capability is
exposed honestly via `heic_hw_decode_available()` /
`avif_hw_decode_available()` and the codec registry.

The same contract holds for video: container parsing, track enumeration,
timeline, and metadata always work on every target, including `wasm32` and
`musl` where no hardware decoder can exist. Only frame decode requires a
backend, and its absence surfaces as the matchable
`VideoError::HwDecoderUnavailable` with `rawshift_video::hw_decode_available()`
reporting capability up front.
