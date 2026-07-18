# rawshift-hwdec

Hardware still-frame codestream decode for [rawshift](https://github.com/justin13888/rawshift):
HEVC (HEIC) and AV1 (AVIF) intra pictures through the platform decode APIs
fixed permanently in [`docs/SUPPORT.md`](../../docs/SUPPORT.md) —
VideoToolbox (macOS/iOS), VAAPI (linux-gnu, dlopen'd at runtime), and NDK
MediaCodec (Android).

This is the **only** crate in the workspace where platform FFI may live
(`#![deny(unsafe_op_in_unsafe_fn)]`, safe public items, documented invariants
on every unsafe block). The platform backends land as separate issues; until
one does, the crate compiles a no-backend stub: `decoder()` returns `None`,
`backend()` returns `None`, `available_codecs()` is empty, and dependants
surface `HwDecoderUnavailable`.

## Feature flags (verified)

| Feature | Meaning |
| --- | --- |
| `hw` | Portable: select the native backend for the compile target (build-script warning + stub on targets with no hardware decode API). |
| `videotoolbox` | Pin VideoToolbox; `compile_error!` on non-Apple targets. |
| `vaapi` | Pin VAAPI; `compile_error!` off linux-gnu. |
| `mediacodec` | Pin MediaCodec; `compile_error!` off Android. |
