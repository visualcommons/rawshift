//! HEIC/HEIF format support — public API on gamut-heic.
//!
//! [`HeicFile`] is the entry point for HEIC files. The container is parsed by
//! [`gamut_heic`], which owns everything around the coded picture: item roles,
//! grid/`iden`/`iovl` derivation, colour conversion, alpha merge, and the
//! `clap`/`irot`/`imir` transforms. The HEVC codestream itself is decoded
//! through gamut-heic's pluggable [`HevcDecoder`](gamut_heic::HevcDecoder)
//! seam, which rawshift plugs with a [`rawshift-hwdec`] hardware decoder when
//! the `hw` feature is enabled.
//!
//! **Container parsing, metadata, and auxiliary-image enumeration always work
//! with no hardware backend at all.** Only pixel decode
//! ([`decode_primary`](HeicFile::decode_primary) /
//! [`decode_aux`](HeicFile::decode_aux) / [`thumbnail`](HeicFile::thumbnail))
//! needs a hardware HEVC decoder; without one it fails with the matchable
//! [`RawError::HwDecoderUnavailable`]. Probe up front with
//! [`heic_hw_decode_available`].
//!
//! HEIC is also reachable through the generic standard-format API
//! ([`decode_standard_image`](crate::formats::decode_standard_image)); use
//! [`HeicFile`] when you need the auxiliary images or richer metadata.
//!
//! [`rawshift-hwdec`]: https://docs.rs/rawshift-hwdec

use gamut_heic::{HeifContainer, HeifImage, HeifItem};
use gamut_isobmff::ColourInformation;

use crate::core::RgbImage;
use crate::core::metadata::{ImageMetadata, MetadataKey, MetadataNamespace, MetadataValue};
use crate::error::{FormatError, RawError, RawResult};
use crate::metadata::exif::ExifParser;

/// The codec name reported in [`RawError::HwDecoderUnavailable`].
const HEVC: &str = "HEVC";

/// Map a gamut container-parse error into a [`RawError`].
fn heic_err(context: &str, source: gamut_core::Error) -> RawError {
    RawError::Format(FormatError::ImageDecode {
        format: "HEIC",
        message: format!("{context}: {source}"),
    })
}

/// The [`RawError::HwDecoderUnavailable`] for HEVC with `reason`.
fn hw_unavailable(reason: &str) -> RawError {
    RawError::HwDecoderUnavailable {
        codec: HEVC,
        reason: reason.to_string(),
    }
}

/// Classification of an auxiliary/derived image inside a HEIC file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HeicAuxKind {
    /// A scaled-down preview thumbnail (a `thmb` reference to the primary).
    Thumbnail,
    /// A depth or disparity map.
    DepthMap,
    /// An HDR gain map (e.g. Apple `urn:com:apple:photo:2020:aux:hdrgainmap`).
    GainMap,
    /// Any other auxiliary image (alpha mask, unrecognised URN, …).
    Auxiliary,
}

/// Descriptor of one auxiliary image inside a [`HeicFile`].
///
/// Decode it with [`HeicFile::decode_aux`].
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HeicAuxImage {
    /// What kind of auxiliary image this is.
    pub kind: HeicAuxKind,
    /// Width in pixels (0 when the item carries no `ispe` property).
    pub width: u32,
    /// Height in pixels (0 when the item carries no `ispe` property).
    pub height: u32,
    /// Auxiliary type URN string, when the container provides one.
    pub aux_type: Option<String>,
    /// Container item id used to decode this image.
    pub(crate) item_id: u32,
}

/// A parsed HEIC/HEIF file.
///
/// Construct with [`HeicFile::open`]. Opening only parses the container
/// (cheap) — the auxiliary-image table and all metadata are available with no
/// hardware backend. The CPU/hardware-heavy HEVC decode happens in
/// [`decode_primary`](Self::decode_primary) and
/// [`decode_aux`](Self::decode_aux), and requires a hardware HEVC decoder
/// (the `hw` feature + a platform backend; see [`heic_hw_decode_available`]).
#[derive(Debug, Clone)]
pub struct HeicFile {
    image: HeifImage,
    aux: Vec<HeicAuxImage>,
}

impl HeicFile {
    /// Parse a HEIC file from its raw bytes.
    ///
    /// Only the container is parsed here, so this is cheap. The
    /// auxiliary-image table is populated up front; pixel decoding is
    /// deferred.
    ///
    /// # Errors
    /// Returns [`RawError::Format`] if `data` is not a readable HEIC file.
    pub fn open(data: Vec<u8>) -> RawResult<Self> {
        let container = HeifContainer::parse(&data)
            .map_err(|e| heic_err("failed to parse HEIC container", e))?;
        let image = container.image().clone();
        let aux = enumerate_aux(&image);
        Ok(Self { image, aux })
    }

