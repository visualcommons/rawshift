//! HEIC/HEIF format support — public API.
//!
//! [`HeicFile`] is the entry point for HEIC files. It decodes the primary image
//! and gives full access to auxiliary content (thumbnails, depth maps, HDR gain
//! maps, alpha/auxiliary images) and to embedded EXIF/XMP/ICC metadata.
//!
//! HEIC is also reachable through the generic standard-format API
//! ([`decode_standard_image`](crate::formats::decode_standard_image)); use
//! [`HeicFile`] when you need the auxiliary images or richer metadata.

use crate::codecs::heic;
use crate::core::RgbImage;
use crate::core::metadata::{ImageMetadata, MetadataKey, MetadataNamespace, MetadataValue};
use crate::error::{FormatError, RawError, RawResult};
use crate::metadata::exif::ExifParser;

pub use crate::codecs::heic::HeicAuxKind;

/// Map a libheif wrapper error string into a [`RawError`].
fn heic_err(message: String) -> RawError {
    RawError::Format(FormatError::ImageDecode {
        format: "HEIC",
        message,
    })
}

/// Descriptor of one auxiliary image inside a [`HeicFile`].
///
/// Decode it with [`HeicFile::decode_aux`].
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HeicAuxImage {
    /// What kind of auxiliary image this is.
    pub kind: HeicAuxKind,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Auxiliary type URN string, when the container provides one.
    pub aux_type: Option<String>,
    /// Opaque libheif item id used to decode this image.
    pub(crate) item_id: u32,
}

/// A parsed HEIC/HEIF file.
///
/// Construct with [`HeicFile::open`]. Opening only parses the container (cheap);
/// the CPU-heavy HEVC decode happens in [`decode_primary`](Self::decode_primary)
/// and [`decode_aux`](Self::decode_aux).
pub struct HeicFile {
    data: Vec<u8>,
    aux: Vec<HeicAuxImage>,
}

impl std::fmt::Debug for HeicFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeicFile")
            .field("data_len", &self.data.len())
            .field("aux", &self.aux)
            .finish()
    }
}

impl HeicFile {
    /// Parse a HEIC file from its raw bytes.
    ///
    /// Only the container is parsed here, so this is cheap. The auxiliary-image
    /// table is populated up front; pixel decoding is deferred.
    ///
    /// # Errors
    /// Returns [`RawError::Format`] if `data` is not a readable HEIC file.
    pub fn open(data: Vec<u8>) -> RawResult<Self> {
        let infos = heic::list_aux_images(&data).map_err(heic_err)?;
        let aux = infos
            .into_iter()
            .map(|i| HeicAuxImage {
                kind: i.kind,
                width: i.width,
                height: i.height,
                aux_type: i.aux_type,
                item_id: i.item_id,
            })
            .collect();
        Ok(Self { data, aux })
    }

    /// Decode the primary image to a 16-bit RGB image.
    ///
    /// Geometric transforms (`irot`/`imir`) and grid (tiled) stitching are
    /// already applied; HDR (10/12-bit) sources are scaled to the full 16-bit
    /// range.
    pub fn decode_primary(&self) -> RawResult<RgbImage> {
        let decoded = heic::decode_primary(&self.data).map_err(heic_err)?;
        RgbImage::new(decoded.width, decoded.height, decoded.rgb)
    }

    /// Extract embedded EXIF/XMP/ICC and full typed metadata.
    pub fn metadata(&self) -> ImageMetadata {
        read_heic_metadata(&self.data)
    }

    /// All auxiliary images (thumbnails, depth maps, HDR gain maps, auxiliary).
    pub fn aux_images(&self) -> &[HeicAuxImage] {
        &self.aux
    }

    /// Decode a specific auxiliary image to a 16-bit RGB image.
    ///
    /// Single-channel sources (depth maps, gain maps) are returned as
    /// grayscale-expanded RGB.
    pub fn decode_aux(&self, aux: &HeicAuxImage) -> RawResult<RgbImage> {
        let decoded = heic::decode_aux(&self.data, aux.item_id).map_err(heic_err)?;
        RgbImage::new(decoded.width, decoded.height, decoded.rgb)
    }

