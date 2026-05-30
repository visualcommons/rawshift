//! Enumeration of the codec implementations compiled into this build.
//!
//! [`available_encoders`] and [`available_decoders`] report exactly which
//! backends the current Cargo feature set provides — useful for populating a
//! UI picker, or for deriving a cache key from "whatever rawshift was compiled
//! with".
//!
//! Version strings are hand-maintained `major.minor` values: Cargo offers no
//! way to read a pure-Rust dependency's version into a dependent crate. Bump a
//! string here together with the matching dependency in `Cargo.toml`.

// The `Vec::new()` + cfg-gated `push` pattern is intentional: which entries
// exist depends on the active feature set, so a `vec![]` literal cannot be used.
#![allow(clippy::vec_init_then_push)]

// `CodecId` and `CodecDirection` are referenced only by the cfg-gated `push`
// calls below; a zero-feature build compiles none of them in.
#[cfg_attr(
    not(any(any_standard_encode, any_standard_decode)),
    allow(unused_imports)
)]
use crate::core::{CodecDirection, CodecId, CodecInfo};

/// Every encoder implementation compiled into this build, one [`CodecInfo`] per
/// backend.
pub fn available_encoders() -> Vec<CodecInfo> {
    #[allow(unused_mut)]
    let mut encoders: Vec<CodecInfo> = Vec::new();
    #[cfg(feature = "png-encode")]
    encoders.push(CodecInfo::new(
        CodecId::new("png/zune"),
        "0.5",
        CodecDirection::Encode,
    ));
    #[cfg(feature = "jpeg-encode")]
    encoders.push(CodecInfo::new(
        CodecId::new("jpeg/jpeg-encoder"),
        "0.7",
        CodecDirection::Encode,
    ));
    #[cfg(feature = "jpeg-encode-jpegli")]
    encoders.push(CodecInfo::new(
        CodecId::new("jpeg/jpegli"),
        "0.11",
        CodecDirection::Encode,
    ));
    #[cfg(feature = "webp-encode")]
    encoders.push(CodecInfo::new(
        CodecId::new("webp/libwebp"),
        "0.14",
        CodecDirection::Encode,
    ));
    #[cfg(feature = "avif-encode")]
    encoders.push(CodecInfo::new(
        CodecId::new("avif/ravif"),
        "0.13",
        CodecDirection::Encode,
    ));
    // Version tracks the libaom bundled by `libaom-sys` (vendored); a system
    // libaom may differ. Hand-maintained — bump with the `libaom-sys` dependency.
    #[cfg(feature = "avif-encode-libaom")]
    encoders.push(CodecInfo::new(
        CodecId::new("avif/libaom"),
        "3.11",
        CodecDirection::Encode,
    ));
    #[cfg(feature = "jxl-encode")]
    encoders.push(CodecInfo::new(
        CodecId::new("jxl/zune"),
        "0.5",
        CodecDirection::Encode,
    ));
    #[cfg(feature = "jxl-encode-libjxl")]
    encoders.push(CodecInfo::new(
        CodecId::new("jxl/libjxl"),
        "0.11",
        CodecDirection::Encode,
    ));
    #[cfg(feature = "dng-encode")]
    encoders.push(CodecInfo::new(
        CodecId::new("dng/rawshift"),
        env!("CARGO_PKG_VERSION"),
        CodecDirection::Encode,
    ));
    encoders
}

/// Every decoder implementation compiled into this build, one [`CodecInfo`] per
/// backend.
pub fn available_decoders() -> Vec<CodecInfo> {
    #[allow(unused_mut)]
    let mut decoders: Vec<CodecInfo> = Vec::new();
    #[cfg(feature = "jpeg-decode")]
    decoders.push(CodecInfo::new(
        CodecId::new("jpeg/zune"),
        "0.5",
        CodecDirection::Decode,
    ));
    #[cfg(feature = "png-decode")]
    decoders.push(CodecInfo::new(
        CodecId::new("png/zune"),
        "0.5",
        CodecDirection::Decode,
    ));
    #[cfg(feature = "webp-decode")]
    decoders.push(CodecInfo::new(
        CodecId::new("webp/libwebp"),
        "0.14",
        CodecDirection::Decode,
    ));
    #[cfg(feature = "jxl-decode")]
    decoders.push(CodecInfo::new(
        CodecId::new("jxl/jxl-oxide"),
        "0.12",
        CodecDirection::Decode,
    ));
    #[cfg(feature = "gif-decode")]
    decoders.push(CodecInfo::new(
        CodecId::new("gif/gif"),
        "0.13",
        CodecDirection::Decode,
    ));
    #[cfg(feature = "tiff-decode")]
    decoders.push(CodecInfo::new(
        CodecId::new("tiff/tiff"),
        "0.11",
        CodecDirection::Decode,
    ));
    #[cfg(feature = "avif-decode")]
    decoders.push(CodecInfo::new(
        CodecId::new("avif/image"),
        "0.25",
        CodecDirection::Decode,
    ));
    #[cfg(feature = "heic-decode")]
    decoders.push(CodecInfo::new(
        CodecId::new("heic/libheif"),
        "2.7",
        CodecDirection::Decode,
    ));
    #[cfg(feature = "svg-decode")]
    decoders.push(CodecInfo::new(
        CodecId::new("svg/resvg"),
        "0.44",
        CodecDirection::Decode,
    ));
    #[cfg(feature = "ppm-decode")]
    decoders.push(CodecInfo::new(
        CodecId::new("ppm/zune"),
        "0.5",
        CodecDirection::Decode,
    ));
    decoders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoders_are_well_formed() {
        for info in available_encoders() {
            assert_eq!(info.direction, CodecDirection::Encode);
            assert!(
                info.id.id.contains('/'),
                "codec id should be `format/impl`: {}",
                info.id
            );
            assert!(!info.version.is_empty());
        }
    }

    #[test]
    fn decoders_are_well_formed() {
        for info in available_decoders() {
            assert_eq!(info.direction, CodecDirection::Decode);
            assert!(info.id.id.contains('/'));
        }
    }

    #[test]
    fn codec_ids_are_unique_per_direction() {
        for list in [available_encoders(), available_decoders()] {
            let mut ids: Vec<&str> = list.iter().map(|c| c.id.id).collect();
            let total = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), total, "codec ids must be unique");
        }
    }
}
