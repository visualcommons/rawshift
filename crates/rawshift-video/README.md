# rawshift-video

Video support for [rawshift](https://github.com/visualcommons/rawshift):
containers, tracks, timelines, metadata, and hardware-backed frame decode.

The video counterpart to `rawshift-image` — a feature facade over the
container and decoder crates, plus the entry points that tie them together.

```rust,no_run
use rawshift_video::prelude::*;

let mut video = VideoFile::open(std::fs::File::open("clip.mp4")?)?;
println!("{} {:?}", video.container(), video.metadata().container.duration);

for track in video.video_tracks() {
    println!("{} {}x{} rotation {}", track.codec, track.dimensions.width,
             track.dimensions.height, track.rotation);
}

if let Some(frame) = video.thumbnail()? {
    println!("{}x{}", frame.width(), frame.height());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What works without a hardware decoder

Container parsing, track enumeration, timelines, packet access and metadata
work on **every** target in `docs/SUPPORT.md`, including `wasm32` and `musl`
where no hardware decode API can exist. Only frame decode needs a backend;
without one it reports `VideoError::HwDecoderUnavailable`, and
`hw_decode_available()` lets a caller check before committing.

## Support

| Container | Status | Backing |
| --- | --- | --- |
| MP4 / M4V | ✅ | `mp4-atom` boxes, rawshift sample index |
| QuickTime (MOV) | ✅ | the same |
| Matroska / WebM | ✅ | `symphonia-format-mkv` |
| AVI | named, not implemented | no viable Rust crate |
| MXF | named, not implemented | the only `mxf` crate dates from 2017 |

| Codec | Decode | Notes |
| --- | --- | --- |
| HEVC / H.265 | ✅ random access points | via `rawshift-hwdec`; see below |
| H.264 / AVC | seam ready, no backend path yet | the API is final; the VAAPI picture-parameter path is a follow-up |
| ProRes | named, not implemented | |
| AV1, VP9 | named, not implemented | AV1 stills decode through `rawshift-image` |

**Decode scope.** Every random access point decodes, which covers keyframe
extraction, poster frames, thumbnails and scrubbing, and decodes All-Intra
camera modes (Sony XAVC S-I, XAVC HS All-I) completely. Inter-frame decoding
is a purely internal change behind the same public API — it adds a decoded
picture buffer and reference handling — so it needs no second breaking
release.

**No software decode**, deliberately. `docs/SUPPORT.md` rules out shipping
software H.264 or HEVC: both are patent-encumbered independently of any
implementation's own license. Hardware decoders carry the device OEM's
licence.

## Feature Flags

- **Bundles** — `video` (every container and codec), `full` (adds `hw`).
- **Containers** — `mp4`, `mkv`.
- **Codecs** — `h264`, `hevc`.
- **Directions** — `h264-decode`, `hevc-decode`. Encode is a finished trait
  seam with no implementations, so no `-encode` flag gates anything yet.
- **Backends** — `hw` (portable), `hw-vaapi` / `hw-videotoolbox` /
  `hw-mediacodec` (pinned; these fail to compile off-target by design).
- **Device aliases** — `xavc-hs` (HEVC in MP4), `xavc-s` (H.264 in MP4).

A container feature without a codec feature is a valid metadata-and-timeline
build — the same shape as `heic`/`avif` without `hw` on the image side.

## License

Licensed under [MPL-2.0](../../LICENSE).
