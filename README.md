# rawshift

Raw image processing library focused on compatibility, correctness, and interoperability.

## Why rawshift?

Image processing is messy and no single library could do it all (e.g., accurate colour, complete metadata, consistent format support). Rawshift seeks to stand out in at least some of the following ways:

- Compatibility: A single library processes all forms of image formats including all popular compressed and RAW image formats used by both consumers and creative professionals.
- Correctness: While decoding implementations should remain flexible (e.g. to slightly non-conformant image), it should strictly conform to open standards and retain maximum metadata across formats.
- Interoperability: This library compiles and is optimized for several standard desktop and mobile platforms. This is possible because the majority of the library is written in pure Rust and non-Rust dependencies are encapsulated with best practices.

## Getting Started

> This library is still in active development. See `master` branch for latest improvements (noting potential instability). Alpha packages may be published to crates.io occasionally.

<!-- TODO: Add docs on the specific features, etc. on the docs.rs page -->

## Format Support

Here is the list of formats that are being worked on in order of priority:

- Sony ARW (all variations at least up to v5.0.1)
- Adobe DNG (up to v1.6, including what is necessary for Apple ProRAW)
- Standard TIFF

- Canon CR3
- Canon CR2

> Features and performance are being constantly improved. As most functionality are implemented from scratch to meet project goals, expect progressive improvements for format support over time.

> We aim to be liberal in what we accept (decode) and strict in what we give (encode).

| Format       | Decoding | Encoding    | Notes                                                                                                                                                     |
| ------------ | -------- | ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Sony ARW     | WIP      | N/A         |                                                                                                                                                           |
| Canon CR2    | WIP      | N/A         |                                                                                                                                                           |
| Canon CR3    | WIP      | N/A         |                                                                                                                                                           |
| Canon CRW    | WIP      | N/A         |                                                                                                                                                           |
| Adobe DNG    | WIP      | Not planned | We aim to support all standards-compliant DNG for decoding (includes Apple ProRAW) but don't plan to support encoding (as linear DNGs are not so common). |
| Nikon NEF    | WIP      | N/A         |                                                                                                                                                           |
| Fujifilm RAF | WIP      | N/A         |                                                                                                                                                           |
| TIFF         | WIP      | WIP         |                                                                                                                                                           |
| AVIF         | WIP      | WIP         |                                                                                                                                                           |
| HEIC         | TBD      | Not planned | HEIC uses patented H.265 encoding. We may support decoding eventually but not encoding.                                                                   |
| JPEG         | WIP      | WIP         |                                                                                                                                                           |
| JXL          | WIP      | WIP         |                                                                                                                                                           |
| PNG          | WIP      | WIP         |                                                                                                                                                           |
| SVG          | WIP      | Not planned |                                                                                                                                                           |
| WEBP         | WIP      | WIP         |                                                                                                                                                           |
| GIF          | WIP      | Not planned |                                                                                                                                                           |

Note on encoding support: For formats that we do not support encoding, you may still take the decoded pixel data and metadata, and encode it with your own encoding logic.


## Official supported device list

*A device is officially supported if we have thoroughly tested compatibility for it.*

| Device                 | Format(s)       | Notes |
| ---------------------- | --------------- | ----- |
| Sony A7RV (ILCE-7RM5)  | ARW             |       |
| Sony A7IV (ILCE-7M4)   | ARW             |       |
| Sony a6700 (ILCE-6700) | ARW             |       |
| iPhone 13 Pro (Max)    | DNG, HEIC, JPEG |
| iPhone 16 Pro (Max)    | DNG, HEIC, JXL  |

## MSRV

As of August 2025, the minimum supported Rust version (MSRV) is 1.89.0. This may be quite high but the reason is to ensure modern Rust features are available early in development. This may not be bumped up in for a good while.

## Development

It is important that development velocity is maintained regardless of project complexity. Unit tests for all contributions are expected, especially for platform-specific behaviours!

### Testing

```sh
cargo test --features=serde
```

### Library Design Principles

- Stateless: The library should assume nothing about the state to support portability and parallelization.
- Separation of IO and CPU: Writing good IO-heavy and CPU-heavy code can be tough in different ways so we separate it where possible to simplify benching.

#### Safety Boundaries

- `src/tiff`: Safe Rust is strictly required.
- `src/formats`: Safe Rust is strictly required.
- `src/data`: Safe Rust is strictly required.
- `src/core`: Safe Rust is strictly required.
- `src/processing`: Unsafe Rust is acceptable as long as it is constrained to hot paths.
- `src/transforms`: Unsafe Rust is acceptable as long as it is constrained to hot paths.
- `**/**`: TBD

#### Testing Strategy

- Integration tests (`tests/`): Ensure common workflows are functional.
- Unit tests: All major functions should be rigorously tested. Please place them in the same module file when possible.


## License

While many open-source implementations historically used LGPL or similar licenses, RawShift prefers a more permissive license (MPL-2.0; see [LICENSE](./LICENSE)). You are free to link to any software although we welcome contributions in any way.
