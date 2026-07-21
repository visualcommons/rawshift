//! AVIF format support — public API on gamut-avif.
//!
//! [`AvifFile`] is the entry point for AVIF files. The container is parsed by
//! [`gamut_avif`], which owns everything around the coded picture: item roles,
//! `grid`/`iden`/`iovl` derivation, colour conversion, alpha merge, and the
//! `clap`/`irot`/`imir` transforms. The AV1 codestream itself is decoded
//! through gamut-avif's pluggable [`Av1StillDecoder`](gamut_avif::Av1StillDecoder)
//! seam, which rawshift plugs with a [`rawshift-hwdec`] hardware decoder when
//! the `hw` feature is enabled.
//!
//! **Container parsing, metadata, and auxiliary-image enumeration always work
//! with no hardware backend at all.** Only pixel decode
//! ([`decode_primary`](AvifFile::decode_primary) /
//! [`decode_aux`](AvifFile::decode_aux) / [`thumbnail`](AvifFile::thumbnail))
//! needs a hardware AV1 decoder; without one it fails with the matchable
//! [`RawError::HwDecoderUnavailable`]. Probe up front with
//! [`avif_hw_decode_available`].
//!
//! Presentation is 8-bit today: a 10/12-bit AVIF hardware-decodes fine at the
//! rawshift-hwdec level (P010), but gamut-avif's RGBA presentation surface
//! rejects `bit_depth > 8` until high-bit-depth presentation lands upstream
//! (the visualcommons/gamut#303 program), so such files report a matchable
//! [`RawError::Format`] here. Software AV1 decode (Windows/musl/wasm) is
//! post-v1 via visualcommons/gamut#259 — AVIF pixel decode is hardware-only,
//! like HEIC.
//!
//! AVIF is also reachable through the generic standard-format API
//! ([`decode_standard_image`](crate::formats::decode_standard_image)); use
//! [`AvifFile`] when you need the auxiliary images or richer metadata.
//!
//! [`rawshift-hwdec`]: https://docs.rs/rawshift-hwdec

use gamut_avif::{AvifContainer, AvifImage, AvifItem};
use gamut_isobmff::ColourInformation;

use crate::core::RgbImage;
use crate::core::metadata::{ImageMetadata, MetadataKey, MetadataNamespace, MetadataValue};
use crate::error::{FormatError, RawError, RawResult};
use crate::metadata::exif::ExifParser;

/// The codec name reported in [`RawError::HwDecoderUnavailable`].
const AV1: &str = "AV1";

/// Map a gamut container-parse error into a [`RawError`].
fn avif_err(context: &str, source: gamut_core::Error) -> RawError {
    RawError::Format(FormatError::ImageDecode {
        format: "AVIF",
        message: format!("{context}: {source}"),
    })
}

/// The [`RawError::HwDecoderUnavailable`] for AV1 with `reason`.
fn hw_unavailable(reason: &str) -> RawError {
    RawError::HwDecoderUnavailable {
        codec: AV1,
        reason: reason.to_string(),
    }
}

/// Classification of an auxiliary/derived image inside an AVIF file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AvifAuxKind {
    /// A scaled-down preview thumbnail (a `thmb` reference to the primary).
    Thumbnail,
    /// The alpha-plane auxiliary (the MIAF/CICP alpha URN). Note the alpha is
    /// already merged by [`AvifFile::decode_primary`]'s pipeline before the
    /// RGB conversion drops it; decode this item directly (via
    /// [`AvifFile::decode_aux`]) to obtain the plane itself as gray RGB.
    Alpha,
    /// A depth or disparity map.
    DepthMap,
    /// Any other auxiliary image (unrecognised URN, …).
    Auxiliary,
}

/// Descriptor of one auxiliary image inside an [`AvifFile`].
///
/// Decode it with [`AvifFile::decode_aux`].
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AvifAuxImage {
    /// What kind of auxiliary image this is.
    pub kind: AvifAuxKind,
    /// Width in pixels (0 when the item carries no `ispe` property).
    pub width: u32,
    /// Height in pixels (0 when the item carries no `ispe` property).
    pub height: u32,
    /// Auxiliary type URN string, when the container provides one.
    pub aux_type: Option<String>,
    /// Container item id used to decode this image.
    pub(crate) item_id: u32,
}

/// A parsed AVIF file.
///
/// Construct with [`AvifFile::open`]. Opening only parses the container
/// (cheap) — the auxiliary-image table and all metadata are available with no
/// hardware backend. The CPU/hardware-heavy AV1 decode happens in
/// [`decode_primary`](Self::decode_primary) and
/// [`decode_aux`](Self::decode_aux), and requires a hardware AV1 decoder
/// (the `hw` feature + a platform backend; see [`avif_hw_decode_available`]).
#[derive(Debug, Clone)]
pub struct AvifFile {
    image: AvifImage,
    aux: Vec<AvifAuxImage>,
}

impl AvifFile {
    /// Parse an AVIF file from its raw bytes.
    ///
    /// Only the container is parsed here, so this is cheap. The
    /// auxiliary-image table is populated up front; pixel decoding is
    /// deferred.
    ///
    /// # Errors
    /// Returns [`RawError::Format`] if `data` is not a readable AVIF file.
    pub fn open(data: Vec<u8>) -> RawResult<Self> {
        let container = AvifContainer::parse(&data)
            .map_err(|e| avif_err("failed to parse AVIF container", e))?;
        let image = container.image().clone();
        let aux = enumerate_aux(&image);
        Ok(Self { image, aux })
    }

