//! Container-level metadata.
//!
//! The EXIF/XMP/GPS core is [`rawshift_core::ImageMetadata`], reused as-is:
//! `rawshift-core` documents that model as "genuinely video-shared … EXIF/XMP
//! semantics are the same for a video file as for a still", and this is its
//! first video consumer. What video adds on top is the *movie*-level facts a
//! still has no analogue for, which is what [`ContainerMetadata`] holds.

use std::time::Duration;

use rawshift_core::{ImageMetadata, MetadataEntry, MetadataKey, MetadataNamespace, MetadataValue};

use crate::id::ContainerId;

/// Everything rawshift read from a video file's headers.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct VideoMetadata {
    /// The media-agnostic core: EXIF, XMP, GPS, camera identification, and the
    /// generic `extra` table. Container-level facts are also projected into
    /// `common.extra` under the [`MetadataNamespace::Quicktime`] and
    /// [`MetadataNamespace::Matroska`] namespaces, so a caller that only knows
    /// `ImageMetadata` still sees them.
    pub common: ImageMetadata,
    /// Movie-level facts with no still-image analogue.
    pub container: ContainerMetadata,
}

/// Movie-level facts: the parts of a container that describe the file as a
/// whole rather than any one track.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ContainerMetadata {
    /// Which container this came from.
    pub container: Option<ContainerId>,
    /// Total presentation duration, when the container states one.
    pub duration: Option<Duration>,
    /// The movie timescale, in ticks per second.
    ///
    /// The movie header's own unit, distinct from any track's `time_base`.
    pub timescale: Option<u32>,
    /// Creation time, as the container states it.
    pub creation_time: Option<String>,
    /// Modification time, as the container states it.
    pub modification_time: Option<String>,
    /// ISOBMFF `ftyp` major brand and compatible brands (`"qt  "`, `"mp42"`,
    /// `"isom"`, …). Empty for non-ISOBMFF containers.
    pub brands: Vec<String>,
    /// The writing application or muxer, when stated.
    pub writing_app: Option<String>,
}

impl VideoMetadata {
    /// Record a container-level fact into the shared `extra` table.
    ///
    /// The single place container facts enter [`ImageMetadata`], so every
    /// demuxer namespaces them the same way and a caller can find them all
    /// without knowing which container produced them.
    pub fn push_container_entry(
        &mut self,
        namespace: MetadataNamespace,
        tag: impl Into<String>,
        value: MetadataValue,
    ) {
        self.common.extra.push(MetadataEntry {
            key: MetadataKey::new(namespace, tag),
            value,
        });
    }

    /// Look up a container-level fact previously recorded.
    #[must_use]
    pub fn container_entry(
        &self,
        namespace: MetadataNamespace,
        tag: &str,
    ) -> Option<&MetadataValue> {
        self.common
            .extra
            .iter()
            .find(|e| e.key.namespace == namespace && e.key.tag == tag)
            .map(|e| &e.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_entries_round_trip_through_the_shared_table() {
        let mut md = VideoMetadata::default();
        md.push_container_entry(
            MetadataNamespace::Quicktime,
            "com.apple.quicktime.model",
            MetadataValue::Text("iPhone 17 Pro Max".into()),
        );

        assert_eq!(
            md.container_entry(MetadataNamespace::Quicktime, "com.apple.quicktime.model"),
            Some(&MetadataValue::Text("iPhone 17 Pro Max".into()))
        );
        // It really landed in the shared table, not a side channel.
        assert_eq!(md.common.extra.len(), 1);
    }

    #[test]
    fn lookup_is_namespaced_not_just_tag_matched() {
        let mut md = VideoMetadata::default();
        md.push_container_entry(
            MetadataNamespace::Matroska,
            "title",
            MetadataValue::Text("mkv".into()),
        );
        // Same tag, different namespace: must not match.
        assert_eq!(
            md.container_entry(MetadataNamespace::Quicktime, "title"),
            None
        );
        assert!(
            md.container_entry(MetadataNamespace::Matroska, "title")
                .is_some()
        );
    }

    #[test]
    fn a_default_is_empty_rather_than_absent() {
        let md = VideoMetadata::default();
        assert!(md.common.extra.is_empty());
        assert_eq!(md.container.container, None);
        assert!(md.container.brands.is_empty());
    }
}
