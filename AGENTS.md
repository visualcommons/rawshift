# rawshift

Clean-slate implementation for raw image decoding with full metadata support and conversion.

## MANDATORY: Use td for Task Management

You must run td usage --new-session at conversation start (or after /clear) to see current work.
Use td usage -q for subsequent reads

## Testing Methodology

Try to unit test the bulk majority of the code but functions that take in external inputs such as image/video file(s) should use test fixtures derived from external sources (which may require human sourcing as prerequisite). Also extend example binaries in `examples/` as necessary to show that each feature actually works.
