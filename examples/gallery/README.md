# rawshift gallery

A cross-platform GUI that demonstrates rawshift's decode and encode paths end
to end, including hardware-accelerated HEIC/AVIF decode.

Pick images from the filesystem. Each one is decoded, re-encoded through every
selected output format with every parameter exposed, written to the system temp
directory, and decoded back. Results are shown side by side per source with a
numeric comparison against the original — or, where a stage failed, the error
text inside an empty placeholder.

```sh
just gallery                          # the window
just gallery-headless path/to/*.jpg   # the same matrix, no window
just gallery-check                    # fmt + clippy + tests
```

## Why this is not a workspace member

`examples/gallery` sits in the root `Cargo.toml`'s `exclude` list and carries
its own `[workspace]` table.

Its `gamut-cmm` dependency comes from git, and the gamut repository has `aom`
and `dav1d` submodules that Cargo fetches with it — a checkout of roughly
1.4 GB. As a workspace member that cost would land on *every* CI job, including
ones that named it in `--exclude`, because Cargo still reads every member
manifest to build the resolve graph. Standing outside the workspace, it is
built only by the `just gallery*` recipes and the dedicated `gallery` CI job.

It also keeps iced, wgpu, and winit out of `cargo test -p rawshift-image`:
Cargo has no optional dev-dependencies, so an `[[example]]` with
`required-features` would not have avoided that.

## The gamut-cmm carve-out

`AGENTS.md` forbids git dependencies on gamut because they prevent publishing
rawshift. That reason does not reach this crate — it is `publish = false` and
outside the workspace, so no published crate's dependency tree contains it.

`gamut-cmm` is the ICC colour-management module ([gamut#323]). It is not on
crates.io yet, and it is the only way to apply an ICC transform to pixels:
`gamut-icc` parses and serialises profiles but explicitly does not transform
them. Retire the carve-out for a crates.io version once gamut-cmm ships
([gamut#377]); the ~1.4 GB fetch is [gamut#378].

**Two gamut trees.** `gamut-cmm` depends on `gamut-core` / `gamut-color` /
`gamut-icc` by path inside the gamut workspace, so over git those resolve to
the git checkout — a different Cargo *source* from crates.io even at identical
version numbers. This crate therefore links two copies of `gamut-icc` whose
`IccProfile` types are not interchangeable. ICC data crosses that boundary as
**bytes** and is re-parsed; see `src/color.rs`.

[gamut#323]: https://github.com/visualcommons/gamut/issues/323
[gamut#377]: https://github.com/visualcommons/gamut/issues/377
[gamut#378]: https://github.com/visualcommons/gamut/issues/378

## Linux

No system development packages are needed. The crate selects iced's `x11` and
`wayland` features; `wayland` brings `winit/wayland-dlopen`, so libwayland is
loaded at runtime rather than linked. Those two features are not optional
extras — they are the only windowing backends on Linux, and building with
`default-features = false` and neither of them selected fails in `wayland-sys`,
whose build script requires `wayland-client` through pkg-config.

## Layout

| Module | Role |
| --- | --- |
| `pipeline.rs` | decode → encode → temp file → decode → compare. No `iced` types. |
| `color.rs` | container colour resolution and the display transform via gamut-cmm. |
| `settings.rs` | the encode-settings model, holding the library's own config structs. |
| `render.rs` | decoded image to texture, with preview downsampling. |
| `headless.rs` | the same matrix with no window. |
| `app.rs` | the window: state, messages, update, view. |

`pipeline`, `color`, and `settings` are free of `iced` types on purpose: the
headless runner and the unit tests drive exactly the code the window does, so
there is no second implementation to drift.

## What the numbers mean

- **bit-exact** — every sample matched. Asserted for the lossless
  configurations (PNG, lossless WebP/AVIF/JXL); a lossless configuration that
  does *not* come back bit-exact is reported as `LOSSLESS VIOLATED`, which is a
  defect rather than expected loss.
- **PSNR / max Δ** — for lossy configurations, computed on the
  full-resolution buffers (the on-screen previews are downsampled; the numbers
  are not).
- **no hardware decoder** — expected on a machine with no backend, and not
  counted as a failure. A decoder that exists but *rejects* the bitstream is
  counted: that is an interoperability gap, not an absent backend.

## Known interoperability gap

On Apple hardware, rawshift cannot currently decode its own AVIF output.
`gamut-avif` encodes identity-matrix 4:4:4, which is AV1 Profile 1, and
VideoToolbox's still-image path decodes Main / Profile 0 only. The AVIF column
therefore shows a decode error on macOS even though both halves work in
isolation. The gallery surfaces this rather than hiding it: [gamut#375].

[gamut#375]: https://github.com/visualcommons/gamut/issues/375

## Colour management

Two layers, addressed separately:

- **Container** — what the file declared. rawshift resolves this now:
  `RgbImage::color()` carries the CICP description, and the raw profile is in
  `ImageMetadata::icc_profile`. The panel under each source shows both.
- **Display** — getting the samples into the viewer's space. An
  ICC-authoritative source (one tagged `UNSPECIFIED`, meaning its profile has
  no faithful CICP expression) goes through `gamut-cmm`. Anything gamut-cmm
  cannot link — LUT-based profiles, since its phase P5 is unimplemented — falls
  back to sRGB with a visible warning rather than refusing to draw.

  A wide-gamut image that carried CICP code points rather than a profile has
  nothing for gamut-cmm to link from, so it is shown as sRGB with a warning.
  Closing that needs a `ColourPrimaries` → chromaticities accessor upstream
  ([gamut#376]); the matching library-side conversion is [rawshift#66].

[gamut#376]: https://github.com/visualcommons/gamut/issues/376
[rawshift#66]: https://github.com/visualcommons/rawshift/issues/66