    /// Decode the primary image to a 16-bit RGB image.
    ///
    /// Grid (tiled) stitching, overlay compositing, and the container's
    /// `clap`/`irot`/`imir` geometric transforms are applied by gamut-heic's
    /// decode pipeline; 8-bit output is scaled to the full 16-bit range.
    ///
    /// # Errors
    /// Returns [`RawError::HwDecoderUnavailable`] when no hardware HEVC
    /// decoder is available (probe with [`heic_hw_decode_available`]), and
    /// [`RawError::Format`] for a malformed or unsupported image.
    pub fn decode_primary(&self) -> RawResult<RgbImage> {
        self.decode_item(self.image.primary_item().id())
    }

    /// Extract embedded EXIF/XMP/ICC and full typed metadata.
    ///
    /// Works with no hardware backend.
    pub fn metadata(&self) -> ImageMetadata {
        metadata_from_image(&self.image)
    }

    /// All auxiliary images (thumbnails, depth maps, HDR gain maps,
    /// alpha/auxiliary). Enumerated at [`open`](Self::open) time with no
    /// hardware backend.
    pub fn aux_images(&self) -> &[HeicAuxImage] {
        &self.aux
    }

    /// Decode a specific auxiliary image to a 16-bit RGB image.
    ///
    /// Single-channel sources (depth maps, gain maps) are returned as
    /// grayscale-expanded RGB.
    ///
    /// # Errors
    /// As [`decode_primary`](Self::decode_primary).
    pub fn decode_aux(&self, aux: &HeicAuxImage) -> RawResult<RgbImage> {
        self.decode_item(aux.item_id)
    }

    /// Decode the embedded thumbnail, if the file carries one.
    ///
    /// Returns `Ok(None)` when there is no thumbnail.
    ///
    /// # Errors
    /// As [`decode_primary`](Self::decode_primary).
    pub fn thumbnail(&self) -> RawResult<Option<RgbImage>> {
        match self.aux.iter().find(|a| a.kind == HeicAuxKind::Thumbnail) {
            Some(thumb) => Ok(Some(self.decode_aux(thumb)?)),
            None => Ok(None),
        }
    }

    /// Decode one item through the hardware HEVC decoder.
    #[cfg(feature = "hw")]
    fn decode_item(&self, id: u32) -> RawResult<RgbImage> {
        let Some(decoder) = rawshift_hwdec::decoder(rawshift_hwdec::HwCodec::Hevc) else {
            return Err(hw_unavailable(
                "no hardware HEVC decode backend is compiled in or usable at runtime \
                 on this target (see docs/SUPPORT.md)",
            ));
        };
        let mut adapter = hw::HwHevcAdapter::new(decoder);
        match self.image.decode_item_rgba8(id, &mut adapter) {
            Ok(rgba) => rgba8_to_rgb_image(&rgba),
            Err(source) => Err(adapter.into_raw_error(source)),
        }
    }

    /// Without the `hw` feature there is no decoder to route to: pixel decode
    /// is honestly unavailable (container/metadata paths above still work).
    #[cfg(not(feature = "hw"))]
    fn decode_item(&self, _id: u32) -> RawResult<RgbImage> {
        Err(hw_unavailable(
            "rawshift was built without the `hw` feature; HEIC pixel decode requires \
             a hardware HEVC decoder (rawshift-hwdec)",
        ))
    }
}

/// Whether HEIC pixel decode can work in this build on this machine: a
/// hardware HEVC decoder is compiled in (`hw`/`hw-*` feature) **and** usable
/// at runtime.
///
/// Container parsing, metadata, and auxiliary-image enumeration do not depend
/// on this — they always work.
#[must_use]
pub fn heic_hw_decode_available() -> bool {
    #[cfg(feature = "hw")]
    {
        rawshift_hwdec::decoder(rawshift_hwdec::HwCodec::Hevc).is_some()
    }
    #[cfg(not(feature = "hw"))]
    {
        false
    }
}

/// Extract unified metadata from HEIC file bytes.
///
/// Reads the embedded EXIF, XMP, and ICC profile and maps them onto
/// [`ImageMetadata`]. Returns a default (empty) value when the file carries no
/// metadata or cannot be parsed. Used by both [`HeicFile::metadata`] and
/// [`read_standard_image_metadata`](crate::formats::read_standard_image_metadata).
pub fn read_heic_metadata(data: &[u8]) -> ImageMetadata {
    match HeifContainer::parse(data) {
        Ok(container) => metadata_from_image(container.image()),
        Err(_) => ImageMetadata::default(),
    }
}

// ── enumeration ──────────────────────────────────────────────────────────────

/// The standard `aux_type` URNs marking a depth-map auxiliary (ISO/IEC
/// 23008-12 §6.5.8; MIAF §7.3.5 / CICP). Vendor URNs are caught by the
/// substring heuristics below.
const DEPTH_AUX_URNS: [&str; 2] = [
    "urn:mpeg:hevc:2015:auxid:2",
    "urn:mpeg:mpegB:cicp:systems:auxiliary:depth",
];

