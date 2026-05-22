//! Safe wrapper around libheif (via the `libheif-rs` crate) for HEIC/HEIF decoding.
//!
//! All `libheif-rs`/`libheif` interaction is confined to this module. Callers
//! receive plain-data structs and `Result<_, String>`; `src/formats` maps the
//! `String` into a [`FormatError::ImageDecode`](crate::error::FormatError).
//!
//! libheif handles ISOBMFF container parsing, HEVC decoding, grid (tiled) image
//! stitching, and the `irot`/`imir` geometric transforms — see [`decode_primary`].

use libheif_rs::{
    AuxiliaryImagesFilter, ColorProfile, ColorSpace, HeifContext, ImageHandle, LibHeif, RgbChroma,
};

/// A decoded HEIC image as interleaved 16-bit RGB (alpha dropped).
///
/// libheif applies the container's `irot`/`imir` geometric transforms during
/// decode, so `rgb` is already correctly oriented. 8-bit sources are scaled to
/// 16-bit by `*257`; HDR (10/12-bit) sources are bit-replicated to full range.
pub struct DecodedHeic {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Interleaved RGB samples, row-major, length `width * height * 3`.
    pub rgb: Vec<u16>,
}

/// Classification of an auxiliary/derived image inside a HEIC file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HeicAuxKind {
    /// A scaled-down preview thumbnail.
    Thumbnail,
    /// A depth or disparity map.
    DepthMap,
    /// An HDR gain map (e.g. Apple `urn:com:apple:photo:2020:aux:hdrgainmap`).
    GainMap,
    /// Any other auxiliary image (alpha mask, unrecognised URN, …).
    Auxiliary,
}

/// Lightweight descriptor of one auxiliary image referenced by the primary image.
pub struct HeicAuxInfo {
    /// What kind of auxiliary image this is.
    pub kind: HeicAuxKind,
    /// Auxiliary image width in pixels.
    pub width: u32,
    /// Auxiliary image height in pixels.
    pub height: u32,
    /// libheif item id — pass to [`decode_aux`] to decode this image.
    pub item_id: u32,
    /// Auxiliary type URN string, when the container provides one.
    pub aux_type: Option<String>,
}

/// Raw metadata blocks and container facts pulled from a HEIC file.
#[derive(Default)]
pub struct HeicMetaBlobs {
    /// Clean TIFF-structured EXIF byte stream (libheif's 4-byte offset prefix
    /// stripped), if present.
    pub exif: Option<Vec<u8>>,
    /// Raw XMP (RDF/XML) packet bytes, if present.
    pub xmp: Option<Vec<u8>>,
    /// Raw embedded ICC color profile bytes (`rICC`/`prof` boxes), if present.
    pub icc: Option<Vec<u8>>,
    /// Luma bits-per-channel of the primary image (8, 10, 12, …).
    pub bit_depth: u8,
    /// Whether the primary image carries an alpha channel.
    pub has_alpha: bool,
    /// Primary image width in pixels.
    pub width: u32,
    /// Primary image height in pixels.
    pub height: u32,
}

/// Open a HEIF context over `data`.
///
/// `libheif` borrows the buffer without copying, so the returned context
/// borrows `data` for its lifetime.
fn open_ctx(data: &[u8]) -> Result<HeifContext<'_>, String> {
    HeifContext::read_from_bytes(data).map_err(|e| format!("failed to parse HEIC container: {e}"))
}

/// Bit-replicate a `depth`-bit sample up to the full 16-bit range.
///
/// This matches the `*257` convention used for 8-bit sources elsewhere in the
/// crate: it is exact at both `0` and the maximum value (for `depth` in `8..=15`).
#[inline]
fn upscale_to_u16(v: u16, depth: u8) -> u16 {
    if depth >= 16 || depth == 0 {
        return v;
    }
    let d = depth as u32;
    let v = v as u32;
    let scaled = if 2 * d >= 16 {
        (v << (16 - d)) | (v >> (2 * d - 16))
    } else {
        v << (16 - d)
    };
    scaled as u16
}

