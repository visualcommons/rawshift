# rawshift

Clean-slate implementation for raw image decoding with full metadata support and conversion.

## Performance

- This library is heavy on both CPU and IO. Use appropriate SIMD primitives, data structures and memory allocators.
 

## Testing Methodology

Try to unit test the bulk majority of the code but functions that take in external inputs such as image/video file(s) should use test fixtures derived from external sources (which may require human sourcing as prerequisite). Also extend example binaries in `examples/` as necessary to show that each feature actually works.

## Library Design Principles

@PRINCIPLES.md