/// Classify an auxiliary image from its `auxC` type URN.
fn classify_aux(aux_type: Option<&str>) -> HeicAuxKind {
    match aux_type {
        Some(t) => {
            if DEPTH_AUX_URNS.contains(&t) {
                return HeicAuxKind::DepthMap;
            }
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

/// The stored dimensions of an item, `(0, 0)` when it has no `ispe`.
fn item_dims(item: &HeifItem<'_>) -> (u32, u32) {
    item.dimensions()
        .map(|d| (d.width, d.height))
        .unwrap_or((0, 0))
}

/// Enumerate the thumbnails and auxiliary images referenced by the primary
/// image — pure container work, no codestream decode.
fn enumerate_aux(image: &HeifImage) -> Vec<HeicAuxImage> {
    let primary = image.primary_item().id();
    let mut out = Vec::new();

    for thumb in image.thumbnails_of(primary) {
        let (width, height) = item_dims(&thumb);
        out.push(HeicAuxImage {
            kind: HeicAuxKind::Thumbnail,
            width,
            height,
            aux_type: None,
            item_id: thumb.id(),
        });
    }

    for aux in image.auxiliaries_of(primary) {
        let aux_type = aux.auxiliary_type().filter(|s| !s.is_empty());
        let (width, height) = item_dims(&aux);
        out.push(HeicAuxImage {
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

/// The HEIF `Exif` item payload starts with a 4-byte big-endian
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

/// The luma bit depth of the primary item: the `pixi` property when present,
/// else the `hvcC` luma depth, else 8.
fn primary_bit_depth(image: &HeifImage) -> u8 {
    let primary = image.primary_item();
    if let Some(bits) = primary.bits_per_channel()
        && let Some(&first) = bits.first()
    {
        return first;
    }
    if let Some(Ok(config)) = primary.hevc_config() {
        return config.bit_depth_luma();
    }
    8
}

/// Map a parsed HEIF image onto [`ImageMetadata`] — EXIF, XMP, ICC, and the
/// generic-table container facts.
fn metadata_from_image(image: &HeifImage) -> ImageMetadata {
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

    // Record HEIC container facts in the generic table.
    let (width, height) = item_dims(&primary);
    let has_alpha = image.alpha_auxiliary_of(primary.id()).is_some();
    md.insert(
        MetadataKey::new(MetadataNamespace::Heic, "bit_depth"),
        MetadataValue::U64(bit_depth as u64),
    );
    md.insert(
        MetadataKey::new(MetadataNamespace::Heic, "has_alpha"),
        MetadataValue::U64(has_alpha as u64),
    );
    md.insert(
        MetadataKey::new(MetadataNamespace::Heic, "width"),
        MetadataValue::U64(width as u64),
    );
    md.insert(
        MetadataKey::new(MetadataNamespace::Heic, "height"),
        MetadataValue::U64(height as u64),
    );

    md
}

// ── decoded-frame presentation ───────────────────────────────────────────────

/// Convert gamut-heic's presentation output (8-bit RGBA, transforms applied)
/// to rawshift's 16-bit [`RgbImage`]: alpha dropped, samples scaled by `*257`
/// (exact at both endpoints).
#[cfg(feature = "hw")]
fn rgba8_to_rgb_image(rgba: &gamut_core::ImageBuf<gamut_core::Rgba8>) -> RawResult<RgbImage> {
    let (width, height) = (rgba.width(), rgba.height());
    let samples = rgba.as_samples();
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for px in samples.chunks_exact(4) {
        rgb.push(u16::from(px[0]) * 257);
        rgb.push(u16::from(px[1]) * 257);
        rgb.push(u16::from(px[2]) * 257);
    }
    RgbImage::new(width, height, rgb)
}

// ── rawshift-hwdec adapter ───────────────────────────────────────────────────

/// The adapter that plugs a [`rawshift_hwdec::HwStillDecoder`] into
/// gamut-heic's [`HevcDecoder`](gamut_heic::HevcDecoder) seam.
///
/// gamut-heic drives the whole decode pipeline (derivation, tiles, colour,
/// alpha, transforms) and calls
/// [`decode_intra`](gamut_heic::HevcDecoder::decode_intra) once per coded
/// picture; the adapter lowers each call to a
/// [`StillDecodeRequest`](rawshift_hwdec::StillDecodeRequest) and lifts the
/// returned surface back into gamut-heic's planar frame. This module contains
/// **no** platform code — that lives exclusively in rawshift-hwdec.
#[cfg(feature = "hw")]
mod hw {
    use gamut_core::{Error as GamutError, Result as GamutResult};
    use gamut_heic::{ChromaFormat, DecodedFrame as PlanarFrame, HevcConfig, HevcDecoder};
    use rawshift_hwdec::{
        ChromaSubsampling, CodecConfig, DecodedFrame as HwFrame, HwDecodeError, HwStillDecoder,
        PixelFormat, Plane, StillDecodeRequest,
    };

    use crate::error::{FormatError, RawError};

    /// A [`HevcDecoder`] over a hardware [`HwStillDecoder`].
    ///
    /// gamut errors carry only `&'static str`, so the adapter records the
    /// last rich [`HwDecodeError`] and [`into_raw_error`](Self::into_raw_error)
    /// recovers it after the pipeline returns.
    pub(super) struct HwHevcAdapter {
        inner: Box<dyn HwStillDecoder>,
        last_error: Option<HwDecodeError>,
    }

    impl HwHevcAdapter {
        pub(super) fn new(inner: Box<dyn HwStillDecoder>) -> Self {
            Self {
                inner,
                last_error: None,
            }
        }

        /// Map the pipeline's failure to a [`RawError`], preferring the rich
        /// hardware error captured during the failing `decode_intra` call.
        pub(super) fn into_raw_error(self, source: GamutError) -> RawError {
            match self.last_error {
                Some(HwDecodeError::Unavailable { reason, .. }) => RawError::HwDecoderUnavailable {
                    codec: super::HEVC,
                    reason,
                },
                Some(other) => RawError::Format(FormatError::ImageDecode {
                    format: "HEIC",
                    message: other.to_string(),
                }),
                None => RawError::Format(FormatError::ImageDecode {
                    format: "HEIC",
                    message: source.to_string(),
                }),
            }
        }
    }

    impl HevcDecoder for HwHevcAdapter {
        fn decode_intra(
            &mut self,
            config: &HevcConfig,
            payload: &[u8],
        ) -> GamutResult<PlanarFrame> {
            let hvcc = hvcc_bytes(config)?;
            let request = StillDecodeRequest {
                config: CodecConfig::Hvcc(&hvcc),
                payload,
                // hvcC carries no picture size; the coded bitstream (SPS) is
                // authoritative and the backend reports the real geometry.
                width: 0,
                height: 0,
                bit_depth: config.bit_depth_luma(),
                chroma: chroma_subsampling(config.chroma_format()),
            };
            match self.inner.decode_still(&request) {
                Ok(frame) => hw_frame_to_planar(&frame, config.chroma_format()),
                Err(error) => {
                    let mapped = match &error {
                        HwDecodeError::Unavailable { .. } => {
                            GamutError::Unsupported("HEIC: hardware HEVC decoder unavailable")
                        }
                        _ => GamutError::InvalidInput("HEIC: hardware HEVC decode failed"),
                    };
                    self.last_error = Some(error);
                    Err(mapped)
                }
            }
        }
    }

    /// gamut-heic's chroma format as rawshift-hwdec's advisory subsampling.
    fn chroma_subsampling(chroma: ChromaFormat) -> ChromaSubsampling {
        match chroma {
            ChromaFormat::Monochrome => ChromaSubsampling::Cs400,
            ChromaFormat::Yuv420 => ChromaSubsampling::Cs420,
            ChromaFormat::Yuv422 => ChromaSubsampling::Cs422,
            ChromaFormat::Yuv444 => ChromaSubsampling::Cs444,
        }
    }

    /// Re-serialize a typed [`HevcConfig`] to `hvcC` record bytes (ISO/IEC
    /// 14496-15 §8.3.3.1) for the hardware seam, which transports the raw
    /// record. Reserved bits are written as all-ones per the spec's layout;
    /// gamut's parser masks them away, so `parse(hvcc_bytes(c)) == c`.
    ///
    /// gamut-heic is deliberately decode-only for HEIF, so it exposes no
    /// serializer; this writer is rawshift-owned FFI glue, not duplicated
    /// upstream logic.
    ///
    /// # Errors
    ///
    /// Returns an error when a count no longer fits its record field (only
    /// possible for a hand-mutated config; parse-produced configs always fit).
    pub(super) fn hvcc_bytes(config: &HevcConfig) -> GamutResult<Vec<u8>> {
        if config.arrays.len() > usize::from(u8::MAX) {
            return Err(GamutError::InvalidInput(
                "HEIC: hvcC cannot carry more than 255 parameter-set arrays",
            ));
        }
        let mut out = Vec::with_capacity(64);
        out.push(1); // configurationVersion
        out.push(
            (config.general_profile_space << 6)
                | (u8::from(config.general_tier_flag) << 5)
                | (config.general_profile_idc & 0x1f),
        );
        out.extend_from_slice(&config.general_profile_compatibility_flags.to_be_bytes());
        out.extend_from_slice(&config.general_constraint_indicator_flags.to_be_bytes()[2..8]);
        out.push(config.general_level_idc);
        out.extend_from_slice(
            &(0xf000 | (config.min_spatial_segmentation_idc & 0x0fff)).to_be_bytes(),
        );
        out.push(0xfc | (config.parallelism_type & 0x03));
        out.push(0xfc | (config.chroma_format_idc & 0x03));
        out.push(0xf8 | (config.bit_depth_luma_minus8 & 0x07));
        out.push(0xf8 | (config.bit_depth_chroma_minus8 & 0x07));
        out.extend_from_slice(&config.avg_frame_rate.to_be_bytes());
        out.push(
            (config.constant_frame_rate << 6)
                | ((config.num_temporal_layers & 0x07) << 3)
                | (u8::from(config.temporal_id_nested) << 2)
                | (config.length_size_minus_one & 0x03),
        );
        out.push(config.arrays.len() as u8);
        for array in &config.arrays {
            if array.nal_units.len() > usize::from(u16::MAX) {
                return Err(GamutError::InvalidInput(
                    "HEIC: hvcC array cannot carry more than 65535 NAL units",
                ));
            }
            out.push((u8::from(array.completeness) << 7) | (array.nal_unit_type.raw() & 0x3f));
            out.extend_from_slice(&(array.nal_units.len() as u16).to_be_bytes());
            for nal in &array.nal_units {
                let Ok(len) = u16::try_from(nal.len()) else {
                    return Err(GamutError::InvalidInput(
                        "HEIC: hvcC NAL unit longer than 65535 bytes",
                    ));
                };
                out.extend_from_slice(&len.to_be_bytes());
                out.extend_from_slice(nal);
            }
        }
        Ok(out)
    }

    /// Lift a hardware surface ([`HwFrame`]: NV12/P010/I420/I010, byte planes
    /// with strides) into gamut-heic's uniform-`u16` planar frame.
    ///
    /// All four surface formats are 4:2:0 containers, but a **monochrome**
    /// coded stream (`hvcC` chroma_format_idc 0 — e.g. an alpha or depth
    /// auxiliary) still comes back from hardware in one of them, with neutral
    /// chroma planes. `coded_chroma` (from the `hvcC`) decides: monochrome
    /// streams drop the surface's chroma planes and present as
    /// [`ChromaFormat::Monochrome`]; everything else presents as
    /// [`ChromaFormat::Yuv420`]. `PlanarFrame::new` revalidates the geometry.
    pub(super) fn hw_frame_to_planar(
        frame: &HwFrame,
        coded_chroma: ChromaFormat,
    ) -> GamutResult<PlanarFrame> {
        let (w, h) = (frame.width(), frame.height());
        let bit_depth = frame.bit_depth();
        let (cw, ch) = (w.div_ceil(2) as usize, h.div_ceil(2) as usize);
        let planes = frame.planes();

        if coded_chroma == ChromaFormat::Monochrome {
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
            return PlanarFrame::new(w, h, bit_depth, ChromaFormat::Monochrome, y, vec![], vec![]);
        }

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
            PixelFormat::I010 => {
                let shift = 0;
                (
                    read_plane_16(&planes[0], w as usize, h as usize, shift, bit_depth),
                    read_plane_16(&planes[1], cw, ch, shift, bit_depth),
                    read_plane_16(&planes[2], cw, ch, shift, bit_depth),
                )
            }
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

        PlanarFrame::new(w, h, bit_depth, ChromaFormat::Yuv420, y, cb, cr)
    }

    /// Copy an 8-bit plane into `u16` samples, honouring the row stride.
    fn read_plane_8(plane: &Plane, width: usize, rows: usize) -> Vec<u16> {
        let mut out = Vec::with_capacity(width * rows);
        for row in 0..rows {
            let start = row * plane.stride;
            out.extend(
                plane.data[start..start + width]
                    .iter()
                    .map(|&b| u16::from(b)),
            );
        }
        out
    }

    /// Split an 8-bit interleaved CbCr plane into separate Cb/Cr samples.
    fn deinterleave_8(plane: &Plane, width: usize, rows: usize) -> (Vec<u16>, Vec<u16>) {
        let mut cb = Vec::with_capacity(width * rows);
        let mut cr = Vec::with_capacity(width * rows);
        for row in 0..rows {
            let start = row * plane.stride;
            for pair in plane.data[start..start + width * 2].chunks_exact(2) {
                cb.push(u16::from(pair[0]));
                cr.push(u16::from(pair[1]));
            }
        }
        (cb, cr)
    }

    /// Copy a 16-bit-word plane into `u16` samples: little-endian words,
    /// shifted down by `shift` (P010 keeps the value in the high bits) and
    /// masked to `bit_depth`.
    fn read_plane_16(
        plane: &Plane,
        width: usize,
        rows: usize,
        shift: u32,
        bit_depth: u8,
    ) -> Vec<u16> {
        let mask = sample_mask(bit_depth);
        let mut out = Vec::with_capacity(width * rows);
        for row in 0..rows {
            let start = row * plane.stride;
            for word in plane.data[start..start + width * 2].chunks_exact(2) {
                out.push((u16::from_le_bytes([word[0], word[1]]) >> shift) & mask);
            }
        }
        out
    }

    /// Split a 16-bit-word interleaved CbCr plane into Cb/Cr samples.
    fn deinterleave_16(
        plane: &Plane,
        width: usize,
        rows: usize,
        shift: u32,
        bit_depth: u8,
    ) -> (Vec<u16>, Vec<u16>) {
        let mask = sample_mask(bit_depth);
        let mut cb = Vec::with_capacity(width * rows);
        let mut cr = Vec::with_capacity(width * rows);
        for row in 0..rows {
            let start = row * plane.stride;
            for pair in plane.data[start..start + width * 4].chunks_exact(4) {
                cb.push((u16::from_le_bytes([pair[0], pair[1]]) >> shift) & mask);
                cr.push((u16::from_le_bytes([pair[2], pair[3]]) >> shift) & mask);
            }
        }
        (cb, cr)
    }

    /// The all-ones mask for `bit_depth`-bit samples (identity for 16).
    fn sample_mask(bit_depth: u8) -> u16 {
        if bit_depth >= 16 {
            u16::MAX
        } else {
            (1u16 << bit_depth) - 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamut_isobmff::{IsoBmffImage, Item, ItemReference, Property, PropertyKind, write};

    /// Minimal valid `hvcC`: 23-byte header, 8-bit 4:2:0, lengthSizeMinusOne
    /// = 3 (4-byte NAL length prefixes), no parameter-set arrays.
    fn minimal_hvcc() -> Vec<u8> {
        let mut v = vec![0u8; 23];
        v[0] = 1; // configurationVersion
        v[16] = 0b0000_0001; // chroma_format_idc = 1 (4:2:0)
        v[21] = 0b0000_0011; // lengthSizeMinusOne = 3
        v[22] = 0; // numOfArrays
        v
    }

    /// As [`minimal_hvcc`] but monochrome (chroma_format_idc = 0) — the shape
    /// of an alpha/depth auxiliary's configuration.
    fn mono_hvcc() -> Vec<u8> {
        let mut v = minimal_hvcc();
        v[16] = 0;
        v
    }

    /// A coded hvc1 item: hvcC config + a single IDR_W_RADL NAL payload
    /// (header byte 0x26 = type 19) behind a 4-byte length prefix.
    fn coded_item(id: u32, width: u32, height: u32) -> Item {
        Item {
            id,
            item_type: *b"hvc1",
            name: String::new(),
            content_type: None,
            content_encoding: None,
            hidden: false,
            references: vec![],
            properties: vec![
                Property {
                    essential: true,
                    kind: PropertyKind::CodecConfiguration {
                        kind: *b"hvcC",
                        data: minimal_hvcc(),
                    },
                },
                Property {
                    essential: false,
                    kind: PropertyKind::ImageSpatialExtents { width, height },
                },
            ],
            payload: vec![0x00, 0x00, 0x00, 0x03, 0x26, 0x01, 0xDD],
        }
    }

    /// A synthetic iPhone-shaped HEIC: primary image + thumbnail + depth +
    /// gain-map + alpha auxiliaries + EXIF + XMP — everything the
    /// backend-less enumeration/metadata paths must see.
    fn synthetic_heic() -> Vec<u8> {
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
        let depth = aux(3, "urn:mpeg:hevc:2015:auxid:2");
        let gain = aux(4, "urn:com:apple:photo:2020:aux:hdrgainmap");
        let mut alpha = aux(5, "urn:mpeg:mpegB:cicp:systems:auxiliary:alpha");
        // An alpha plane is coded monochrome.
        alpha.properties[0] = Property {
            essential: true,
            kind: PropertyKind::CodecConfiguration {
                kind: *b"hvcC",
                data: mono_hvcc(),
            },
        };

        // EXIF item: 4-byte exif_tiff_header_offset (0) + a TIFF header stub.
        let exif = Item {
            id: 6,
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
            id: 7,
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
            major_brand: *b"heic",
            minor_version: 0,
            compatible_brands: vec![*b"heic", *b"mif1"],
            primary_item_id: 1,
            items: vec![primary, thumb, depth, gain, alpha, exif, xmp],
            groups: vec![],
        };
        write(&image).expect("write synthetic HEIC")
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
        assert!(HeicFile::open(junk).is_err());
    }

    #[test]
    fn read_metadata_junk_returns_default() {
        let junk = vec![0u8; 64];
        assert_eq!(read_heic_metadata(&junk), ImageMetadata::default());
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
        let file = HeicFile::open(synthetic_heic()).expect("open synthetic HEIC");
        let aux = file.aux_images();
        assert_eq!(aux.len(), 4, "thumbnail + depth + gain map + alpha");

        let kind = |k: HeicAuxKind| aux.iter().filter(|a| a.kind == k).count();
        assert_eq!(kind(HeicAuxKind::Thumbnail), 1);
        assert_eq!(kind(HeicAuxKind::DepthMap), 1);
        assert_eq!(kind(HeicAuxKind::GainMap), 1);
        assert_eq!(
            kind(HeicAuxKind::Auxiliary),
            1,
            "alpha classifies as Auxiliary"
        );

        let thumb = aux
            .iter()
            .find(|a| a.kind == HeicAuxKind::Thumbnail)
            .unwrap();
        assert_eq!((thumb.width, thumb.height), (1, 1));
        let gain = aux.iter().find(|a| a.kind == HeicAuxKind::GainMap).unwrap();
        assert_eq!(
            gain.aux_type.as_deref(),
            Some("urn:com:apple:photo:2020:aux:hdrgainmap")
        );
    }

    #[test]
    fn synthetic_container_metadata_backend_less() {
        let file = HeicFile::open(synthetic_heic()).expect("open synthetic HEIC");
        let md = file.metadata();
        assert_eq!(md.exif_raw.as_deref(), Some(&tiff_stub()[..]));
        assert!(
            md.xmp
                .as_deref()
                .is_some_and(|x| x.starts_with(b"<x:xmpmeta"))
        );
        assert_eq!(md.image.bit_depth, 8);
        assert_eq!(
            md.get(MetadataNamespace::Heic, "width"),
            Some(&MetadataValue::U64(2))
        );
        assert_eq!(
            md.get(MetadataNamespace::Heic, "height"),
            Some(&MetadataValue::U64(2))
        );
        assert_eq!(
            md.get(MetadataNamespace::Heic, "has_alpha"),
            Some(&MetadataValue::U64(1))
        );
    }

    /// Until a platform backend lands (VAAPI is next), every pixel decode —
    /// with or without the `hw` feature — reports the matchable
    /// `HwDecoderUnavailable`, while enumeration/metadata keep working.
    #[test]
    fn pixel_decode_reports_hw_decoder_unavailable() {
        let file = HeicFile::open(synthetic_heic()).expect("open synthetic HEIC");
        assert!(!heic_hw_decode_available());
        let err = file.decode_primary().unwrap_err();
        assert!(
            matches!(err, RawError::HwDecoderUnavailable { codec: "HEVC", .. }),
            "expected HwDecoderUnavailable, got: {err}"
        );
        let thumb = file
            .aux_images()
            .iter()
            .find(|a| a.kind == HeicAuxKind::Thumbnail)
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
        use super::super::hw::{HwHevcAdapter, hvcc_bytes, hw_frame_to_planar};
        use super::*;
        use gamut_color::{ColorRange, ycbcr_to_rgb};
        use gamut_heic::{ChromaFormat, HeifContainer, HevcConfig};
        use rawshift_hwdec::{
            CodecConfig, DecodedFrame as HwFrame, HwDecodeError, HwStillDecoder, PixelFormat,
            Plane, StillDecodeRequest,
        };

        #[test]
        fn hvcc_round_trips_through_the_serializer() {
            // Header-only record.
            let original = HevcConfig::parse(&minimal_hvcc()).unwrap();
            let bytes = hvcc_bytes(&original).unwrap();
            assert_eq!(HevcConfig::parse(&bytes).unwrap(), original);

            // Record with parameter-set arrays (VPS + SPS with two NALs).
            let mut raw = minimal_hvcc();
            raw[22] = 2; // numOfArrays
            // array 1: complete, VPS (32), 1 NAL of 3 bytes
            raw.extend_from_slice(&[0x80 | 32, 0, 1, 0, 3, 0x40, 0x01, 0xAA]);
            // array 2: incomplete, SPS (33), 2 NALs
            raw.extend_from_slice(&[33, 0, 2, 0, 2, 0x42, 0x01, 0, 1, 0x99]);
            let original = HevcConfig::parse(&raw).unwrap();
            let bytes = hvcc_bytes(&original).unwrap();
            assert_eq!(HevcConfig::parse(&bytes).unwrap(), original);
        }

        fn hw_frame_i420(width: u32, height: u32, y: u8, cb: u8, cr: u8) -> HwFrame {
            let (cw, ch) = (width.div_ceil(2) as usize, height.div_ceil(2) as usize);
            HwFrame::new(
                PixelFormat::I420,
                width,
                height,
                8,
                ColorRange::Limited,
                vec![
                    Plane {
                        data: vec![y; (width * height) as usize],
                        stride: width as usize,
                    },
                    Plane {
                        data: vec![cb; cw * ch],
                        stride: cw,
                    },
                    Plane {
                        data: vec![cr; cw * ch],
                        stride: cw,
                    },
                ],
            )
            .expect("valid I420 frame")
        }

        #[test]
        fn i420_and_nv12_lift_to_the_same_planar_frame() {
            let i420 = hw_frame_i420(2, 2, 81, 90, 240);
            let planar = hw_frame_to_planar(&i420, ChromaFormat::Yuv420).unwrap();
            assert_eq!(planar.y(), &[81u16; 4]);
            assert_eq!(planar.cb(), &[90]);
            assert_eq!(planar.cr(), &[240]);

            let nv12 = HwFrame::new(
                PixelFormat::Nv12,
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
                        data: vec![90, 240], // interleaved Cb, Cr
                        stride: 2,
                    },
                ],
            )
            .unwrap();
            assert_eq!(
                hw_frame_to_planar(&nv12, ChromaFormat::Yuv420).unwrap(),
                planar
            );

            // A monochrome coded stream (alpha/depth) drops the surface's
            // neutral chroma planes.
            let mono = hw_frame_to_planar(&i420, ChromaFormat::Monochrome).unwrap();
            assert_eq!(mono.chroma(), ChromaFormat::Monochrome);
            assert_eq!(mono.y(), &[81u16; 4]);
            assert!(mono.cb().is_empty() && mono.cr().is_empty());
        }

        #[test]
        fn p010_and_i010_lift_with_correct_bit_positions() {
            // 10-bit value 612: P010 stores it << 6; I010 stores it as-is.
            let value: u16 = 612;
            let p010_word = (value << 6).to_le_bytes();
            let i010_word = value.to_le_bytes();

            let p010 = HwFrame::new(
                PixelFormat::P010,
                2,
                1,
                10,
                ColorRange::Limited,
                vec![
                    Plane {
                        data: [p010_word, p010_word].concat(),
                        stride: 4,
                    },
                    Plane {
                        data: [p010_word, p010_word].concat(), // one CbCr pair
                        stride: 4,
                    },
                ],
            )
            .unwrap();
            let planar = hw_frame_to_planar(&p010, ChromaFormat::Yuv420).unwrap();
            assert_eq!(planar.y(), &[value, value]);
            assert_eq!((planar.cb(), planar.cr()), (&[value][..], &[value][..]));
            assert_eq!(planar.bit_depth(), 10);

            let i010 = HwFrame::new(
                PixelFormat::I010,
                2,
                1,
                10,
                ColorRange::Limited,
                vec![
                    Plane {
                        data: [i010_word, i010_word].concat(),
                        stride: 4,
                    },
                    Plane {
                        data: i010_word.to_vec(),
                        stride: 2,
                    },
                    Plane {
                        data: i010_word.to_vec(),
                        stride: 2,
                    },
                ],
            )
            .unwrap();
            let planar = hw_frame_to_planar(&i010, ChromaFormat::Yuv420).unwrap();
            assert_eq!(planar.y(), &[value, value]);
            assert_eq!(planar.cb(), &[value]);
        }

        /// A synthetic in-process "hardware" decoder: asserts the request the
        /// adapter lowers (hvcC config variant, payload passthrough) and
        /// returns a fixed I420 frame — exactly the seam #29's VAAPI backend
        /// will occupy.
        struct FakeHevcDecoder {
            y: u8,
            cb: u8,
            cr: u8,
        }

        impl HwStillDecoder for FakeHevcDecoder {
            fn decode_still(
                &mut self,
                request: &StillDecodeRequest<'_>,
            ) -> Result<HwFrame, HwDecodeError> {
                assert_eq!(request.codec(), rawshift_hwdec::HwCodec::Hevc);
                assert!(matches!(request.config, CodecConfig::Hvcc(_)));
                // The adapter hands the item payload through untouched
                // (length-prefixed NAL stream).
                assert_eq!(request.payload, &[0x00, 0x00, 0x00, 0x03, 0x26, 0x01, 0xDD]);
                assert_eq!(request.bit_depth, 8);
                // The hvcC the adapter re-serialized must parse back.
                let config = HevcConfig::parse(request.config.bytes()).expect("valid hvcC");
                assert_eq!(config.nal_length_size(), 4);
                Ok(hw_frame_i420(2, 2, self.y, self.cb, self.cr))
            }
        }

        #[test]
        fn synthetic_frames_decode_through_the_adapter_to_rgb16() {
            let (y, cb, cr) = (81u8, 90u8, 240u8);
            let data = synthetic_heic();
            let container = HeifContainer::parse(&data).unwrap();

            let mut adapter = HwHevcAdapter::new(Box::new(FakeHevcDecoder { y, cb, cr }));
            let rgba = container
                .image()
                .decode_primary_rgba8(&mut adapter)
                .expect("decode through the adapter");
            let rgb = rgba8_to_rgb_image(&rgba).expect("present as RgbImage");

            // No `colr` in the synthetic container → gamut-heic's documented
            // BT.601 limited-range default, i.e. gamut-color's ycbcr module.
            let (r, g, b) = ycbcr_to_rgb(y, cb, cr, ColorRange::Limited);
            let expected = [u16::from(r) * 257, u16::from(g) * 257, u16::from(b) * 257];
            assert_eq!((rgb.width(), rgb.height()), (2, 2));
            for px in rgb.data().chunks_exact(3) {
                assert_eq!(px, expected);
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

            let data = synthetic_heic();
            let container = HeifContainer::parse(&data).unwrap();
            let mut adapter = HwHevcAdapter::new(Box::new(Failing));
            let err = container
                .image()
                .decode_primary_rgba8(&mut adapter)
                .unwrap_err();
            let raw = adapter.into_raw_error(err);
            let text = raw.to_string();
            assert!(
                text.contains("driver rejected the session"),
                "rich backend message must survive: {text}"
            );
        }
    }
}
