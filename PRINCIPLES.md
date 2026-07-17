# Library Design Principles

- Stateless: The library should assume nothing about the state to support portability and parallelization.
- Separation of IO and CPU: Writing good IO-heavy and CPU-heavy code can be tough in different ways so we separate it where possible to simplify benching.
- Reinvent the wheel only when necessary: We should aim to use existing mature libraries for functionality; but often there are libraries that are either: lacking low-level features, non-performant for specific use cases, or insufficiently mature. For image primitives, codecs, containers, and metadata, the mature library is [gamut](https://github.com/justin13888/gamut) — improve it upstream rather than reimplementing here (see the Upstream-First Policy in AGENTS.md). Accepted exceptions: GIF (`gif`), SVG (`resvg`), PPM (`zune-ppm`).

## Safety Boundaries

- `crates/rawshift-core`: `#![forbid(unsafe_code)]`.
- `crates/rawshift-image/src/formats`: Safe Rust is strictly required.
- `crates/rawshift-image/src/data`: Safe Rust is strictly required.
- `crates/rawshift-image/src/metadata`: Safe Rust is strictly required.
- `crates/rawshift-image/src/processing`: Unsafe Rust is acceptable as long as it is constrained to hot paths.
- `crates/rawshift-image/src/transforms`: Unsafe Rust is acceptable as long as it is constrained to hot paths.
- `crates/rawshift-hwdec`: Unsafe FFI is permitted — `#![deny(unsafe_op_in_unsafe_fn)]`, every public item is a safe wrapper, every unsafe block documents its invariants. No platform unsafe lives anywhere else.
- `**/**`: TBD

## Testing Strategy

- Integration tests (`tests/`): Ensure common workflows are functional.
- Unit tests: All major functions should be rigorously tested. Please place them in the same module file when possible.
