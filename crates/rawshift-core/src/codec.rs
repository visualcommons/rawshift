//! Codec identity and shared encode vocabulary.
//!
//! These types are format-agnostic so `rawshift-image` and a future
//! `rawshift-video` can describe their codecs with one shared vocabulary.

use std::fmt;

/// A stable identifier for one codec implementation, in `"{format}/{impl}"` form.
///
/// For example `"jpeg/mozjpeg"` or `"avif/gamut"`. The string is stable across
/// releases, so callers may use it as part of a cache key.
///
/// Only [`Serialize`](serde::Serialize) is derived: the `&'static str` is
/// discovered at runtime by enumerating compiled codecs, never deserialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CodecId {
    /// The `"{format}/{impl}"` identifier string.
    pub id: &'static str,
}

impl CodecId {
    /// Create a codec id from a static `"{format}/{impl}"` string.
    pub const fn new(id: &'static str) -> Self {
        Self { id }
    }
}

impl fmt::Display for CodecId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id)
    }
}

/// Whether a codec encodes or decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CodecDirection {
    /// Produces an encoded image from pixels.
    Encode,
    /// Produces pixels from an encoded image.
    Decode,
}

/// Identity and version of one compiled-in codec implementation.
///
/// Returned by `available_encoders` / `available_decoders` in `rawshift-image`.
/// Serialize-only, because it embeds a [`CodecId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CodecInfo {
    /// Stable implementation identifier.
    pub id: CodecId,
    /// Backend version string — a crate version for pure-Rust backends, or a
    /// runtime-reported library version for C/C++ backends.
    pub version: String,
    /// Whether this entry encodes or decodes.
    pub direction: CodecDirection,
}

impl CodecInfo {
    /// Construct a [`CodecInfo`].
    pub fn new(id: CodecId, version: impl Into<String>, direction: CodecDirection) -> Self {
        Self {
            id,
            version: version.into(),
            direction,
        }
    }
}

/// Controls which metadata blocks an encoder embeds into its output container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MetadataEmbedOptions {
    /// Embed EXIF metadata. Default: `true`.
    pub embed_exif: bool,
    /// Embed the ICC color profile. Default: `true`.
    pub embed_icc: bool,
    /// Embed XMP metadata. Default: `true`.
    pub embed_xmp: bool,
}

impl MetadataEmbedOptions {
    /// Embed every supported metadata block (this is also [`Default`]).
    pub fn all() -> Self {
        Self {
            embed_exif: true,
            embed_icc: true,
            embed_xmp: true,
        }
    }

    /// Embed no metadata.
    pub fn none() -> Self {
        Self {
            embed_exif: false,
            embed_icc: false,
            embed_xmp: false,
        }
    }

    /// True if at least one metadata block is requested.
    pub fn any(&self) -> bool {
        self.embed_exif || self.embed_icc || self.embed_xmp
    }
}

impl Default for MetadataEmbedOptions {
    fn default() -> Self {
        Self::all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_id_display() {
        let id = CodecId::new("jpeg/mozjpeg");
        assert_eq!(id.to_string(), "jpeg/mozjpeg");
    }

    #[test]
    fn metadata_embed_constructors() {
        assert_eq!(MetadataEmbedOptions::default(), MetadataEmbedOptions::all());
        assert!(MetadataEmbedOptions::all().any());
        assert!(!MetadataEmbedOptions::none().any());
    }
}
