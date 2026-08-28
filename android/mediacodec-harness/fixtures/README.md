# Fixture provenance

The four `.rshw` files are 64×64, single-frame, lossless test vectors generated
with FFmpeg 8.1.2 from its deterministic `testsrc2` source. They contain the
container-style codec configuration, coded item payload, and expected dense
decoded pixels. The expected layouts are I420 for 8-bit and MSB-aligned,
interleaved P010 for 10-bit.

Regeneration on Linux or macOS requires FFmpeg with `libx265` and `libaom-av1`:

```sh
tmp=$(mktemp -d)
ffmpeg -f lavfi -i 'testsrc2=size=64x64:rate=1' -frames:v 1 -pix_fmt yuv420p \
  -c:v libx265 -preset ultrafast \
  -x265-params 'lossless=1:keyint=1:min-keyint=1:repeat-headers=1:annexb=1' \
  -f hevc "$tmp/hevc8.hevc"
ffmpeg -f lavfi -i 'testsrc2=size=64x64:rate=1' -frames:v 1 -pix_fmt yuv420p10le \
  -c:v libx265 -preset ultrafast \
  -x265-params 'lossless=1:keyint=1:min-keyint=1:repeat-headers=1:annexb=1' \
  -f hevc "$tmp/hevc10.hevc"
ffmpeg -f lavfi -i 'testsrc2=size=64x64:rate=1' -frames:v 1 -pix_fmt yuv420p \
  -c:v libaom-av1 -cpu-used 8 -still-picture 1 -crf 0 -b:v 0 -f obu "$tmp/av1-8.obu"
ffmpeg -f lavfi -i 'testsrc2=size=64x64:rate=1' -frames:v 1 -pix_fmt yuv420p10le \
  -c:v libaom-av1 -cpu-used 8 -still-picture 1 -crf 0 -b:v 0 -f obu "$tmp/av1-10.obu"
ffmpeg -f lavfi -i 'testsrc2=size=64x64:rate=1' -frames:v 1 -pix_fmt yuv420p \
  -f rawvideo "$tmp/yuv8.raw"
ffmpeg -f lavfi -i 'testsrc2=size=64x64:rate=1' -frames:v 1 -pix_fmt yuv420p10le \
  -f rawvideo "$tmp/yuv10.raw"
rustc --edition 2024 package.rs -o "$tmp/package"
"$tmp/package" hevc 8 "$tmp/hevc8.hevc" "$tmp/yuv8.raw" data/hevc-8.rshw
"$tmp/package" hevc 10 "$tmp/hevc10.hevc" "$tmp/yuv10.raw" data/hevc-10.rshw
"$tmp/package" av1 8 "$tmp/av1-8.obu" "$tmp/yuv8.raw" data/av1-8.rshw
"$tmp/package" av1 10 "$tmp/av1-10.obu" "$tmp/yuv10.raw" data/av1-10.rshw
```

The host unit tests parse all four packages and exercise codec preparation plus
stride/crop normalization. The physical-device harness uses the same bytes and
compares every decoded output byte with the embedded expected frame.
