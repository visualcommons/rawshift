# rawshift-video-isobmff

MP4, M4V and QuickTime (MOV) support for
[rawshift](https://github.com/visualcommons/rawshift).

Demuxing is backed by
[`symphonia-format-isomp4`](https://crates.io/crates/symphonia-format-isomp4)
(MPL-2.0, pure Rust, no build script), which parses the movie box tree, the
sample tables, fragments, and the `avc1`/`hvc1`/`hev1`/`av01` sample entries
rawshift needs.

Two things symphonia does not surface, which this crate reads itself from a
focused header walk:

- the `tkhd` display matrix, and so track rotation — without which most phone
  video presents sideways;
- the `ftyp` brands and `mvhd` timescale and times.

Sync samples are a third: symphonia exposes no keyframe flag, so random access
points are read out of the bitstream by `rawshift-video-core`, which is the
authoritative source anyway.

No symphonia type appears in this crate's public API.

## License

Licensed under [MPL-2.0](../../LICENSE).
