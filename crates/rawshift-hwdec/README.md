# rawshift-hwdec

Hardware still-frame codestream decode for [rawshift](https://github.com/visualcommons/rawshift):
HEVC (HEIC) and AV1 (AVIF) intra pictures through the platform decode APIs
fixed permanently in [`docs/SUPPORT.md`](../../docs/SUPPORT.md) —
VideoToolbox (macOS/iOS), VAAPI (linux-gnu, dlopen'd at runtime), and NDK
MediaCodec (Android).

This is the **only** crate in the workspace where platform FFI may live
(`#![deny(unsafe_op_in_unsafe_fn)]`, safe public items, documented invariants
on every unsafe block). The **VideoToolbox and VAAPI backends are
implemented**; MediaCodec lands as a separate issue. On builds/targets with no
backend the crate compiles a no-backend stub: `decoder()` returns `None`,
`backend()` returns `None`, `available_codecs()` is empty, and dependants
surface `HwDecoderUnavailable`.

## VideoToolbox backend (macOS / iOS)

VideoToolbox, CoreMedia and CoreVideo are always-present system frameworks, so
there is nothing to dlopen and no device to find: availability is answered by
`VTIsHardwareDecodeSupported` per codec, cached once per process.

Still-picture scope:

| Codec | Profiles | Output |
| --- | --- | --- |
| HEVC (HEIC) | Main, Main 10 — `hvcC` parameter sets, length-prefixed payload passed through unchanged (no Annex-B conversion) | NV12 (8-bit), P010 (10-bit) |
| AV1 (AVIF) | Profile 0 (Main) — `av1C` config atom, raw OBU temporal unit; **runtime-probed**, since AV1 hardware decode arrived with M3 / A17 Pro | NV12 (8-bit), P010 (10-bit) |

On Apple silicon older than M3 and on Intel, `available_codecs()` honestly
omits `Av1` while HEVC keeps working.

Two behaviours are worth knowing about:

- **Hardware first, with a narrow fallback.** Sessions are created on macOS
  with `RequireHardwareAcceleratedVideoDecoder`, so the decode does not
  silently drop to VideoToolbox's software decoder. Apple's hardware HEVC
  block, however, refuses pictures below roughly 64x64 — and HEIF thumbnails
  are routinely 32x32 — so a failed hardware-pinned create is retried once
  without the requirement. Availability is still gated on the hardware probe,
  so this only widens the accepted picture *sizes*, never the codec list.
- **An explicit destination pixel format is requested.** Left to itself, a
  10-bit decode natively emits `'p420'`, which appears in no public CoreVideo
  header and whose plane contents do not match the documented `x420` samples
  for the same bitstream. All four documented 4:2:0 surfaces are offered
  instead, so the decoder stays on interpretable layouts while still choosing
  the one that matches the stream — which is what preserves the video/full
  range distinction.

### Why generated bindings here and a hand-written `sys.rs` for VAAPI

This backend uses the [`objc2`](https://github.com/madsmtm/objc2) framework
crates (`objc2-video-toolbox`, `-core-media`, `-core-video`,
`-core-foundation`). The VAAPI backend hand-writes its FFI, and the difference
is deliberate: libva must be **dlopen'd** so a machine without it degrades to
"no decoder" rather than failing to start, which rules out ordinary generated
bindings. The Apple frameworks are guaranteed present and link normally, so
the maintained bindings win — `CFRetained<T>` gives Create/Get-rule reference
counting as RAII instead of hand-paired `CFRetain`/`CFRelease`, and the
`#[link(kind = "framework")]` attributes live upstream. They are taken with
`default-features = false` and only the per-header features used, so no
Objective-C runtime, Metal, OpenGL or CoreAudio is compiled in; the only
transitive addition is `bitflags`.

## VAAPI backend (linux-gnu)

libva is **dlopen'd at runtime** (`libva.so.2` + `libva-drm.so.2` via
`libloading`) — nothing links against it, so a machine without libva, a
`/dev/dri/renderD*` node, or driver support simply reports "no decoder"
instead of failing to start (headless/CI safe). The probe answers from the
driver's real `vaQueryConfigProfiles`/`vaQueryConfigEntrypoints` lists.

Still-picture scope:

| Codec | Profiles | Output |
| --- | --- | --- |
| HEVC (HEIC) | Main, Main 10 — IRAP intra, 4:2:0/monochrome, tiles/WPP OK; scaling lists and range/SCC extensions are rejected with a clear error | NV12 (8-bit), P010 (10-bit) |
| AV1 (AVIF) | Profile 0 (Main) — one intra frame, any tiling, film grain OK; profiles 1/2 and large-scale tile rejected | NV12 (8-bit), P010 (10-bit) |

### GPU vendors — including NVIDIA

VAAPI covers **Intel** (media-driver / i965) and **AMD** (Mesa radeonsi)
natively. **NVIDIA GPUs are supported through the maintained
[`nvidia-vaapi-driver`](https://github.com/elFarto/nvidia-vaapi-driver)
translation layer over NVDEC** — install it (and set
`NVD_BACKEND`/`LIBVA_DRIVER_NAME` per its README if needed) and this backend
picks it up through the same dlopen path with no rawshift changes. This is
why rawshift has no separate NVDEC backend; see the permanent matrix and
justification in [`docs/SUPPORT.md`](../../docs/SUPPORT.md).

## Feature flags (verified)

| Feature | Meaning |
| --- | --- |
| `hw` | Portable: select the native backend for the compile target (VideoToolbox on macOS/iOS, VAAPI on linux-gnu; build-script warning + stub on targets with no hardware decode API). |
| `videotoolbox` | Pin VideoToolbox; `compile_error!` on non-Apple targets. |
| `vaapi` | Pin VAAPI; `compile_error!` off linux-gnu. |
| `mediacodec` | Pin MediaCodec; `compile_error!` off Android. |

## Testing

The device tests (`tests/videotoolbox_device.rs`, `tests/vaapi_device.rs`) run
real decodes and skip gracefully when the machine has no decoder for the
codec, so CI stays green without ever exercising hardware. That means CI does
**not** cover decode correctness: run `just test-hw` locally, which is the
documented pre-release gate (see "Pre-release hardware verification" in
`DEVELOPMENT.md`).