/// Decode an [`ImageHandle`] to interleaved 16-bit RGB.
fn handle_to_decoded(lib: &LibHeif, handle: &ImageHandle) -> Result<DecodedHeic, String> {
    let bit_depth = handle.luma_bits_per_pixel().max(8);
    let hdr = bit_depth > 8;
    let chroma = if hdr {
        RgbChroma::HdrRgbBe
    } else {
        RgbChroma::Rgb
    };

    // libheif's high-level decode also carries out all geometric transformations
    // specified in the container (rotation, mirroring, cropping) and stitches
    // `grid` (tiled) images, so the output needs no further fix-up.
    let image = lib
        .decode(handle, ColorSpace::Rgb(chroma), None)
        .map_err(|e| format!("HEVC decode failed: {e}"))?;

    let planes = image.planes();
    let plane = planes
        .interleaved
        .ok_or_else(|| "decoded HEIC image has no interleaved RGB plane".to_string())?;

    let w = plane.width as usize;
    let h = plane.height as usize;
    let stride = plane.stride;
    let data = plane.data;
    let mut rgb = vec![0u16; w * h * 3];

    if hdr {
        // 6 bytes per pixel: three big-endian 16-bit samples. The real
        // per-channel depth is `plane.bits_per_pixel`; the value occupies the
        // low bits of each 16-bit word.
        let depth = if plane.bits_per_pixel == 0 {
            bit_depth
        } else {
            plane.bits_per_pixel
        };
        let mask: u16 = if depth >= 16 {
            u16::MAX
        } else {
            (1u16 << depth) - 1
        };
        for y in 0..h {
            let row_start = y * stride;
            let row = data
                .get(row_start..row_start + w * 6)
                .ok_or_else(|| "HEIC HDR plane shorter than expected".to_string())?;
            for x in 0..w {
                let dst = (y * w + x) * 3;
                for c in 0..3 {
                    let hi = row[x * 6 + c * 2] as u16;
                    let lo = row[x * 6 + c * 2 + 1] as u16;
                    let v = (((hi << 8) | lo) & mask).min(mask);
                    rgb[dst + c] = upscale_to_u16(v, depth);
                }
            }
        }
    } else {
        // 3 bytes per pixel, 8-bit samples scaled to 16-bit by `*257`.
        for y in 0..h {
            let row_start = y * stride;
            let row = data
                .get(row_start..row_start + w * 3)
                .ok_or_else(|| "HEIC RGB plane shorter than expected".to_string())?;
            for x in 0..w {
                let dst = (y * w + x) * 3;
                rgb[dst] = (row[x * 3] as u16) * 257;
                rgb[dst + 1] = (row[x * 3 + 1] as u16) * 257;
                rgb[dst + 2] = (row[x * 3 + 2] as u16) * 257;
            }
        }
    }

    Ok(DecodedHeic {
        width: plane.width,
        height: plane.height,
        rgb,
    })
}

/// Decode the primary image of a HEIC file to interleaved 16-bit RGB.
pub fn decode_primary(data: &[u8]) -> Result<DecodedHeic, String> {
    // TODO(platform): on Apple targets a hardware-accelerated path via ImageIO
    //   (CGImageSource) and on Android via the NDK ImageDecoder could be
    //   selected here. A single cross-platform libheif backend is used for now.
    let ctx = open_ctx(data)?;
    let lib = LibHeif::new();
    let handle = ctx
        .primary_image_handle()
        .map_err(|e| format!("no primary image in HEIC file: {e}"))?;
    handle_to_decoded(&lib, &handle)
}

/// Classify an auxiliary image from its type URN.
fn classify_aux(aux_type: Option<&str>) -> HeicAuxKind {
    match aux_type {
        Some(t) => {
            let l = t.to_ascii_lowercase();
            if l.contains("gainmap") || l.contains("hdrgain") {
                HeicAuxKind::GainMap
            } else if l.contains("depth") || l.contains("disparity") {
                HeicAuxKind::DepthMap
            } else {
                HeicAuxKind::Auxiliary
            }
        }
        None => HeicAuxKind::Auxiliary,
    }
}

