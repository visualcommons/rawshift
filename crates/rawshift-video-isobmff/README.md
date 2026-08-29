# rawshift-video-isobmff

MP4, M4V and QuickTime (MOV) support for
[rawshift](https://github.com/visualcommons/rawshift).

The box layer is [`mp4-atom`](https://crates.io/crates/mp4-atom) (MIT, pure
Rust, no build script). It performs no interpretation, so this crate owns the
parts that give a container meaning: the sample index reconstructed from the
`stbl` tables, the track model, and the demux and seek semantics.

## Why not an off-the-shelf demuxer

This crate first used `symphonia-format-isomp4`, which is audio-first and caps
the `hvcC` configuration record at 1 KB. Real HEVC records are 2–3 KB, so
**every HEVC MP4 and MOV failed to open** — that is XAVC HS and default iPhone
video, the two highest-priority formats on rawshift's roadmap. It also hides
the `tkhd` matrix (and so rotation), the `colr` box (and so all colour
signalling), the sync-sample table, and ProRes-shaped sample entries.

Owning the sample index on a box-level library costs a few hundred lines and
removes all five limitations at once. Matroska keeps its symphonia backend,
which has none of these problems.

## What it reads

`ftyp` brands, the `mvhd` timescale and times, per-track `tkhd` rotation,
`mdhd` timescale and language, the `stsd` sample entry (including `colr`
colour signalling and the `avcC`/`hvcC` record a decoder needs), the full
`stbl` sample tables, and the `elst` presentation shift — without which every
timestamp is offset by the encoder's reorder delay and disagrees with every
other tool.

Samples are read on demand, so opening a file costs only its headers however
large the media is.

## Not covered

Fragmented MP4 (`mvex`/`moof`) is rejected with a clear error rather than
presented as a track with no samples. Camera-origin files — rawshift's subject
— are not fragmented.

## License

Licensed under [MPL-2.0](../../LICENSE).
