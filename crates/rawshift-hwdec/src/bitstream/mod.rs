//! Safe parsing and framing shared by the platform hardware-decode backends.
//!
//! Each backend consumes a different subset of this module, because the
//! platform APIs sit at different levels. VAAPI is a slice-level API, so it
//! needs everything down to slice headers, tile groups and the
//! `build_*_param` builders. VideoToolbox takes the configuration record and
//! the coded sample almost as-is, so it only needs `hvcC`/`av1C` parsing, NAL
//! classification and the AV1 sequence header — it lets the framework do the
//! rest. The unused remainder is therefore expected rather than dead, and the
//! allow below keeps `-D warnings` builds honest for every backend selection
//! without scattering per-item `cfg`s that would couple this module to the
//! backend list.
#![allow(dead_code)]

pub(crate) mod av1;
pub(crate) mod bits;
pub(crate) mod hevc;
