# rawshift

Image processing library focused on compatibility, correctness, and interoperability.

## Why rawshift?

Image processing is messy and no single library could do it all. Rawshift seeks to stand out in at least one of the following ways:

- Compatibility: A single library processes all forms of image formats including all popular compressed and RAW image formats used by both consumers and creative professionals.
- Correctness: While decoding implementations should remain flexible (e.g. to slightly non-conformant image), it should strictly conform to open standards and retain maximum metadata across formats.
- Interoperability: This library compiles and is optimized for several standard desktop and mobile platforms. This is possible because the majority of the library is written in pure Rust and non-Rust dependencies are encapsulated with best practices.

## Getting Started

> This library is still in active development. See `master` branch for latest improvements (noting potential instability). Alpha packages may be published to crates.io occasionally.

<!-- TODO: Add docs on the specific features, etc. on the docs.rs page -->

## Developing

It is important that development velocity is maintained regardless of project complexity. Unit tests for all contributions are expected, especially for platform-specific behaviours!
