# Library Design Principles

- Stateless: The library should assume nothing about the state to support portability and parallelization.
- Separation of IO and CPU: Writing good IO-heavy and CPU-heavy code can be tough in different ways so we separate it where possible to simplify benching.
- Reinvent the wheel only when necessary: We should aim to use existing mature libraries for functionality; but often there are libraries that are either: lacking low-level features, non-performant for specific use cases, or insufficiently mature.

## Safety Boundaries

- `src/tiff`: Safe Rust is strictly required.
- `src/formats`: Safe Rust is strictly required.
- `src/data`: Safe Rust is strictly required.
- `src/core`: Safe Rust is strictly required.
- `src/processing`: Unsafe Rust is acceptable as long as it is constrained to hot paths.
- `src/transforms`: Unsafe Rust is acceptable as long as it is constrained to hot paths.
- `**/**`: TBD

## Testing Strategy

- Integration tests (`tests/`): Ensure common workflows are functional.
- Unit tests: All major functions should be rigorously tested. Please place them in the same module file when possible.
