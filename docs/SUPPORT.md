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

rawshift decodes HEVC (HEIC) and AV1 (AVIF) still-frame codestreams through
platform hardware decoders in the `rawshift-hwdec` crate. The API set is
fixed:

| API | Status | Platforms | HEVC | AV1 | Linking |
| --- | --- | --- | --- | --- | --- |
| VideoToolbox | ✅ in | macOS 11+, iOS 14+ | ✅ | ✅ runtime-probed (M3+ / A17 Pro+ hardware) | system framework |
| VAAPI (libva) | ✅ in | Linux (gnu) | ✅ Main / Main10 | ✅ AV1 Main (driver-dependent) | dlopen at runtime — absence degrades to "no decoder", never a link failure |
| MediaCodec (NDK) | ✅ in | Android | ✅ | ✅ (device codec; mandated on newer API levels) | NDK |

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

### Software decode

- **HEVC: never.** No acceptable pure-Rust implementation exists, and the
  patent posture rules out shipping one.
- **AV1: post-v1.** gamut's planned pure-Rust AV1 still decoder will restore
  AVIF decode on Windows, musl, and wasm as a software fallback.

### Behavior without a hardware decoder

Container parsing, metadata, and auxiliary-image enumeration always work on
every target. Pixel decode without a compiled-in, runtime-probed backend
fails with the matchable `RawError::HwDecoderUnavailable`; capability is
exposed honestly via `heic_hw_decode_available()` /
`avif_hw_decode_available()` and the codec registry.
