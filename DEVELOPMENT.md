# Development

Contributor and maintainer guide for `rawshift`. For library design rules see
[PRINCIPLES.md](PRINCIPLES.md); for test data see [TEST_FIXTURES.md](TEST_FIXTURES.md).

## Building & testing

The toolchain is pinned by [`rust-toolchain.toml`](rust-toolchain.toml) (1.90.0).

```sh
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Some features link system libraries and need fixtures; see CI (`.github/workflows/ci.yml`)
and [TEST_FIXTURES.md](TEST_FIXTURES.md) for the full matrix.

## Releasing

Releases are automated with [release-plz](https://release-plz.dev). It reads
[Conventional Commits](https://www.conventionalcommits.org) on `master`,
proposes the next version and changelog in a **Release PR**, and on merge
publishes to crates.io, tags the commit, and cuts a GitHub release.

### Model

- **One version, in lockstep.** All four crates (`rawshift`, `rawshift-core`,
  `rawshift-image`, `rawshift-video`) share a single version via
  `version.workspace = true` and are released together.
- **One changelog.** A single root [`CHANGELOG.md`](CHANGELOG.md) aggregates
  every crate's changes (the `rawshift` facade owns it via `changelog_include`
  in [`release-plz.toml`](release-plz.toml)). It already contains the `0.1.0`
  entry; release-plz inserts each new version above it. Commit grouping is
  release-plz's default — customize it later with a `[changelog]` section in
  `release-plz.toml` (e.g. to skip merge commits) if desired.
- **One tag + one release per version:** `vX.Y.Z` (e.g. `v0.1.0`), owned by the
  facade.
- **No publish tokens in steady state.** Publishing uses crates.io
  [Trusted Publishing](https://crates.io/docs/trusted-publishing) (OIDC), so no
  `CARGO_REGISTRY_TOKEN` secret is stored. The workflow only needs `id-token: write`.

### Conventional commits → version bump

The project is pre-1.0, so breaking changes bump the **minor** version (not major):

| Commit prefix                         | Effect on `0.x`        |
| ------------------------------------- | ---------------------- |
| `fix:`                                | patch (`0.1.0→0.1.1`)  |
| `feat:`                               | minor (`0.1.0→0.2.0`)  |
| `feat!:` / `fix!:` / `BREAKING CHANGE:` | minor (pre-1.0)      |
| `chore:` `docs:` `refactor:` `test:` `ci:` `style:` `perf:` | no release (still listed in changelog where relevant) |

A bump to **any** crate moves the shared version, so all crates re-release at
the new version.

### Steady-state flow

1. Merge conventional-commit PRs into `master` as usual.
2. The `release-plz-pr` job opens/updates a **Release PR** ("chore: release vX.Y.Z")
   that bumps `[workspace.package] version` and updates `CHANGELOG.md`.
3. Review and merge the Release PR.
4. The `release-plz-release` job publishes the crates (core → image/video →
   rawshift), pushes the `vX.Y.Z` tag, and creates the GitHub release.

> **Note:** the Release PR is opened with the default `GITHUB_TOKEN`, so CI
> (`ci.yml`) does **not** run on it. The change is already CI-verified on
> `master` before the PR is cut. To run CI on Release PRs, switch the workflow
> to a GitHub App token / PAT via the action's `token:` input.

## One-time setup (bootstrap)

crates.io trusted publishing **cannot create a brand-new crate** — a Trusted
Publisher can only be configured on a crate that already exists. So the very
first `0.1.0` publish is done manually, once. Everything after is automated.

Do this in order:

1. **Enable PR automation.** Repo → Settings → Actions → General → *Workflow
   permissions* → check **"Allow GitHub Actions to create and approve pull
   requests."**

2. **Create a temporary crates.io token** with the `publish-new` and
   `publish-update` scopes (used only for steps 3–4, then deleted):
   ```sh
   cargo login   # paste the token when prompted
   ```

3. **Publish `0.1.0` manually, in dependency order** (each waits for crates.io
   to index the previous one):
   ```sh
   cargo publish -p rawshift-core
   cargo publish -p rawshift-image
   cargo publish -p rawshift-video
   cargo publish -p rawshift
   ```
   Tip: `cargo publish --dry-run -p <crate>` first to catch packaging issues.

4. **Tag the baseline** so release-plz has a starting point for changelog diffs
   (subsequent tags are created automatically):
   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```

5. **Configure a Trusted Publisher for each of the four crates** on crates.io
   (crate → Settings → Trusted Publishing → Add GitHub):
   - Repository owner: `justin13888`
   - Repository name: `rawshift`
   - Workflow filename: `release-plz.yml`
   - Environment: *(leave blank)*

6. **Delete the temporary token** (crates.io → Account Settings → API Tokens,
   and `cargo logout`). Steady-state publishing no longer needs it.

After this, pushing releasable commits to `master` drives the whole pipeline.

## Local / manual operations

- **Preview the next release** without touching GitHub (run on a scratch branch):
  ```sh
  cargo install release-plz   # once
  release-plz update          # writes version + CHANGELOG.md changes locally
  git diff                    # inspect, then `git checkout .` to discard
  ```
- **Emergency manual publish** (e.g. trusted publishing unavailable): repeat the
  bootstrap step 3 publish order with a valid token, then tag manually.

## Troubleshooting

- **Release PR not appearing:** confirm step 1 (PR automation) is enabled and the
  commits since the last tag include a releasable type (`feat`/`fix`/breaking).
- **`error: crate ... does not exist` on first OIDC publish:** the crate was never
  bootstrapped — finish the manual `0.1.0` publish (bootstrap step 3) first.
- **Publish fails mid-way:** crates.io indexing can lag; re-run the
  `release-plz-release` job — release-plz skips already-published crates and
  resumes with the rest.
- **A crate didn't bump with the others:** verify all crates use
  `version.workspace = true`; if a crate still lags, add `version_group = "rawshift"`
  to every `[[package]]` block in [`release-plz.toml`](release-plz.toml).
- **Duplicated `# Changelog` header after a release:** the changelog must always
  keep at least one `## [x.y.z]` section. If reset to a header only (no version),
  release-plz prepends a fresh header on the next release. Keep the `0.1.0`
  section, or start the file empty and let release-plz create it.
