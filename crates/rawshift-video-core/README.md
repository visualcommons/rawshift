# rawshift-video-core

Shared vocabulary for [rawshift](https://github.com/visualcommons/rawshift)'s
video crates — the video counterpart to `rawshift-image-core`.

This crate holds the types every `rawshift-video-*` crate agrees on and no
implementation of its own: errors, format and codec identity, the track /
packet / frame model, container-level metadata, and the `Demuxer` /
`VideoDecoder` / `VideoEncoder` contracts.

It exists because the video crates cannot reuse `rawshift-image-core`: the
workspace enforces (in CI) that no `rawshift-image*` crate appears in
`rawshift-video`'s dependency tree, so the two media share only
`rawshift-core`.

`#![forbid(unsafe_code)]`. No I/O, no platform code, no container or codec
dependencies.

## License

Licensed under [MPL-2.0](../../LICENSE).
