# rawshift

Clean-slate implementation for raw image decoding with full metadata support and conversion.

## Performance

- This library is heavy on both CPU and IO. Use appropriate SIMD primitives, data structures and memory allocators.

## Upstream-First Policy (gamut)

rawshift depends on [gamut](https://github.com/visualcommons/gamut) for image
primitives, color, metadata, container parsing, and codecs. gamut is consumed
as versioned crates.io dependencies in the workspace `Cargo.toml`; git
dependencies are not permitted because they prevent publishing rawshift.

- If a rawshift change needs any change in gamut (new API, bug fix, missing
  format capability), you MUST: (1) open an issue on `visualcommons/gamut`
  first, (2) wait for it to land on gamut `master`, (3) wait for the affected
  gamut crates to be published, (4) update the workspace dependency versions,
  and only then (5) make the rawshift change.
- Robustness/hardening doubts about gamut (parser bounds checks, byte
  completeness, fuzz coverage) are handled the same way, as `chore`-labeled
  gamut issues: they are correctness verifications, not API asks, and they
  gate the dependent rawshift migration.
- Never work around a gamut limitation inside rawshift: no vendoring or
  copy-pasting gamut code, no forks, no shims, and no interim alternative
  dependency for a format gamut is expected to cover. Blocked work stays
  blocked and the rawshift issue carries the `blocked-upstream` label with a
  link to the gamut issue.
- Permanent exceptions (stay on current deps; do not migrate, do not file
  upstream issues): GIF (`gif`), SVG (`resvg`), PPM (`zune-ppm`).
- Supported compilation targets and hardware decode APIs are fixed in
  `docs/SUPPORT.md` (with justifications for exclusions) — do not add or
  remove targets/APIs; they were decided once at v1.
- MSRV stays as low as upstream dependencies require; never raise it
  independently.
- Updating gamut dependencies is a deliberate, reviewed change: one commit
  that only updates the version requirements and lockfile, a full test +
  benchmark run, and a CHANGELOG.md note for any behavioral change.

### Video is outside gamut's charter

The policy above governs **image** work. It does not apply to video, because
gamut has declared video permanently out of scope. From gamut's README:

> **gamut is image-first.** Even where a format's codec (AV1, AV2, VVC, HEVC)
> is fundamentally a video codec, gamut implements only the intra-frame,
> still-image subset those formats use — no inter-frame prediction, no motion
> compensation, no video sequences. […] gamut will not grow video primitives.

`gamut-isobmff` says the same at the container level: *"Image sequences/tracks
and item protection are out of scope."* It parses the HEIF still-image item
model (`meta`/`iloc`/`iinf`/`iprp`), not the movie model (`moov`/`trak`/`stbl`),
so it cannot back MP4/MOV video however much the two formats share a box
grammar.

Consequently, for `rawshift-video` and the `rawshift-video-*` crates:

- Do **not** open gamut issues for video containers, video codecs, or
  multi-frame primitives. They will be declined as out of charter, and
  `blocked-upstream` is the wrong label for them.
- Third-party dependencies are permitted where a mature library exists —
  judged by the `PRINCIPLES.md` rule ("reinvent the wheel only when
  necessary"), not by the upstream-first rule. Current choices:
  `symphonia-format-isomp4` and `symphonia-format-mkv` for demuxing (MPL-2.0,
  matching rawshift's own license; pure Rust with no build script).
- Video **decode** stays in `rawshift-hwdec`. `docs/SUPPORT.md` fixes the
  reasons: software HEVC is ruled out by the patent posture, and FFmpeg/libav
  is ruled out on license and portability grounds (see "Excluded" there).
- Everything else in this file still applies to video: no git dependencies,
  the fixed target list, the MSRV rule, and a CHANGELOG note for behavioral
  change.

Image work is unaffected — gamut remains the upstream home for image
primitives, color, metadata, still-image container parsing, and codecs, and the
upstream-first sequence above is still mandatory there.

## Testing Methodology

Try to unit test the bulk majority of the code but functions that take in external inputs such as image/video file(s) should use test fixtures derived from external sources (which may require human sourcing as prerequisite). Also extend example binaries in `examples/` as necessary to show that each feature actually works.

## Library Design Principles

@PRINCIPLES.md
