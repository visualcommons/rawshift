# rawshift-video-hwdec

H.264 and HEVC video decoders for
[rawshift](https://github.com/visualcommons/rawshift), backed by the platform
hardware decoders in `rawshift-hwdec`.

## Why one crate for two codecs

The image side has a leaf crate per format because each wraps a different
codec library. Here both codecs go through the same seam and the same backend,
and the two implementations differ only in which configuration record they
carry — two crates would be near-identical copies. The `h264` and `hevc`
features give a build the same narrowing a split would, without the
duplication.

## Software decode

There is none, deliberately. `docs/SUPPORT.md` rules out shipping software
H.264 or HEVC: both are patent-encumbered independently of any
implementation's own license. Hardware decoders carry the device OEM's
licence, so that is the only path rawshift offers. Where no backend exists the
decoders report `VideoError::HwDecoderUnavailable`, and container parsing,
track enumeration and metadata keep working regardless.

## License

Licensed under [MPL-2.0](../../LICENSE).