/// Enumerate the thumbnails, depth maps, gain maps, and auxiliary images
/// referenced by the primary image.
pub fn list_aux_images(data: &[u8]) -> Result<Vec<HeicAuxInfo>, String> {
    // TODO(platform): platform-native containers expose the same items; the
    //   libheif enumeration below is cross-platform.
    let ctx = open_ctx(data)?;
    let handle = ctx
        .primary_image_handle()
        .map_err(|e| format!("no primary image in HEIC file: {e}"))?;
    let mut out = Vec::new();

    // Thumbnails.
    let n_thumbs = handle.number_of_thumbnails();
    if n_thumbs > 0 {
        let mut ids = vec![0u32; n_thumbs];
        let got = handle.thumbnail_ids(&mut ids);
        ids.truncate(got);
        for id in ids {
            if let Ok(th) = handle.thumbnail(id) {
                out.push(HeicAuxInfo {
                    kind: HeicAuxKind::Thumbnail,
                    width: th.width(),
                    height: th.height(),
                    item_id: id,
                    aux_type: None,
                });
            }
        }
    }

    // Depth images.
    let n_depth = handle.number_of_depth_images().max(0) as usize;
    if n_depth > 0 {
        let mut ids = vec![0u32; n_depth];
        let got = handle.depth_image_ids(&mut ids);
        ids.truncate(got);
        for id in ids {
            if let Ok(dh) = handle.depth_image_handle(id) {
                out.push(HeicAuxInfo {
                    kind: HeicAuxKind::DepthMap,
                    width: dh.width(),
                    height: dh.height(),
                    item_id: id,
                    aux_type: None,
                });
            }
        }
    }

    // Auxiliary images (alpha, gain maps, …). Depth is enumerated above, so it
    // is omitted here to avoid listing the same item twice.
    for aux in handle.auxiliary_images(AuxiliaryImagesFilter::new().omit_depth()) {
        let aux_type = aux.auxiliary_type().ok().filter(|s| !s.is_empty());
        out.push(HeicAuxInfo {
            kind: classify_aux(aux_type.as_deref()),
            width: aux.width(),
            height: aux.height(),
            item_id: aux.item_id(),
            aux_type,
        });
    }

    Ok(out)
}

/// Decode a single auxiliary image by its libheif item id.
pub fn decode_aux(data: &[u8], item_id: u32) -> Result<DecodedHeic, String> {
    let ctx = open_ctx(data)?;
    let lib = LibHeif::new();
    let handle = ctx
        .image_handle(item_id)
        .map_err(|e| format!("HEIC auxiliary image {item_id} not found: {e}"))?;
    handle_to_decoded(&lib, &handle)
}

/// libheif stores the EXIF item with a leading 4-byte big-endian offset that
/// points at the start of the TIFF header. Strip it so the result is a clean
/// TIFF byte stream.
fn strip_exif_prefix(raw: Vec<u8>) -> Option<Vec<u8>> {
    if raw.len() < 4 {
        return None;
    }
    let offset = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    let start = 4usize.checked_add(offset)?;
    if start >= raw.len() {
        return None;
    }
    Some(raw[start..].to_vec())
}

