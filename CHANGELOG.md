# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/justin13888/rawshift/releases/tag/v0.1.0) - 2026-05-29

Initial release.

### Added

- Cargo workspace layout: the `rawshift` facade crate plus `rawshift-core`
  (shared geometry, pixel, and metadata types), `rawshift-image` (still-image
  decoding, RAW processing, and encoding), and a `rawshift-video` placeholder.
- Still-image decoding with explicit, per-implementation decoder backend
  selection and configuration.
- PPM / Netpbm support via `zune-ppm`.
- HEIC / HEIF decode support via libheif.
- Layered RAW feature flags organized into a decode / encode / tier hierarchy.

### Build

- Workspace MSRV set to 1.90.0.
- Workspace-aware `justfile`, CI matrix, and lefthook pre-commit/pre-push hooks.