    /// Decode the embedded thumbnail, if the file carries one.
    ///
    /// Returns `Ok(None)` when there is no thumbnail. (libheif does not expose
    /// the thumbnail's coded bitstream, so a decoded image is returned.)
    pub fn thumbnail(&self) -> RawResult<Option<RgbImage>> {
        match self.aux.iter().find(|a| a.kind == HeicAuxKind::Thumbnail) {
            Some(thumb) => Ok(Some(self.decode_aux(thumb)?)),
            None => Ok(None),
        }
    }
}

/// Extract unified metadata from HEIC file bytes.
///
/// Reads the embedded EXIF, XMP, and ICC profile and maps them onto
/// [`ImageMetadata`]. Returns a default (empty) value when the file carries no
/// metadata or cannot be parsed. Used by both [`HeicFile::metadata`] and
/// [`read_standard_image_metadata`](crate::formats::read_standard_image_metadata).
pub fn read_heic_metadata(data: &[u8]) -> ImageMetadata {
    use little_exif::filetype::FileExtension;

    let blobs = match heic::extract_metadata_blobs(data) {
        Ok(b) => b,
        Err(_) => return ImageMetadata::default(),
    };

    // Parse the EXIF block (a raw TIFF stream) into typed + generic fields.
    let mut md = match blobs.exif {
        Some(ref exif) => ExifParser::parse_from_bytes(exif, FileExtension::TIFF),
        None => ImageMetadata::default(),
    };

    md.exif_raw = blobs.exif;
    md.xmp = blobs.xmp;
    md.icc_profile = blobs.icc;
    if md.image.bit_depth == 0 {
        md.image.bit_depth = blobs.bit_depth;
    }

    // Record HEIC container facts in the generic table.
    md.insert(
        MetadataKey::new(MetadataNamespace::Heic, "bit_depth"),
        MetadataValue::U64(blobs.bit_depth as u64),
    );
    md.insert(
        MetadataKey::new(MetadataNamespace::Heic, "has_alpha"),
        MetadataValue::U64(blobs.has_alpha as u64),
    );
    md.insert(
        MetadataKey::new(MetadataNamespace::Heic, "width"),
        MetadataValue::U64(blobs.width as u64),
    );
    md.insert(
        MetadataKey::new(MetadataNamespace::Heic, "height"),
        MetadataValue::U64(blobs.height as u64),
    );

    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_rejects_junk() {
        let junk = vec![0u8; 64];
        assert!(HeicFile::open(junk).is_err());
    }

    #[test]
    fn read_metadata_junk_returns_default() {
        let junk = vec![0u8; 64];
        assert_eq!(read_heic_metadata(&junk), ImageMetadata::default());
    }

    /// Exercise the full public API against libheif's bundled sample file when
    /// it is available on this machine. Skips gracefully otherwise.
    #[test]
    fn open_and_decode_homebrew_sample() {
        let candidates = [
            "/opt/homebrew/share/libheif/example.heic",
            "/usr/local/share/libheif/example.heic",
        ];
        let Some(path) = candidates.iter().find(|p| std::path::Path::new(p).exists()) else {
            eprintln!("skipping: no libheif sample HEIC found");
            return;
        };
        let data = std::fs::read(path).expect("read sample heic");

        let file = HeicFile::open(data).expect("open sample heic");
        let primary = file.decode_primary().expect("decode primary");
        assert!(primary.width() > 0 && primary.height() > 0);
        assert_eq!(
            primary.data().len(),
            primary.width() as usize * primary.height() as usize * 3
        );

        // Every enumerated auxiliary image must decode.
        for aux in file.aux_images() {
            let img = file.decode_aux(aux).expect("decode aux image");
            assert!(img.width() > 0 && img.height() > 0);
        }

        // Metadata extraction must not panic.
        let _ = file.metadata();
    }
}
