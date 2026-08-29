# rawshift-video-symphonia

Internal bridge between [symphonia](https://github.com/pdeljanov/Symphonia) and
[rawshift](https://github.com/visualcommons/rawshift)'s video model.

Not a container crate and not a public API. It exists so
`rawshift-video-isobmff` and `rawshift-video-matroska` share one translation of
symphonia's tracks, packets, timestamps and seek vocabulary instead of two
copies that drift.

The translation is one-directional and total: no symphonia type leaves this
crate, which is what lets the container crates keep that guarantee too.

## License

Licensed under [MPL-2.0](../../LICENSE).