    /// Decode the primary image to a 16-bit RGB image.
    ///
    /// Grid (tiled) stitching, overlay compositing, alpha merge, and the
    /// container's `clap`/`irot`/`imir` geometric transforms are applied by
    /// gamut-avif's decode pipeline; 8-bit output is scaled to the full
    /// 16-bit range.
    ///
    /// # Errors
    /// Returns [`RawError::HwDecoderUnavailable`] when no hardware AV1
    /// decoder is available (probe with [`avif_hw_decode_available`]), and
    /// [`RawError::Format`] for a malformed or unsupported image — including
    /// 10/12-bit content, whose RGBA presentation is pending upstream
    /// (visualcommons/gamut#303; see the module docs).
    pub fn decode_primary(&self) -> RawResult<RgbImage> {
        self.decode_item(self.image.primary_item().id())
    }

    /// Extract embedded EXIF/XMP/ICC and full typed metadata.
    ///
    /// Works with no hardware backend.
    pub fn metadata(&self) -> ImageMetadata {
        metadata_from_image(&self.image)
    }

    /// All auxiliary images (thumbnails, alpha plane, depth maps, other
    /// auxiliaries). Enumerated at [`open`](Self::open) time with no hardware
    /// backend.
    pub fn aux_images(&self) -> &[AvifAuxImage] {
        &self.aux
    }

    /// Decode a specific auxiliary image to a 16-bit RGB image.
    ///
    /// Single-channel sources (alpha planes, depth maps) are returned as
    /// grayscale-expanded RGB.
    ///
    /// # Errors
    /// As [`decode_primary`](Self::decode_primary).
    pub fn decode_aux(&self, aux: &AvifAuxImage) -> RawResult<RgbImage> {
        self.decode_item(aux.item_id)
    }

    /// Decode the embedded thumbnail, if the file carries one.
    ///
    /// Returns `Ok(None)` when there is no thumbnail.
    ///
    /// # Errors
    /// As [`decode_primary`](Self::decode_primary).
    pub fn thumbnail(&self) -> RawResult<Option<RgbImage>> {
        match self.aux.iter().find(|a| a.kind == AvifAuxKind::Thumbnail) {
            Some(thumb) => Ok(Some(self.decode_aux(thumb)?)),
            None => Ok(None),
        }
    }

    /// Decode one item through the hardware AV1 decoder.
    #[cfg(feature = "hw")]
    fn decode_item(&self, id: u32) -> RawResult<RgbImage> {
        let Some(decoder) = rawshift_hwdec::decoder(rawshift_hwdec::HwCodec::Av1) else {
            return Err(hw_unavailable(
                "no hardware AV1 decode backend is compiled in or usable at runtime \
                 on this target (see docs/SUPPORT.md)",
            ));
        };
        let mut adapter = hw::HwAv1Adapter::new(decoder);
        match self.image.decode_item_rgba8(id, &mut adapter) {
            Ok(rgba) => super::hw_planes::rgba8_to_rgb_image(&rgba),
            Err(source) => Err(adapter.into_raw_error(source)),
        }
    }

    /// Without the `hw` feature there is no decoder to route to: pixel decode
    /// is honestly unavailable (container/metadata paths above still work).
    #[cfg(not(feature = "hw"))]
    fn decode_item(&self, _id: u32) -> RawResult<RgbImage> {
        Err(hw_unavailable(
            "rawshift was built without the `hw` feature; AVIF pixel decode requires \
             a hardware AV1 decoder (rawshift-hwdec)",
        ))
    }
}

/// Whether AVIF pixel decode can work in this build on this machine: a
/// hardware AV1 decoder is compiled in (`hw`/`hw-*` feature) **and** usable
/// at runtime.
///
/// Container parsing, metadata, and auxiliary-image enumeration do not depend
/// on this — they always work.
#[must_use]
pub fn avif_hw_decode_available() -> bool {
    #[cfg(feature = "hw")]
    {
        rawshift_hwdec::decoder(rawshift_hwdec::HwCodec::Av1).is_some()
    }
    #[cfg(not(feature = "hw"))]
    {
        false
    }
}

/// Extract unified metadata from AVIF file bytes.
///
/// Reads the embedded EXIF, XMP, and ICC profile and maps them onto
/// [`ImageMetadata`]. Returns a default (empty) value when the file carries no
/// metadata or cannot be parsed. Used by both [`AvifFile::metadata`] and
/// [`read_standard_image_metadata`](crate::formats::read_standard_image_metadata).
pub fn read_avif_metadata(data: &[u8]) -> ImageMetadata {
    match AvifContainer::parse(data) {
        Ok(container) => metadata_from_image(container.image()),
        Err(_) => ImageMetadata::default(),
    }
}

// ── enumeration ──────────────────────────────────────────────────────────────

/// The `aux_type` URNs AVIF v1.2.0 §4 defines for its two standard auxiliary
/// roles (the MIAF/CICP URNs; AVIF defines no format-specific aliases the way
/// HEVC does).
const ALPHA_AUX_URN: &str = "urn:mpeg:mpegB:cicp:systems:auxiliary:alpha";
const DEPTH_AUX_URN: &str = "urn:mpeg:mpegB:cicp:systems:auxiliary:depth";