/// Pull EXIF / XMP / ICC metadata blocks and basic container facts from a HEIC file.
pub fn extract_metadata_blobs(data: &[u8]) -> Result<HeicMetaBlobs, String> {
    let ctx = open_ctx(data)?;
    let handle = ctx
        .primary_image_handle()
        .map_err(|e| format!("no primary image in HEIC file: {e}"))?;

    let mut blobs = HeicMetaBlobs {
        bit_depth: handle.luma_bits_per_pixel().max(8),
        has_alpha: handle.has_alpha_channel(),
        width: handle.width(),
        height: handle.height(),
        ..Default::default()
    };

    for md in handle.all_metadata() {
        if &md.item_type.0 == b"Exif" {
            blobs.exif = strip_exif_prefix(md.raw_data);
        } else if md.content_type == "application/rdf+xml" {
            blobs.xmp = Some(md.raw_data);
        }
    }

    if let Some(profile) = handle.color_profile_raw() {
        // Keep only genuine ICC profiles (`rICC`/`prof`); skip `nclx`.
        let typ = profile.profile_type().0;
        if &typ == b"rICC" || &typ == b"prof" {
            blobs.icc = Some(profile.data);
        }
    }

    Ok(blobs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upscale_endpoints_are_exact() {
        // 8-bit: 0 -> 0, 255 -> 65535 (matches the `*257` convention).
        assert_eq!(upscale_to_u16(0, 8), 0);
        assert_eq!(upscale_to_u16(255, 8), 65535);
        // 10-bit: 0 -> 0, 1023 -> 65535.
        assert_eq!(upscale_to_u16(0, 10), 0);
        assert_eq!(upscale_to_u16(1023, 10), 65535);
        // 12-bit: 4095 -> 65535.
        assert_eq!(upscale_to_u16(4095, 12), 65535);
        // 16-bit: identity.
        assert_eq!(upscale_to_u16(12345, 16), 12345);
    }

    #[test]
    fn strip_exif_prefix_zero_offset() {
        // 4-byte offset of 0 -> TIFF stream starts immediately after the prefix.
        let raw = vec![0, 0, 0, 0, b'I', b'I', 0x2A, 0x00];
        assert_eq!(strip_exif_prefix(raw), Some(vec![b'I', b'I', 0x2A, 0x00]));
    }

    #[test]
    fn strip_exif_prefix_nonzero_offset() {
        // Offset of 2 -> skip 4-byte prefix + 2 pad bytes.
        let raw = vec![0, 0, 0, 2, 0xAA, 0xBB, b'M', b'M'];
        assert_eq!(strip_exif_prefix(raw), Some(vec![b'M', b'M']));
    }

    #[test]
    fn strip_exif_prefix_rejects_short_or_oob() {
        assert_eq!(strip_exif_prefix(vec![0, 0]), None);
        assert_eq!(strip_exif_prefix(vec![0, 0, 0, 99, 1, 2]), None);
    }

    #[test]
    fn classify_aux_recognises_urns() {
        assert_eq!(
            classify_aux(Some("urn:com:apple:photo:2020:aux:hdrgainmap")),
            HeicAuxKind::GainMap
        );
        assert_eq!(
            classify_aux(Some("urn:mpeg:hevc:2015:auxid:2:depth")),
            HeicAuxKind::DepthMap
        );
        assert_eq!(
            classify_aux(Some("urn:mpeg:hevc:2015:auxid:1")),
            HeicAuxKind::Auxiliary
        );
        assert_eq!(classify_aux(None), HeicAuxKind::Auxiliary);
    }

    #[test]
    fn decode_primary_rejects_junk() {
        let junk = vec![0u8; 64];
        assert!(decode_primary(&junk).is_err());
    }

    #[test]
    fn extract_metadata_blobs_rejects_junk() {
        let junk = vec![0u8; 64];
        assert!(extract_metadata_blobs(&junk).is_err());
    }

    /// Decode libheif's bundled sample file when it is available on this
    /// machine (Homebrew install). Skips gracefully otherwise.
    #[test]
    fn decode_primary_homebrew_sample() {
        let candidates = [
            "/opt/homebrew/share/libheif/example.heic",
            "/usr/local/share/libheif/example.heic",
        ];
        let Some(path) = candidates.iter().find(|p| std::path::Path::new(p).exists()) else {
            eprintln!("skipping: no libheif sample HEIC found");
            return;
        };
        let data = std::fs::read(path).expect("read sample heic");
        let decoded = decode_primary(&data).expect("decode sample heic");
        assert!(decoded.width > 0 && decoded.height > 0);
        assert_eq!(
            decoded.rgb.len(),
            decoded.width as usize * decoded.height as usize * 3
        );
    }
}
