# rawshift-video

Video format support for [rawshift](https://github.com/visualcommons/rawshift).

> **Status: parked for v1, unpublished.** rawshift v1 ships image only. No
> video code ships today, this crate is marked `publish = false`, and it is
> **not** a dependency of the [`rawshift`](https://crates.io/crates/rawshift)
> facade — there is no `video` feature to enable. The crate remains in the
> workspace to hold the roadmap below and the workspace slot for post-v1 work.
>
> It is re-added to the publish set and to the facade when it has an
> implementation to publish. Until then the feature flags below gate no code and
> should be treated as a design sketch, not a supported surface.

## Roadmap

The formats below are prioritised by the cameras in rawshift's supported device
list:

| Format / Codec       | Container       | Status  | Notes                                          |
| -------------------- | --------------- | ------- | ---------------------------------------------- |
| XAVC HS (H.265/HEVC) | MP4             | Planned | Sony mirrorless video.                         |
| XAVC S (H.264/AVC)   | MP4             | Planned | Sony mirrorless video.                         |
| Apple ProRes         | QuickTime (MOV) | Planned | iPhone Pro and professional editing workflows. |
| HEVC (H.265)         | QuickTime (MOV) | Planned | Default iPhone video.                          |
| H.264 (AVC)          | QuickTime (MOV) | Planned | Legacy and compatibility video.                |

Initial work will focus on container parsing and metadata extraction, reusing
the in-repo ISOBMFF parser already used for Canon CR3 (both MP4 and QuickTime
are ISOBMFF-based). Codec-level decoding is a later milestone.

## Feature Flags

Video features mirror the `rawshift-image` tier structure but currently gate no
code or dependencies. They are a design sketch for post-v1 work and are not
reachable through the `rawshift` facade — see the status note above.

- **Bundles** — `video` (all formats), `full`.
- **Formats** — `xavc-hs`, `xavc-s`, `hevc`, `h264`, `prores`.
- **Directions** — `xavc-hs-decode`, `xavc-s-decode`, `hevc-decode`,
  `h264-decode`, `prores-decode` (decode-only for now).

## License

Licensed under [MPL-2.0](../../LICENSE).
