# MediaCodec physical-device harness

This Gradle project is the merge gate for rawshift's Android hardware decoder.
It builds the Rust JNI library for `arm64-v8a`, installs an instrumentation
test, rejects emulators, and decodes checked-in lossless HEVC and AV1 fixtures.

The test verifies all decoded bytes, not merely that MediaCodec returned an
output buffer. It decodes every required fixture twice through one decoder:
the first duration is reported as `cold_us`, and the second (exact-format
session reuse) as `warm_us`. These are diagnostic numbers, not thresholds.

## Toolchain

- JDK 17
- Android SDK Platform 36 and Build Tools 36.0.0
- Android NDK `29.0.14206865`
- Rust 1.92.0 with `aarch64-linux-android`
- Gradle 9.4.1 (checked wrapper) and Android Gradle Plugin 9.2.1

Install the Android pieces with:

```sh
sdkmanager "platforms;android-36" "build-tools;36.0.0" "ndk;29.0.14206865"
rustup target add aarch64-linux-android
```

Set `ANDROID_HOME` and set `ANDROID_NDK_HOME` to the NDK directory. Then
connect exactly one unlocked ARM64 physical device with USB debugging enabled:

```sh
adb devices -l
cd android/mediacodec-harness
./gradlew connectedDebugAndroidTest
```

The full acceptance matrix is two physical-device runs:

| Run | Device requirements | Required result |
| --- | --- | --- |
| Compatibility floor | API 29, ARM64, hardware HEVC and AV1 decoders | byte-exact 8-bit HEVC + AV1, cold and warm |
| High bit depth | API 33+, ARM64, hardware HEVC and AV1 decoders | the same 8-bit checks plus byte-exact P010 for at least one codec |

Attach both `rawshift-hwdec` report blocks from logcat/test output to the pull
request before changing it from draft to ready. An emulator is intentionally
not accepted: its advertised codec inventory and acceleration path depend on
host passthrough, so it cannot establish the physical hardware contract.

## Why this is the Android boundary

`AMediaCodec` is the decoder. `AImageReader` supplies its CPU-readable output
surface. A small JNI classification step calls `MediaCodecList` because API 29
is the first public Android API that definitively exposes
`MediaCodecInfo.isHardwareAccelerated()` and `isAlias()`. JNI therefore does
not introduce a second decode backend; it identifies the exact named hardware
components that the NDK `AMediaCodec_createCodecByName` path opens.

This is the only Android decode API rawshift targets. Media3/ExoPlayer are
playback orchestration layers over MediaCodec, while `ImageDecoder` accepts
complete supported image sources rather than the HEIF/AVIF item codestream
seam rawshift must implement. The relevant platform contracts are:

- <https://developer.android.com/reference/android/media/MediaCodecList>
- <https://developer.android.com/media/optimize/performance/codec>
- <https://developer.android.com/ndk/reference/group/media>
- <https://developer.android.com/reference/android/media/ImageReader>
- <https://developer.android.com/reference/android/media/MediaCodecInfo.CodecCapabilities#COLOR_FormatYUVP010>

P010 is deliberately API 33+ in rawshift: although `ImageFormat.YCBCR_P010`
appeared earlier, `MediaCodecInfo.CodecCapabilities.COLOR_FormatYUVP010` is a
public codec capability only from API 33. The backend never silently
downconverts 10-bit input.
