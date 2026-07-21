# rawshift-hwdec

Hardware still-frame codestream decode for [rawshift](https://github.com/visualcommons/rawshift):
HEVC (HEIC) and AV1 (AVIF) intra pictures through the platform decode APIs
fixed permanently in [`docs/SUPPORT.md`](../../docs/SUPPORT.md) —
VideoToolbox (macOS/iOS), VAAPI (linux-gnu, dlopen'd at runtime), and NDK
MediaCodec (Android).

This is the **only** crate in the workspace where platform FFI may live
(`#![deny(unsafe_op_in_unsafe_fn)]`, safe public items, documented invariants
on every unsafe block). The **VAAPI backend is implemented**; VideoToolbox
and MediaCodec land as separate issues. On builds/targets with no backend the
crate compiles a no-backend stub: `decoder()` returns `None`, `backend()`
returns `None`, `available_codecs()` is empty, and dependants surface
`HwDecoderUnavailable`.

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
| `hw` | Portable: select the native backend for the compile target (VAAPI on linux-gnu; build-script warning + stub on targets with no hardware decode API). |
| `videotoolbox` | Pin VideoToolbox; `compile_error!` on non-Apple targets. |
| `vaapi` | Pin VAAPI; `compile_error!` off linux-gnu. |
| `mediacodec` | Pin MediaCodec; `compile_error!` off Android. |