/// Classify an auxiliary image from its `auxC` type URN.
fn classify_aux(aux_type: Option<&str>) -> AvifAuxKind {
    match aux_type {
        Some(ALPHA_AUX_URN) => AvifAuxKind::Alpha,
        Some(DEPTH_AUX_URN) => AvifAuxKind::DepthMap,
        Some(t) => {
            let l = t.to_ascii_lowercase();
            if l.contains("depth") || l.contains("disparity") {
                AvifAuxKind::DepthMap
            } else {
                AvifAuxKind::Auxiliary
            }
        }
        None => AvifAuxKind::Auxiliary,
    }
}

/// The stored dimensions of an item, `(0, 0)` when it has no `ispe`.
fn item_dims(item: &AvifItem<'_>) -> (u32, u32) {
    item.dimensions()
        .map(|d| (d.width, d.height))
        .unwrap_or((0, 0))
}

/// Enumerate the thumbnails and auxiliary images referenced by the primary
/// image — pure container work, no codestream decode.
fn enumerate_aux(image: &AvifImage) -> Vec<AvifAuxImage> {
    let primary = image.primary_item().id();
    let mut out = Vec::new();

    for thumb in image.thumbnails_of(primary) {
        let (width, height) = item_dims(&thumb);
        out.push(AvifAuxImage {
            kind: AvifAuxKind::Thumbnail,
            width,
            height,
            aux_type: None,
            item_id: thumb.id(),
        });
    }

    for aux in image.auxiliaries_of(primary) {
        let aux_type = aux.auxiliary_type().filter(|s| !s.is_empty());
        let (width, height) = item_dims(&aux);
        out.push(AvifAuxImage {
            kind: classify_aux(aux_type),
            width,
            height,
            aux_type: aux_type.map(str::to_owned),
            item_id: aux.id(),
        });
    }

    out
}

// ── metadata ─────────────────────────────────────────────────────────────────

/// The HEIF/AVIF `Exif` item payload starts with a 4-byte big-endian
/// `exif_tiff_header_offset` pointing at the TIFF header (ISO/IEC 23008-12
/// §A.2.1). Strip it so the result is a clean TIFF byte stream.
fn strip_exif_prefix(raw: &[u8]) -> Option<&[u8]> {
    if raw.len() < 4 {
        return None;
    }
    let offset = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    let start = 4usize.checked_add(offset)?;
    if start >= raw.len() {
        return None;
    }
    Some(&raw[start..])
}

/// The bit depth of the primary item: the `pixi` property when present, else
/// the `av1C` record's coded bit depth, else 8.
fn primary_bit_depth(image: &AvifImage) -> u8 {
    let primary = image.primary_item();
    if let Some(bits) = primary.bits_per_channel()
        && let Some(&first) = bits.first()
    {
        return first;
    }
    if let Some(Ok(config)) = primary.av1_config() {
        return config.bit_depth();
    }
    8
}

/// Map a parsed AVIF image onto [`ImageMetadata`] — EXIF, XMP, ICC, and the
/// generic-table container facts.
fn metadata_from_image(image: &AvifImage) -> ImageMetadata {
    let primary = image.primary_item();

    // Parse the EXIF block (a raw TIFF stream) into typed + generic fields.
    let exif_tiff: Option<Vec<u8>> = image
        .exif()
        .and_then(|item| strip_exif_prefix(&item.as_isobmff_item().payload).map(<[u8]>::to_vec));
    let mut md = match exif_tiff {
        Some(ref exif) => ExifParser::parse_exif_blob(exif),
        None => ImageMetadata::default(),
    };

    md.exif_raw = exif_tiff;
    md.xmp = image
        .xmp()
        .map(|item| item.as_isobmff_item().payload.clone());
    md.icc_profile = primary.colour().and_then(|colour| match colour {
        ColourInformation::RestrictedIcc(bytes) | ColourInformation::UnrestrictedIcc(bytes) => {
            Some(bytes.clone())
        }
        _ => None,
    });

    let bit_depth = primary_bit_depth(image);
    if md.image.bit_depth == 0 {
        md.image.bit_depth = bit_depth;
    }

    // Record AVIF container facts in the generic table.
    let (width, height) = item_dims(&primary);
    let has_alpha = image.alpha_auxiliary_of(primary.id()).is_some();
    md.insert(
        MetadataKey::new(MetadataNamespace::Avif, "bit_depth"),
        MetadataValue::U64(bit_depth as u64),
    );
    md.insert(
        MetadataKey::new(MetadataNamespace::Avif, "has_alpha"),
        MetadataValue::U64(has_alpha as u64),
    );
    md.insert(
        MetadataKey::new(MetadataNamespace::Avif, "width"),
        MetadataValue::U64(width as u64),
    );
    md.insert(
        MetadataKey::new(MetadataNamespace::Avif, "height"),
        MetadataValue::U64(height as u64),
    );

    md
}

// ── rawshift-hwdec adapter ───────────────────────────────────────────────────

/// The adapter that plugs a [`rawshift_hwdec::HwStillDecoder`] into
/// gamut-avif's [`Av1StillDecoder`](gamut_avif::Av1StillDecoder) seam.
///
/// gamut-avif drives the whole decode pipeline (derivation, grid tiles,
/// colour, alpha, transforms) and calls
/// [`decode_still`](gamut_avif::Av1StillDecoder::decode_still) once per coded
/// picture; the adapter lowers each call to a
/// [`StillDecodeRequest`](rawshift_hwdec::StillDecodeRequest) — `av1C` config
/// bytes + the raw OBU item payload, exactly the shape hardware still-decode
/// APIs consume — and lifts the returned surface back into gamut-avif's
/// planar frame. This module contains **no** platform code — that lives
/// exclusively in rawshift-hwdec.
#[cfg(feature = "hw")]
mod hw {
    use gamut_avif::{Av1Config, Av1StillDecoder, ChromaFormat, DecodedFrame};
    use gamut_core::{Error as GamutError, Result as GamutResult};
    use rawshift_hwdec::{
        ChromaSubsampling, CodecConfig, DecodedFrame as HwFrame, HwDecodeError, HwStillDecoder,
        PixelFormat, StillDecodeRequest,
    };

