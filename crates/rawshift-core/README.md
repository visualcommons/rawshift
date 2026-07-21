# rawshift-core

Shared core types for [rawshift](https://github.com/visualcommons/rawshift) —
geometry, pixel samples, and the format-agnostic metadata model.

This crate holds pure, stateless data structures with **no decoding logic**. It
is depended on by both `rawshift-image` and `rawshift-video` so they share one
vocabulary of types without either pulling in the other. Most consumers should
depend on the [`rawshift`](https://crates.io/crates/rawshift) facade rather than
this crate directly.

## What's here

- **Geometry** — `Size`, `Point`, `Rect`.
- **Pixel samples** — `Sample`, `Rgb`/`Rgba` (8-bit, 16-bit, and f32 variants),
  and the `FromF32` conversion trait.
- **Image containers** — raw/RGB image types and CFA descriptors
  (`XTransPattern`).
- **Color** — `BitDepth`, `ColorSpace`.
- **Codec descriptors** — `CodecId`, `CodecInfo`, `CodecDirection`,
  `MetadataEmbedOptions`.
- **Metadata model** — `ImageMetadata`, `MetadataEntry`, `MetadataKey`,
  `MetadataValue`, `MetadataNamespace`, and the `MetadataExtractor` trait.

## Feature Flags

- `serde` — derive `Serialize`/`Deserialize` for the metadata and option types.

## License

Licensed under [MPL-2.0](../../LICENSE).
