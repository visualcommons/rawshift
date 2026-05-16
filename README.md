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

| Format       | Decoding                                                                                 | Encoding                                                                                         | Notes                                     |
| ------------ | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | ----------------------------------------- |
| Sony ARW     | Custom LJPEG (Stabilizing)                                                               | N/A                                                                                              |                                           |
| Canon CR2    | Custom LJPEG (Incomplete)                                                                | N/A                                                                                              | No test fixtures.                         |
| Canon CR3    | Custom ISOBMFF parser (Incomplete)                                                       | N/A                                                                                              | Metadata only. CRX codec not implemented. |
| Canon CRW    | Custom CIFF parser (Incomplete)                                                          | N/A                                                                                              | Detection only. No pixel decode.          |
| Adobe DNG    | Custom TIFF + jxl-oxide (Stabilizing)                                                    | Custom TIFF writer (Stabilizing)                                                                 | Includes Apple ProRAW (DNG 1.7 + JXL).    |
| Nikon NEF    | Custom TIFF parser (Incomplete)                                                          | N/A                                                                                              | No test fixtures.                         |
| Fujifilm RAF | Custom RAF parser (Incomplete)                                                           | N/A                                                                                              | No test fixtures.                         |
| JPEG         | [zune-jpeg](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg) (Stable) | [jpeg-encoder](https://github.com/vstroebel/jpeg-encoder) (Stable)                               |                                           |
| PNG          | [zune-png](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-png) (Stable)   | [zune-png](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-png) (Stable)           |                                           |
| WebP         | [libwebp-sys](https://github.com/noxf/libwebp-sys) (Stable)                              | [libwebp-sys](https://github.com/noxf/libwebp-sys) (Stable)                                      | C FFI bindings to libwebp.                |
| GIF          | [gif](https://github.com/image-rs/image-gif) (Stable)                                    | Not planned                                                                                      |                                           |
| TIFF         | [tiff](https://github.com/image-rs/image-tiff) (Stable)                                  | Not planned                                                                                      |                                           |
| JXL          | [jxl-oxide](https://github.com/tirr-c/jxl-oxide) (Stable)                                | [zune-jpegxl](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpegxl) (Functional) |                                           |
| AVIF         | [image/avif-native](https://github.com/image-rs/image) (Functional)                      | [ravif](https://github.com/kornelski/cavif-rs/tree/main/ravif) (Functional)                      |                                           |
| HEIC         | Detection only                                                                           | Not planned                                                                                      | H.265 patents.                            |
| SVG          | [resvg/tiny-skia](https://github.com/linebender/resvg) (Functional)                      | Not planned                                                                                      |                                           |

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

## Sovereignty

It is my intention (as developer and maintainer) to ensure Rawshift remains open permissively to all.

## License

While many open-source implementations historically used LGPL or similar licenses, RawShift prefers a more permissive license (MPL-2.0; see [LICENSE](./LICENSE)). You are free to link to any software although we welcome contributions in any way.