    use super::super::hw_planes::{deinterleave_8, deinterleave_16, read_plane_8, read_plane_16};
    use crate::error::{FormatError, RawError};

    /// An [`Av1StillDecoder`] over a hardware [`HwStillDecoder`].
    ///
    /// gamut errors carry only `&'static str`, so the adapter records the
    /// last rich [`HwDecodeError`] and [`into_raw_error`](Self::into_raw_error)
    /// recovers it after the pipeline returns.
    pub(super) struct HwAv1Adapter {
        inner: Box<dyn HwStillDecoder>,
        last_error: Option<HwDecodeError>,
    }

    impl HwAv1Adapter {
        pub(super) fn new(inner: Box<dyn HwStillDecoder>) -> Self {
            Self {
                inner,
                last_error: None,
            }
        }

        /// Map the pipeline's failure to a [`RawError`], preferring the rich
        /// hardware error captured during the failing `decode_still` call.
        pub(super) fn into_raw_error(self, source: GamutError) -> RawError {
            match self.last_error {
                Some(HwDecodeError::Unavailable { reason, .. }) => RawError::HwDecoderUnavailable {
                    codec: super::AV1,
                    reason,
                },
                Some(other) => RawError::Format(FormatError::ImageDecode {
                    format: "AVIF",
                    message: other.to_string(),
                }),
                None => RawError::Format(FormatError::ImageDecode {
                    format: "AVIF",
                    message: source.to_string(),
                }),
            }
        }
    }

    impl Av1StillDecoder for HwAv1Adapter {
        fn decode_still(
            &mut self,
            config: &Av1Config,
            payload: &[u8],
        ) -> GamutResult<DecodedFrame> {
            let av1c = av1c_bytes(config);
            let request = StillDecodeRequest {
                config: CodecConfig::Av1c(&av1c),
                payload,
                // av1C carries no picture size; the coded bitstream (sequence
                // header) is authoritative and the backend reports the real
                // geometry.
                width: 0,
                height: 0,
                bit_depth: config.bit_depth(),
                chroma: chroma_subsampling(config.chroma_format()),
            };
            match self.inner.decode_still(&request) {
                Ok(frame) => hw_frame_to_decoded(&frame, config.chroma_format()),
                Err(error) => {
                    let mapped = match &error {
                        HwDecodeError::Unavailable { .. } => {
                            GamutError::Unsupported("AVIF: hardware AV1 decoder unavailable")
                        }
                        _ => GamutError::InvalidInput("AVIF: hardware AV1 decode failed"),
                    };
                    self.last_error = Some(error);
                    Err(mapped)
                }
            }
        }
    }

    /// gamut-avif's chroma format as rawshift-hwdec's advisory subsampling.
    fn chroma_subsampling(chroma: ChromaFormat) -> ChromaSubsampling {
        match chroma {
            ChromaFormat::Monochrome => ChromaSubsampling::Cs400,
            ChromaFormat::Yuv420 => ChromaSubsampling::Cs420,
            ChromaFormat::Yuv422 => ChromaSubsampling::Cs422,
            ChromaFormat::Yuv444 => ChromaSubsampling::Cs444,
        }
    }

