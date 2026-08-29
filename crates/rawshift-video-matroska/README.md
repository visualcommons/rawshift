# rawshift-video-matroska

Matroska and WebM support for
[rawshift](https://github.com/visualcommons/rawshift).

Backed by [`symphonia-format-mkv`](https://crates.io/crates/symphonia-format-mkv)
(MPL-2.0, pure Rust, no build script), which maps `V_MPEG4/ISO/AVC` and
`V_MPEGH/ISO/HEVC` tracks and surfaces their `CodecPrivate` — the same `avcC`
and `hvcC` records MP4 carries — as codec extra data.

As with the ISOBMFF crate, random access points are read from the bitstream
rather than from a container flag, and no symphonia type appears in the public
API.

## License

Licensed under [MPL-2.0](../../LICENSE).
