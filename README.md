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
- Adobe DNG (up to v1.7, including what is necessary for Apple ProRAW)
- Standard TIFF

- Canon CR3
- Canon CR2

> Features and performance are being constantly improved. As most functionality are implemented from scratch to meet project goals, expect progressive improvements for format support over time.

> We aim to be liberal in what we accept (decode) and strict in what we give (encode).

| Format       | Decoding    | Encoding    | Notes                                                                    |
| ------------ | ----------- | ----------- | ------------------------------------------------------------------------ |
| Sony ARW     | Stabilizing | N/A         |                                                                          |
| Canon CR2    | Incomplete  | N/A         | No test fixtures.                                                        |
| Canon CR3    | Incomplete  | N/A         | Metadata only. CRX codec not implemented.                                |
| Canon CRW    | Incomplete  | N/A         | Detection only. No pixel decode.                                         |
| Adobe DNG    | Stabilizing | Stabilizing | Includes Apple ProRAW (DNG 1.7 + JXL).                                  |
| Nikon NEF    | Incomplete  | N/A         | No test fixtures.                                                        |
| Fujifilm RAF | Incomplete  | N/A         | No test fixtures.                                                        |
| JPEG         | Stable      | Stable      |                                                                          |
| PNG          | Stable      | Stable      |                                                                          |
| WebP         | Stable      | Stable      |                                                                          |
| GIF          | Stable      | Not planned |                                                                          |
| TIFF         | Stable      | Not planned |                                                                          |
| JXL          | Stable      | Functional  |                                                                          |
| AVIF         | Functional  | Functional  |                                                                          |
| HEIC         | Detection   | Not planned | H.265 patents.                                                           |
| SVG          | Functional  | Not planned |                                                                          |

Note on encoding support: For formats that we do not support encoding, you may still take the decoded pixel data and metadata, and encode it with your own encoding logic.

## Official supported device list

*A device is officially supported if we have thoroughly tested compatibility for it.*

| Device                 | Format(s)       | Notes |
| ---------------------- | --------------- | ----- |
| Sony A7RV (ILCE-7RM5)  | ARW             |       |
| Sony A7IV (ILCE-7M4)   | ARW             |       |
| Sony a6700 (ILCE-6700) | ARW             |       |
| iPhone 13 Pro (Max)    | DNG, HEIC, JPEG |       |
| iPhone 16 Pro (Max)    | DNG, HEIC, JXL  |       |

## MSRV

The minimum supported Rust version (MSRV) is **1.85** (edition 2024 requirement). This may be bumped as new language features stabilize.

## Development

It is important that development velocity is maintained regardless of project complexity. Unit tests for all contributions are expected, especially for platform-specific behaviours!

### Setup

Install [lefthook](https://github.com/evilmartians/lefthook) and activate the pre-commit and pre-push hooks:

```sh
# macOS / Homebrew
brew install lefthook

# Linux (Homebrew on Linux)
brew install lefthook

# via cargo
cargo install lefthook

# then install the hooks
lefthook install
```

The pre-commit hook runs `cargo fmt --check` and `cargo clippy`.
The pre-push hook runs the full test suite.

### Testing

```sh
cargo test --features=serde
```

### Library Design Principles

- Stateless: The library should assume nothing about the state to support portability and parallelization.
- Separation of IO and CPU: Writing good IO-heavy and CPU-heavy code can be tough in different ways so we separate it where possible to simplify benching.
- Reinvent the wheel only when necessary: We should aim to use existing mature libraries for functionality; but often there are libraries that are either: lacking low-level features, non-performant for specific use cases, or insufficiently mature.

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

## Sovereignty

It is my intention (as developer and maintainer) to ensure Rawshift remains open permissively to all.

## License

While many open-source implementations historically used LGPL or similar licenses, RawShift prefers a more permissive license (MPL-2.0; see [LICENSE](./LICENSE)). You are free to link to any software although we welcome contributions in any way.