    /// Re-serialize a typed [`Av1Config`] to `av1C` record bytes (AV1-ISOBMFF
    /// v1.3.0 §2.3.3) for the hardware seam, which transports the raw record.
    /// Reserved bits are written as zero; gamut's parser masks them away, so
    /// `parse(av1c_bytes(c)) == c`. Infallible: every field is a bit-field
    /// that fits its slot by construction, and `configOBUs` are copied
    /// verbatim.
    ///
    /// gamut-avif is deliberately decode-only for the container read surface,
    /// so it exposes no serializer; this writer is rawshift-owned FFI glue,
    /// not duplicated upstream logic.
    pub(super) fn av1c_bytes(config: &Av1Config) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + config.config_obus.len());
        // marker(1) = 1 | version(7) = 1
        out.push(0x81);
        out.push((config.seq_profile << 5) | (config.seq_level_idx_0 & 0x1f));
        out.push(
            (config.seq_tier_0 << 7)
                | (u8::from(config.high_bitdepth) << 6)
                | (u8::from(config.twelve_bit) << 5)
                | (u8::from(config.monochrome) << 4)
                | ((config.chroma_subsampling_x & 0x01) << 3)
                | ((config.chroma_subsampling_y & 0x01) << 2)
                | (config.chroma_sample_position & 0x03),
        );
        out.push(match config.initial_presentation_delay_minus_one {
            Some(delay) => 0x10 | (delay & 0x0f),
            None => 0,
        });
        out.extend_from_slice(&config.config_obus);
        out
    }

    /// Lift a hardware surface ([`HwFrame`]: NV12/P010/I420/I010, byte planes
    /// with strides) into gamut-avif's uniform-`u16` planar [`DecodedFrame`].
    ///
    /// All four surface formats are 4:2:0 containers, but a **monochrome**
    /// coded stream (`av1C` `monochrome` — e.g. an alpha or depth auxiliary)
    /// still comes back from hardware in one of them, with neutral chroma
    /// planes. `coded_chroma` (from the `av1C`) decides: monochrome streams
    /// drop the surface's chroma planes and present as
    /// [`ChromaFormat::Monochrome`]; 4:2:0 streams present as
    /// [`ChromaFormat::Yuv420`]. A 4:2:2/4:4:4 coded stream cannot be carried
    /// by a 4:2:0 surface, so it is rejected honestly (today's hardware
    /// backends only decode AV1 Profile 0, which cannot code it either).
    /// `DecodedFrame::new` revalidates the geometry.
    pub(super) fn hw_frame_to_decoded(
        frame: &HwFrame,
        coded_chroma: ChromaFormat,
    ) -> GamutResult<DecodedFrame> {
        let (w, h) = (frame.width(), frame.height());
        let bit_depth = frame.bit_depth();
        let (cw, ch) = (w.div_ceil(2) as usize, h.div_ceil(2) as usize);
        let planes = frame.planes();

        match coded_chroma {
            ChromaFormat::Monochrome => {
                let y = match frame.format() {
                    PixelFormat::I420 | PixelFormat::Nv12 => {
                        read_plane_8(&planes[0], w as usize, h as usize)
                    }
                    PixelFormat::I010 => {
                        read_plane_16(&planes[0], w as usize, h as usize, 0, bit_depth)
                    }
                    PixelFormat::P010 => read_plane_16(
                        &planes[0],
                        w as usize,
                        h as usize,
                        16 - u32::from(bit_depth),
                        bit_depth,
                    ),
                };
                DecodedFrame::new(w, h, bit_depth, ChromaFormat::Monochrome, y, vec![], vec![])
            }
            ChromaFormat::Yuv420 => {
                let (y, cb, cr) = match frame.format() {
                    PixelFormat::I420 => (
                        read_plane_8(&planes[0], w as usize, h as usize),
                        read_plane_8(&planes[1], cw, ch),
                        read_plane_8(&planes[2], cw, ch),
                    ),
                    PixelFormat::Nv12 => {
                        let (cb, cr) = deinterleave_8(&planes[1], cw, ch);
                        (read_plane_8(&planes[0], w as usize, h as usize), cb, cr)
                    }
                    PixelFormat::I010 => (
                        read_plane_16(&planes[0], w as usize, h as usize, 0, bit_depth),
                        read_plane_16(&planes[1], cw, ch, 0, bit_depth),
                        read_plane_16(&planes[2], cw, ch, 0, bit_depth),
                    ),
                    PixelFormat::P010 => {
                        // P010 stores the value in the most significant bits.
                        let shift = 16 - u32::from(bit_depth);
                        let (cb, cr) = deinterleave_16(&planes[1], cw, ch, shift, bit_depth);
                        (
                            read_plane_16(&planes[0], w as usize, h as usize, shift, bit_depth),
                            cb,
                            cr,
                        )
                    }
                };
                DecodedFrame::new(w, h, bit_depth, ChromaFormat::Yuv420, y, cb, cr)
            }
            ChromaFormat::Yuv422 | ChromaFormat::Yuv444 => Err(GamutError::Unsupported(
                "AVIF: the hardware surface is 4:2:0; 4:2:2/4:4:4 coded content is outside \
                 the hardware decode scope (AV1 Profile 0 only)",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamut_isobmff::{IsoBmffImage, Item, ItemReference, Property, PropertyKind, write};

    /// A minimal `av1C`: marker+version, profile 0 / level 0, 8-bit 4:2:0.
    fn av1c_420() -> Vec<u8> {
        vec![0x81, 0x00, 0x0C, 0x00]
    }

    /// As [`av1c_420`] but monochrome (subsampling stays `(1, 1)`, as the
    /// spec requires) — the shape of an alpha/depth auxiliary's config.
    fn av1c_mono() -> Vec<u8> {
        vec![0x81, 0x00, 0x1C, 0x00]
    }

    /// A conforming still payload: a sequence header OBU with
    /// `reduced_still_picture_header = 1`, then a frame OBU. (The frame body
    /// is not a decodable codestream — the backend-less paths never decode
    /// it, and the honest-error test asserts a real decoder rejects it.)
    fn still_payload() -> Vec<u8> {
        vec![0x0A, 0x01, 0x18, 0x32, 0x03, 0xAA, 0xBB, 0xCC]
    }

    /// A coded av01 item: av1C config + a conforming still payload.
    fn coded_item(id: u32, width: u32, height: u32) -> Item {
        Item {
            id,
            item_type: *b"av01",
            name: String::new(),
            content_type: None,
            content_encoding: None,
            hidden: false,
            references: vec![],
            properties: vec![
                Property {
                    essential: true,
                    kind: PropertyKind::CodecConfiguration {
                        kind: *b"av1C",
                        data: av1c_420(),
                    },
                },
                Property {
                    essential: false,
                    kind: PropertyKind::ImageSpatialExtents { width, height },
                },
            ],
            payload: still_payload(),
        }
    }

    /// A synthetic AVIF: primary image + thumbnail + alpha + depth
    /// auxiliaries + EXIF + XMP — everything the backend-less
    /// enumeration/metadata paths must see.
    fn synthetic_avif() -> Vec<u8> {
        let mut primary = coded_item(1, 2, 2);

        let mut thumb = coded_item(2, 1, 1);
        thumb.references.push(ItemReference {
            reference_type: *b"thmb",
            to_item_ids: vec![1],
        });

        let aux = |id: u32, urn: &str| {
            let mut item = coded_item(id, 2, 2);
            item.references.push(ItemReference {
                reference_type: *b"auxl",
                to_item_ids: vec![1],
            });
            item.properties.push(Property {
                essential: false,
                kind: PropertyKind::AuxiliaryType {
                    aux_type: urn.to_string(),
                    aux_subtype: vec![],
                },
            });
            item
        };
        let mut alpha = aux(3, ALPHA_AUX_URN);
        // An alpha plane is coded monochrome.
        alpha.properties[0] = Property {
            essential: true,
            kind: PropertyKind::CodecConfiguration {
                kind: *b"av1C",
                data: av1c_mono(),
            },
        };
        let depth = aux(4, DEPTH_AUX_URN);

        // EXIF item: 4-byte exif_tiff_header_offset (0) + a TIFF header stub.
        let exif = Item {
            id: 5,
            item_type: *b"Exif",
            name: String::new(),
            content_type: None,
            content_encoding: None,
            hidden: false,
            references: vec![ItemReference {
                reference_type: *b"cdsc",
                to_item_ids: vec![1],
            }],
            properties: vec![],
            payload: [&[0u8, 0, 0, 0][..], &tiff_stub()[..]].concat(),
        };
        let xmp = Item {
            id: 6,
            item_type: *b"mime",
            name: String::new(),
            content_type: Some("application/rdf+xml".to_string()),
            content_encoding: None,
            hidden: false,
            references: vec![ItemReference {
                reference_type: *b"cdsc",
                to_item_ids: vec![1],
            }],
            properties: vec![],
            payload: b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>".to_vec(),
        };

        // `pixi`: 3 channels of 8 bits.
        primary.properties.push(Property {
            essential: false,
            kind: PropertyKind::PixelInformation {
                bits_per_channel: vec![8, 8, 8],
            },
        });

        let image = IsoBmffImage {
            major_brand: *b"avif",
            minor_version: 0,
            compatible_brands: vec![*b"avif", *b"mif1", *b"miaf"],
            primary_item_id: 1,
            items: vec![primary, thumb, alpha, depth, exif, xmp],
            groups: vec![],
        };
        write(&image).expect("write synthetic AVIF")
    }

    /// A minimal little-endian TIFF stream: header + one empty IFD.
    fn tiff_stub() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"II");
        v.extend_from_slice(&42u16.to_le_bytes());
        v.extend_from_slice(&8u32.to_le_bytes()); // first IFD at offset 8
        v.extend_from_slice(&0u16.to_le_bytes()); // 0 entries
        v.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
        v
    }

    #[test]
    fn open_rejects_junk() {
        let junk = vec![0u8; 64];
        assert!(AvifFile::open(junk).is_err());
    }

    #[test]
    fn read_metadata_junk_returns_default() {
        let junk = vec![0u8; 64];
        assert_eq!(read_avif_metadata(&junk), ImageMetadata::default());
    }

    #[test]
    fn classify_aux_recognises_urns() {
        assert_eq!(classify_aux(Some(ALPHA_AUX_URN)), AvifAuxKind::Alpha);
        assert_eq!(classify_aux(Some(DEPTH_AUX_URN)), AvifAuxKind::DepthMap);
        assert_eq!(
            classify_aux(Some("urn:vendor:aux:disparity-map")),
            AvifAuxKind::DepthMap
        );
        assert_eq!(
            classify_aux(Some("urn:vendor:aux:something")),
            AvifAuxKind::Auxiliary
        );
        assert_eq!(classify_aux(None), AvifAuxKind::Auxiliary);
    }

    #[test]
    fn strip_exif_prefix_handles_offsets() {
        // Offset 0: TIFF stream starts immediately after the prefix.
        let raw = [0, 0, 0, 0, b'I', b'I', 0x2A, 0x00];
        assert_eq!(strip_exif_prefix(&raw), Some(&raw[4..]));
        // Offset 2: skip prefix + 2 pad bytes.
        let raw = [0, 0, 0, 2, 0xAA, 0xBB, b'M', b'M'];
        assert_eq!(strip_exif_prefix(&raw), Some(&raw[6..]));
        // Short / out-of-bounds inputs are rejected.
        assert_eq!(strip_exif_prefix(&[0, 0]), None);
        assert_eq!(strip_exif_prefix(&[0, 0, 0, 99, 1, 2]), None);
    }

    #[test]
    fn synthetic_container_enumerates_backend_less() {
        let file = AvifFile::open(synthetic_avif()).expect("open synthetic AVIF");
        let aux = file.aux_images();
        assert_eq!(aux.len(), 3, "thumbnail + alpha + depth");

        let kind = |k: AvifAuxKind| aux.iter().filter(|a| a.kind == k).count();
        assert_eq!(kind(AvifAuxKind::Thumbnail), 1);
        assert_eq!(kind(AvifAuxKind::Alpha), 1);
        assert_eq!(kind(AvifAuxKind::DepthMap), 1);

        let thumb = aux
            .iter()
            .find(|a| a.kind == AvifAuxKind::Thumbnail)
            .unwrap();
        assert_eq!((thumb.width, thumb.height), (1, 1));
        let alpha = aux.iter().find(|a| a.kind == AvifAuxKind::Alpha).unwrap();
        assert_eq!(alpha.aux_type.as_deref(), Some(ALPHA_AUX_URN));
    }

    #[test]
    fn synthetic_container_metadata_backend_less() {
        let file = AvifFile::open(synthetic_avif()).expect("open synthetic AVIF");
        let md = file.metadata();
        assert_eq!(md.exif_raw.as_deref(), Some(&tiff_stub()[..]));
        assert!(
            md.xmp
                .as_deref()
                .is_some_and(|x| x.starts_with(b"<x:xmpmeta"))
        );
        assert_eq!(md.image.bit_depth, 8);
        assert_eq!(
            md.get(MetadataNamespace::Avif, "width"),
            Some(&MetadataValue::U64(2))
        );
        assert_eq!(
            md.get(MetadataNamespace::Avif, "height"),
            Some(&MetadataValue::U64(2))
        );
        assert_eq!(
            md.get(MetadataNamespace::Avif, "has_alpha"),
            Some(&MetadataValue::U64(1))
        );
    }

    /// Pixel decode of the synthetic container is honest about the build and
    /// machine: without a usable hardware backend it reports the matchable
    /// `HwDecoderUnavailable`; with one (VAAPI, #29) the synthetic stream —
    /// whose frame OBU is not a real codestream — is rejected by the real
    /// decoder as malformed, **not** as "unavailable". Enumeration/metadata
    /// keep working either way.
    #[test]
    fn pixel_decode_reports_hw_decoder_unavailable() {
        let file = AvifFile::open(synthetic_avif()).expect("open synthetic AVIF");
        let err = file.decode_primary().unwrap_err();
        if avif_hw_decode_available() {
            assert!(
                matches!(err, RawError::Format(_)),
                "a real decoder must reject the synthetic stream as malformed, got: {err}"
            );
            return;
        }
        assert!(
            matches!(err, RawError::HwDecoderUnavailable { codec: "AV1", .. }),
            "expected HwDecoderUnavailable, got: {err}"
        );
        let thumb = file
            .aux_images()
            .iter()
            .find(|a| a.kind == AvifAuxKind::Thumbnail)
            .unwrap()
            .clone();
        let err = file.decode_aux(&thumb).unwrap_err();
        assert!(matches!(err, RawError::HwDecoderUnavailable { .. }));
    }

    // ── hardware adapter (compiled with the `hw` feature; exercised with a
    //    synthetic in-process decoder so #29's real backend plugs into an
    //    already-tested path) ─────────────────────────────────────────────────

    #[cfg(feature = "hw")]
    mod hw_adapter {
        use super::super::hw::{HwAv1Adapter, av1c_bytes, hw_frame_to_decoded};
        use super::*;
        use gamut_avif::{Av1Config, ChromaFormat};
        use gamut_color::{ColorRange, ycbcr_to_rgb};
        use rawshift_hwdec::{
            ChromaSubsampling, CodecConfig, DecodedFrame as HwFrame, HwDecodeError, HwStillDecoder,
            PixelFormat, Plane, StillDecodeRequest,
        };

        #[test]
        fn av1c_round_trips_through_the_serializer() {
            // Header-only records: 8-bit 4:2:0 and monochrome.
            for raw in [av1c_420(), av1c_mono()] {
                let original = Av1Config::parse(&raw).unwrap();
                assert_eq!(av1c_bytes(&original), raw);
            }

            // Distinct, non-zero values in every field the writer packs: a
            // 4:2:2 12-bit professional-profile record with a presentation
            // delay and configOBUs (one empty temporal delimiter), so a
            // swapped shift or mask breaks the round trip.
            let raw = vec![
                0xFF, // marker 1, version... (0x7f) — not version 1!
            ];
            assert!(Av1Config::parse(&raw).is_err());
            let raw = vec![
                0x81,        // marker + version 1
                0b0101_0011, // profile 2, level 0x13
                0b1110_1000, // tier 1, hbd, 12-bit, colour, subsampling (1,0) = 4:2:2
                0x10 | 0x07, // presentation delay present, minus_one = 7
                0x12,
                0x00, // configOBUs: temporal delimiter with size 0
            ];
            let original = Av1Config::parse(&raw).unwrap();
            assert_eq!(original.bit_depth(), 12);
            assert_eq!(original.chroma_format(), ChromaFormat::Yuv422);
            let bytes = av1c_bytes(&original);
            assert_eq!(Av1Config::parse(&bytes).unwrap(), original);
        }

        fn hw_frame_nv12(width: u32, height: u32, y: u8, cb: u8, cr: u8) -> HwFrame {
            let (cw, ch) = (width.div_ceil(2) as usize, height.div_ceil(2) as usize);
            let mut cbcr = Vec::with_capacity(cw * ch * 2);
            for _ in 0..cw * ch {
                cbcr.extend_from_slice(&[cb, cr]);
            }
            HwFrame::new(
                PixelFormat::Nv12,
                width,
                height,
                8,
                gamut_color::ColorRange::Limited,
                vec![
                    Plane {
                        data: vec![y; (width * height) as usize],
                        stride: width as usize,
                    },
                    Plane {
                        data: cbcr,
                        stride: cw * 2,
                    },
                ],
            )
            .expect("valid NV12 frame")
        }

        #[test]
        fn nv12_and_i420_lift_to_the_same_planar_frame() {
            let nv12 = hw_frame_nv12(2, 2, 81, 90, 240);
            let frame = hw_frame_to_decoded(&nv12, ChromaFormat::Yuv420).unwrap();
            assert_eq!(frame.y(), &[81u16; 4]);
            assert_eq!(frame.cb(), &[90]);
            assert_eq!(frame.cr(), &[240]);

            let i420 = HwFrame::new(
                PixelFormat::I420,
                2,
                2,
                8,
                ColorRange::Limited,
                vec![
                    Plane {
                        data: vec![81; 4],
                        stride: 2,
                    },
                    Plane {
                        data: vec![90],
                        stride: 1,
                    },
                    Plane {
                        data: vec![240],
                        stride: 1,
                    },
                ],
            )
            .unwrap();
            assert_eq!(
                hw_frame_to_decoded(&i420, ChromaFormat::Yuv420).unwrap(),
                frame
            );

            // A monochrome coded stream (alpha/depth) drops the surface's
            // neutral chroma planes.
            let mono = hw_frame_to_decoded(&nv12, ChromaFormat::Monochrome).unwrap();
            assert_eq!(mono.chroma(), ChromaFormat::Monochrome);
            assert_eq!(mono.y(), &[81u16; 4]);
            assert!(mono.cb().is_empty() && mono.cr().is_empty());

            // A 4:4:4 coded stream cannot be carried by a 4:2:0 surface.
            assert!(hw_frame_to_decoded(&nv12, ChromaFormat::Yuv444).is_err());
        }

        #[test]
        fn p010_lifts_with_correct_bit_positions() {
            // 10-bit value 612: P010 stores it << 6.
            let value: u16 = 612;
            let word = (value << 6).to_le_bytes();
            let p010 = HwFrame::new(
                PixelFormat::P010,
                2,
                1,
                10,
                ColorRange::Limited,
                vec![
                    Plane {
                        data: [word, word].concat(),
                        stride: 4,
                    },
                    Plane {
                        data: [word, word].concat(), // one CbCr pair
                        stride: 4,
                    },
                ],
            )
            .unwrap();
            let frame = hw_frame_to_decoded(&p010, ChromaFormat::Yuv420).unwrap();
            assert_eq!(frame.y(), &[value, value]);
            assert_eq!((frame.cb(), frame.cr()), (&[value][..], &[value][..]));
            assert_eq!(frame.bit_depth(), 10);
        }

        /// A synthetic in-process "hardware" decoder: asserts the request the
        /// adapter lowers (av1C config variant round-tripping the record,
        /// payload passthrough) and returns a fixed NV12 frame per coded
        /// chroma — exactly the seam the VAAPI backend occupies.
        struct FakeAv1Decoder {
            y: u8,
            cb: u8,
            cr: u8,
            alpha: u8,
        }

        impl HwStillDecoder for FakeAv1Decoder {
            fn decode_still(
                &mut self,
                request: &StillDecodeRequest<'_>,
            ) -> Result<HwFrame, HwDecodeError> {
                assert_eq!(request.codec(), rawshift_hwdec::HwCodec::Av1);
                let CodecConfig::Av1c(av1c) = request.config else {
                    panic!("adapter must lower to an av1C config");
                };
                // The adapter hands the item payload through untouched (a raw
                // OBU stream) and the re-serialized av1C parses back.
                assert_eq!(request.payload, &still_payload()[..]);
                assert_eq!(request.bit_depth, 8);
                let config = Av1Config::parse(av1c).expect("valid av1C");
                if config.monochrome {
                    assert_eq!(request.chroma, ChromaSubsampling::Cs400);
                    // Alpha plane: hardware still hands back a 4:2:0 surface
                    // with neutral chroma.
                    Ok(hw_frame_nv12(2, 2, self.alpha, 128, 128))
                } else {
                    assert_eq!(request.chroma, ChromaSubsampling::Cs420);
                    Ok(hw_frame_nv12(2, 2, self.y, self.cb, self.cr))
                }
            }
        }

        #[test]
        fn synthetic_frames_decode_through_the_adapter_with_alpha_merge() {
            let (y, cb, cr, alpha) = (81u8, 90u8, 240u8, 0x40u8);
            let data = synthetic_avif();
            let container = gamut_avif::AvifContainer::parse(&data).unwrap();

            let mut adapter = HwAv1Adapter::new(Box::new(FakeAv1Decoder { y, cb, cr, alpha }));
            let rgba = container
                .decode_primary_rgba8(&mut adapter)
                .expect("decode through the adapter");

            // No `colr` in the synthetic container → gamut-avif's documented
            // BT.601 limited-range default, i.e. gamut-color's ycbcr module;
            // the alpha auxiliary's luma plane (8-bit) merges 1:1.
            let (r, g, b) = ycbcr_to_rgb(y, cb, cr, ColorRange::Limited);
            assert_eq!((rgba.width(), rgba.height()), (2, 2));
            for px in rgba.as_samples().chunks_exact(4) {
                assert_eq!(px, [r, g, b, alpha]);
            }
        }

        /// A backend that accepts and then fails must surface its rich error
        /// message through `into_raw_error`, not gamut's `&'static str`.
        #[test]
        fn adapter_failure_maps_to_rich_error() {
            struct Failing;
            impl HwStillDecoder for Failing {
                fn decode_still(
                    &mut self,
                    request: &StillDecodeRequest<'_>,
                ) -> Result<HwFrame, HwDecodeError> {
                    Err(HwDecodeError::Decode {
                        codec: request.codec(),
                        message: "driver rejected the session".to_string(),
                    })
                }
            }

            let data = synthetic_avif();
            let container = gamut_avif::AvifContainer::parse(&data).unwrap();
            let mut adapter = HwAv1Adapter::new(Box::new(Failing));
            let err = container.decode_primary_rgba8(&mut adapter).unwrap_err();
            let raw = adapter.into_raw_error(err);
            let text = raw.to_string();
            assert!(
                text.contains("driver rejected the session"),
                "rich backend message must survive: {text}"
            );
        }
    }
}
