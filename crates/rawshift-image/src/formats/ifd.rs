//! Shared gamut-ifd helpers for the TIFF-based RAW decoders.
//!
//! ARW, CR2, and NEF are TIFF containers (CR3 embeds standalone TIFF blocks in
//! its `CMT1`–`CMT4` boxes); this module bridges them onto the
//! [`gamut_ifd`] structure engine:
//!
//! - [`read_all`] / [`parse_tree`] — load a file and parse its IFD chain plus
//!   the standard sub-IFD pointer tree (SubIFDs/Exif/GPS), tolerating a
//!   malformed child directory with a warning (the pre-migration parser's
//!   behaviour) instead of failing the whole parse.
//! - [`tags`] — the private tag catalogue these decoders read. Tag *semantics*
//!   deliberately stay rawshift-side as data over the gamut-ifd engine.
//! - Value coercions and metadata extractors mirroring the legacy
//!   `tiff::metadata_helper` results exactly.

// Which subset of this module (tag constants, tree walking, metadata
// extractors) is live depends on the enabled format-feature combination
// (e.g. a `cr3`-only build uses just the lazy-reader helpers), so dead-code
// analysis is allowed to miss per-combination.
#![allow(dead_code)]

use std::io::{Read, Seek, SeekFrom};

use gamut_ifd::{ByteOrder, Ifd, Value, Variant, read_ifd_at};

use crate::core::metadata::{DateTimeInfo, ExifInfo, GpsInfo, SRational, URational};
use crate::error::{ParseError, RawError, RawResult};

/// The tag catalogue used by the ARW/CR2/NEF decoders, CR3's CMT TIFF blocks,
/// and format detection.
///
/// Numeric ids per TIFF 6.0, TIFF/EP, Exif 3.0, DNG 1.7, and the community
/// Sony SR2 documentation. Names follow the legacy `TiffTag` variants.
pub(crate) mod tags {
    // ── Baseline TIFF (TIFF 6.0 §8) ──────────────────────────────────────────
    pub const NEW_SUBFILE_TYPE: u16 = 0x00FE;
    pub const IMAGE_WIDTH: u16 = 0x0100;
    pub const IMAGE_LENGTH: u16 = 0x0101;
    pub const BITS_PER_SAMPLE: u16 = 0x0102;
    pub const COMPRESSION: u16 = 0x0103;
    pub const PHOTOMETRIC_INTERPRETATION: u16 = 0x0106;
    pub const MAKE: u16 = 0x010F;
    pub const MODEL: u16 = 0x0110;
    pub const STRIP_OFFSETS: u16 = 0x0111;
    pub const ORIENTATION: u16 = 0x0112;
    pub const STRIP_BYTE_COUNTS: u16 = 0x0117;
    pub const DATE_TIME: u16 = 0x0132;
    pub const TILE_WIDTH: u16 = 0x0142;
    pub const TILE_LENGTH: u16 = 0x0143;
    pub const TILE_OFFSETS: u16 = 0x0144;
    pub const TILE_BYTE_COUNTS: u16 = 0x0145;
    pub const SUB_IFDS: u16 = 0x014A;
    pub const JPEG_INTERCHANGE_FORMAT: u16 = 0x0201;
    pub const JPEG_INTERCHANGE_FORMAT_LENGTH: u16 = 0x0202;

    // ── Sub-IFD pointers (TIFF/EP, Exif 3.0) ─────────────────────────────────
    pub const EXIF_IFD_POINTER: u16 = 0x8769;
    pub const GPS_INFO_IFD_POINTER: u16 = 0x8825;

    // ── EXIF private IFD (Exif 3.0 §4.6.5) ───────────────────────────────────
    pub const EXPOSURE_TIME: u16 = 0x829A;
    pub const F_NUMBER: u16 = 0x829D;
    pub const EXPOSURE_PROGRAM: u16 = 0x8822;
    pub const ISO_SPEED_RATINGS: u16 = 0x8827;
    pub const DATE_TIME_ORIGINAL: u16 = 0x9003;
    pub const DATE_TIME_DIGITIZED: u16 = 0x9004;
    pub const OFFSET_TIME: u16 = 0x9010;
    pub const OFFSET_TIME_ORIGINAL: u16 = 0x9011;
    pub const BRIGHTNESS_VALUE: u16 = 0x9203;
    pub const EXPOSURE_BIAS_VALUE: u16 = 0x9204;
    pub const MAX_APERTURE_VALUE: u16 = 0x9205;
    pub const METERING_MODE: u16 = 0x9207;
    pub const FLASH: u16 = 0x9209;
    pub const FOCAL_LENGTH: u16 = 0x920A;
    pub const MAKER_NOTE: u16 = 0x927C;
    pub const SUB_SEC_TIME: u16 = 0x9290;
    pub const SUB_SEC_TIME_ORIGINAL: u16 = 0x9291;
    pub const FOCAL_LENGTH_IN_35MM_FILM: u16 = 0xA405;
    pub const LENS_MAKE: u16 = 0xA433;
    pub const LENS_MODEL: u16 = 0xA434;

    // ── GPS IFD (Exif 3.0 §4.6.6) ────────────────────────────────────────────
    pub const GPS_LATITUDE_REF: u16 = 0x0001;
    pub const GPS_LATITUDE: u16 = 0x0002;
    pub const GPS_LONGITUDE_REF: u16 = 0x0003;
    pub const GPS_LONGITUDE: u16 = 0x0004;
    pub const GPS_ALTITUDE_REF: u16 = 0x0005;
    pub const GPS_ALTITUDE: u16 = 0x0006;
    pub const GPS_TIME_STAMP: u16 = 0x0007;
    pub const GPS_SPEED: u16 = 0x000D;
    pub const GPS_IMG_DIRECTION: u16 = 0x0011;
    pub const GPS_DATE_STAMP: u16 = 0x001D;

    // ── TIFF/EP + DNG tags the RAW sub-IFDs carry ────────────────────────────
    pub const CFA_PATTERN: u16 = 0x828E;
    pub const DNG_VERSION: u16 = 0xC612;
    pub const BLACK_LEVEL: u16 = 0xC61A;
    pub const WHITE_LEVEL: u16 = 0xC61D;
    pub const DEFAULT_CROP_ORIGIN: u16 = 0xC61F;
    pub const DEFAULT_CROP_SIZE: u16 = 0xC620;
    /// DNG 1.2 `ProfileName` (read from gamut-dng's RawTag extras).
    pub const PROFILE_NAME: u16 = 0xC6F8;
    /// DNG 1.2 `ProfileToneCurve` (read from gamut-dng's RawTag extras).
    pub const PROFILE_TONE_CURVE: u16 = 0xC6FC;
    /// DNG 1.3 `NoiseProfile` (read from gamut-dng's RawTag extras).
    pub const NOISE_PROFILE: u16 = 0xC761;

    // ── Sony SR2 private tags (community-documented) ─────────────────────────
    /// SR2 private sub-IFD pointer in IFD0 (also the XMP tag id in plain TIFF;
    /// Sony reuses it for the SR2 block).
    pub const SONY_SR2_PRIVATE: u16 = 0x02BC;
    /// White-balance RGGB levels, found in the raw sub-IFD, the maker note, or
    /// the SR2 sub-IFD.
    pub const SONY_WB_RGGB_LEVELS: u16 = 0x7313;
    // The remaining SR2-set tags are catalogued for the SR2 sub-IFD work that
    // follows (decryption + tone curve); the decoders do not read them yet.
    pub const SONY_RAW_FILE_TYPE: u16 = 0x7200;
    pub const SONY_TONE_CURVE: u16 = 0x7010;
    pub const SONY_CROP_TOP_LEFT: u16 = 0x74C7;
    pub const SONY_CROP_SIZE: u16 = 0x74C8;
    pub const SR2_SUB_IFD_LENGTH: u16 = 0x7201;
    pub const SR2_SUB_IFD_KEY: u16 = 0x7221;
}

/// The sub-IFD pointer tags the TIFF-based RAW decoders follow.
const POINTER_TAGS: [u16; 3] = [
    tags::SUB_IFDS,
    tags::EXIF_IFD_POINTER,
    tags::GPS_INFO_IFD_POINTER,
];

/// Bounds the sub-IFD nesting a hostile pointer graph can force.
const MAX_SUB_IFD_DEPTH: usize = 8;

/// A parsed TIFF container: its layout parameters and the IFD chain with the
/// standard sub-IFD pointer tree resolved.
pub(crate) struct TiffTree {
    /// The byte order the stream was written in.
    pub order: ByteOrder,
    /// Classic TIFF or BigTIFF.
    pub variant: Variant,
    /// The top-level IFD chain; resolved pointers hang off each
    /// [`Ifd::sub_ifds`] group, keyed by pointer tag.
    pub ifds: Vec<Ifd>,
}

/// Reads the whole stream into memory (from absolute offset 0, matching the
/// legacy parser, which always sought to the file start).
pub(crate) fn read_all<R: Read + Seek>(mut reader: R) -> RawResult<Vec<u8>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;
    Ok(data)
}

/// Parses a TIFF stream: header, the top-level IFD chain, and the
/// SubIFDs/Exif/GPS pointer tree.
///
/// A malformed *child* directory is skipped with a warning (matching the
/// legacy parser); a malformed header or top-level chain is an error, wrapped
/// as [`RawError::Gamut`] under `context`.
pub(crate) fn parse_tree(data: &[u8], context: &'static str) -> RawResult<TiffTree> {
    let file = gamut_ifd::read(data).map_err(|e| RawError::gamut(context, e))?;
    let mut ifds = file.ifds;
    let mut visited: Vec<u64> = Vec::new();
    for ifd in &mut ifds {
        resolve_pointers(data, ifd, file.order, file.variant, &mut visited, 0);
    }
    Ok(TiffTree {
        order: file.order,
        variant: file.variant,
        ifds,
    })
}

/// Follows the [`POINTER_TAGS`] of `ifd` (recursively), attaching each parsed
/// child as a sub-IFD group and removing the raw pointer field. A child that
/// fails to parse is logged and skipped; repeated offsets and over-deep
/// nesting are ignored rather than followed.
fn resolve_pointers(
    data: &[u8],
    ifd: &mut Ifd,
    order: ByteOrder,
    variant: Variant,
    visited: &mut Vec<u64>,
    depth: usize,
) {
    if depth > MAX_SUB_IFD_DEPTH {
        return;
    }
    for tag in POINTER_TAGS {
        let offsets: Vec<u64> = match ifd.get(tag) {
            Some(Value::Long(v)) | Some(Value::Ifd(v)) => v.iter().map(|&o| u64::from(o)).collect(),
            Some(Value::Long8(v)) | Some(Value::Ifd8(v)) => v.clone(),
            _ => continue,
        };
        let mut children = Vec::with_capacity(offsets.len());
        for offset in offsets {
            if offset == 0 || visited.contains(&offset) {
                continue;
            }
            visited.push(offset);
            match read_ifd_at(data, offset, order, variant) {
                Ok(mut child) => {
                    resolve_pointers(data, &mut child, order, variant, visited, depth + 1);
                    children.push(child);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse sub-IFD (pointer tag 0x{:04X}) at offset {}: {}",
                        tag,
                        offset,
                        e
                    );
                }
            }
        }
        ifd.remove(tag);
        if !children.is_empty() {
            ifd.set_sub_ifd(tag, children);
        }
    }
}

/// The child IFDs attached under pointer `tag`, or an empty slice.
pub(crate) fn sub_ifd_group(ifd: &Ifd, tag: u16) -> &[Ifd] {
    ifd.sub_ifds()
        .iter()
        .find(|group| group.tag == tag)
        .map(|group| group.ifds.as_slice())
        .unwrap_or(&[])
}

/// The EXIF private sub-IFD of `ifd0`, if it was resolved.
pub(crate) fn exif_ifd(ifd0: &Ifd) -> Option<&Ifd> {
    sub_ifd_group(ifd0, tags::EXIF_IFD_POINTER).first()
}

/// The GPS sub-IFD of `ifd0`, if it was resolved.
pub(crate) fn gps_ifd(ifd0: &Ifd) -> Option<&Ifd> {
    sub_ifd_group(ifd0, tags::GPS_INFO_IFD_POINTER).first()
}

/// Normalises an on-disk string the way the legacy parser did: trailing NUL
/// padding stripped, then surrounding whitespace trimmed.
pub(crate) fn clean_ascii(s: &str) -> String {
    s.trim_end_matches('\0').trim().to_string()
}

/// Reads `tag` as a normalised string (see [`clean_ascii`]).
pub(crate) fn ascii_tag(ifd: &Ifd, tag: u16) -> Option<String> {
    ifd.get(tag).and_then(Value::as_str).map(clean_ascii)
}

/// The first element of an unsigned-integer `tag` (`BYTE`/`SHORT`/`LONG`),
/// the coercion array-shaped tags like `BitsPerSample` need.
pub(crate) fn first_u32(ifd: &Ifd, tag: u16) -> Option<u32> {
    match ifd.get(tag)? {
        Value::Byte(v) => v.first().map(|&x| u32::from(x)),
        Value::Short(v) => v.first().map(|&x| u32::from(x)),
        Value::Long(v) | Value::Ifd(v) => v.first().copied(),
        _ => None,
    }
}

/// Borrows `len` bytes at `offset` of `data`, or reports the out-of-bounds
/// access as [`ParseError::OffsetOutOfBounds`].
pub(crate) fn read_range(data: &[u8], offset: u64, len: usize) -> RawResult<&[u8]> {
    usize::try_from(offset)
        .ok()
        .and_then(|start| data.get(start..start.checked_add(len)?))
        .ok_or(RawError::Parse(ParseError::OffsetOutOfBounds {
            offset,
            size: len as u64,
            file_size: data.len() as u64,
        }))
}

/// Extracts the embedded JPEG thumbnail referenced by IFD0's
/// `JPEGInterchangeFormat`/`JPEGInterchangeFormatLength` pair, if present.
pub(crate) fn jpeg_thumbnail(data: &[u8], ifd0: &Ifd) -> RawResult<Option<Vec<u8>>> {
    let Some(offset) = first_u32(ifd0, tags::JPEG_INTERCHANGE_FORMAT) else {
        return Ok(None);
    };
    let Some(length) = first_u32(ifd0, tags::JPEG_INTERCHANGE_FORMAT_LENGTH) else {
        return Ok(None);
    };
    if length == 0 {
        return Ok(None);
    }
    Ok(Some(
        read_range(data, u64::from(offset), length as usize)?.to_vec(),
    ))
}

// ============================================================================
// Primitive tag readers (mirroring `tiff::metadata_helper`)
// ============================================================================

fn u16_tag(ifd: &Ifd, tag: u16) -> Option<u16> {
    match ifd.get(tag)? {
        Value::Short(v) => v.first().copied(),
        Value::Long(v) => v.first().map(|&x| x as u16),
        Value::Byte(v) => v.first().map(|&x| u16::from(x)),
        _ => None,
    }
}

fn u8_tag(ifd: &Ifd, tag: u16) -> Option<u8> {
    match ifd.get(tag)? {
        Value::Byte(v) => v.first().copied(),
        Value::Short(v) => v.first().map(|&x| x as u8),
        _ => None,
    }
}

fn urational_tag(ifd: &Ifd, tag: u16) -> Option<URational> {
    ifd.get(tag)
        .and_then(Value::as_rationals)
        .and_then(|v| v.first())
        .map(|&(n, d)| URational::new(n, d))
}

fn srational_tag(ifd: &Ifd, tag: u16) -> Option<SRational> {
    ifd.get(tag)
        .and_then(Value::as_srationals)
        .and_then(|v| v.first())
        .map(|&(n, d)| SRational::new(n, d))
}

fn urational3_tag(ifd: &Ifd, tag: u16) -> Option<[URational; 3]> {
    ifd.get(tag)
        .and_then(Value::as_rationals)
        .filter(|v| v.len() >= 3)
        .map(|v| {
            [
                URational::new(v[0].0, v[0].1),
                URational::new(v[1].0, v[1].1),
                URational::new(v[2].0, v[2].1),
            ]
        })
}

// ============================================================================
// High-level metadata extractors (mirroring `tiff::metadata_helper`)
// ============================================================================

/// Extracts EXIF exposure/capture settings from IFD0's EXIF sub-IFD.
pub(crate) fn extract_exif(ifd0: &Ifd) -> ExifInfo {
    let Some(exif) = exif_ifd(ifd0) else {
        return ExifInfo::default();
    };

    ExifInfo {
        iso: u16_tag(exif, tags::ISO_SPEED_RATINGS).map(u32::from),
        exposure_time: urational_tag(exif, tags::EXPOSURE_TIME),
        f_number: urational_tag(exif, tags::F_NUMBER),
        focal_length: urational_tag(exif, tags::FOCAL_LENGTH),
        focal_length_35mm: u16_tag(exif, tags::FOCAL_LENGTH_IN_35MM_FILM),
        exposure_program: u16_tag(exif, tags::EXPOSURE_PROGRAM),
        metering_mode: u16_tag(exif, tags::METERING_MODE),
        flash: u16_tag(exif, tags::FLASH),
        exposure_compensation: srational_tag(exif, tags::EXPOSURE_BIAS_VALUE),
        max_aperture: urational_tag(exif, tags::MAX_APERTURE_VALUE),
        brightness_value: srational_tag(exif, tags::BRIGHTNESS_VALUE),
    }
}

/// Extracts date/time information from IFD0 and its EXIF sub-IFD.
pub(crate) fn extract_datetime(ifd0: &Ifd) -> DateTimeInfo {
    let modify_date = ascii_tag(ifd0, tags::DATE_TIME);
    let exif = exif_ifd(ifd0);

    let datetime_original = exif.and_then(|ifd| ascii_tag(ifd, tags::DATE_TIME_ORIGINAL));
    let create_date = exif.and_then(|ifd| ascii_tag(ifd, tags::DATE_TIME_DIGITIZED));
    let offset_time = exif.and_then(|ifd| {
        ascii_tag(ifd, tags::OFFSET_TIME_ORIGINAL).or_else(|| ascii_tag(ifd, tags::OFFSET_TIME))
    });
    let subsec_time = exif.and_then(|ifd| {
        ascii_tag(ifd, tags::SUB_SEC_TIME_ORIGINAL).or_else(|| ascii_tag(ifd, tags::SUB_SEC_TIME))
    });

    DateTimeInfo {
        datetime_original,
        create_date,
        modify_date,
        offset_time,
        subsec_time,
    }
}

/// Extracts GPS location data from IFD0's GPS sub-IFD.
pub(crate) fn extract_gps(ifd0: &Ifd) -> GpsInfo {
    let Some(gps) = gps_ifd(ifd0) else {
        return GpsInfo::default();
    };

    GpsInfo {
        latitude: urational3_tag(gps, tags::GPS_LATITUDE),
        latitude_ref: ascii_tag(gps, tags::GPS_LATITUDE_REF).and_then(|s| s.chars().next()),
        longitude: urational3_tag(gps, tags::GPS_LONGITUDE),
        longitude_ref: ascii_tag(gps, tags::GPS_LONGITUDE_REF).and_then(|s| s.chars().next()),
        altitude: urational_tag(gps, tags::GPS_ALTITUDE),
        altitude_ref: u8_tag(gps, tags::GPS_ALTITUDE_REF),
        timestamp: urational3_tag(gps, tags::GPS_TIME_STAMP),
        datestamp: ascii_tag(gps, tags::GPS_DATE_STAMP),
        speed: urational_tag(gps, tags::GPS_SPEED),
        img_direction: urational_tag(gps, tags::GPS_IMG_DIRECTION),
    }
}

/// Extracts lens make/model from IFD0's EXIF sub-IFD.
pub(crate) fn extract_lens_info(ifd0: &Ifd) -> (Option<String>, Option<String>) {
    let Some(exif) = exif_ifd(ifd0) else {
        return (None, None);
    };
    (
        ascii_tag(exif, tags::LENS_MAKE),
        ascii_tag(exif, tags::LENS_MODEL),
    )
}

/// Extracts the orientation (1-8) from IFD0.
pub(crate) fn extract_orientation(ifd0: &Ifd) -> Option<u16> {
    u16_tag(ifd0, tags::ORIENTATION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamut_ifd::{TiffFile, write};

    fn build_tree() -> Vec<u8> {
        let mut exif = Ifd::new();
        exif.set(tags::ISO_SPEED_RATINGS, Value::Short(vec![800]));
        exif.set(tags::EXPOSURE_TIME, Value::Rational(vec![(1, 250)]));
        exif.set(tags::LENS_MODEL, Value::Ascii("E 35mm F1.8 OSS".to_owned()));

        let mut raw = Ifd::new();
        raw.set(tags::IMAGE_WIDTH, Value::Short(vec![6656]));
        raw.set(tags::IMAGE_LENGTH, Value::Short(vec![4608]));
        raw.set(tags::PHOTOMETRIC_INTERPRETATION, Value::Short(vec![32803]));
        raw.set(tags::BITS_PER_SAMPLE, Value::Short(vec![14]));

        let mut ifd0 = Ifd::new();
        ifd0.set(tags::MAKE, Value::Ascii("SONY".to_owned()));
        ifd0.set(tags::ORIENTATION, Value::Short(vec![6]));
        ifd0.set(
            tags::DATE_TIME,
            Value::Ascii("2023:05:01 12:00:00".to_owned()),
        );
        ifd0.set_sub_ifd(tags::SUB_IFDS, vec![raw]);
        ifd0.set_sub_ifd(tags::EXIF_IFD_POINTER, vec![exif]);

        write(&TiffFile {
            order: ByteOrder::LittleEndian,
            variant: Variant::Classic,
            ifds: vec![ifd0],
        })
        .expect("write")
    }

    #[test]
    fn parse_tree_resolves_sub_ifds_and_exif() {
        let data = build_tree();
        let tree = parse_tree(&data, "test").expect("parse");
        assert_eq!(tree.order, ByteOrder::LittleEndian);
        assert_eq!(tree.ifds.len(), 1);

        let ifd0 = &tree.ifds[0];
        // The pointer fields were consumed into groups.
        assert!(ifd0.get(tags::SUB_IFDS).is_none());
        assert!(ifd0.get(tags::EXIF_IFD_POINTER).is_none());

        let raw = &sub_ifd_group(ifd0, tags::SUB_IFDS)[0];
        assert_eq!(first_u32(raw, tags::IMAGE_WIDTH), Some(6656));
        assert_eq!(
            raw.get(tags::PHOTOMETRIC_INTERPRETATION)
                .and_then(Value::as_u32),
            Some(32803)
        );

        let exif = exif_ifd(ifd0).expect("exif sub-IFD");
        assert_eq!(u16_tag(exif, tags::ISO_SPEED_RATINGS), Some(800));
    }

    #[test]
    fn parse_tree_tolerates_malformed_sub_ifd() {
        // IFD0 whose SubIFDs pointer aims past the end of the stream: the
        // legacy parser skipped the child with a warning, and so does this.
        let mut ifd0 = Ifd::new();
        ifd0.set(tags::MAKE, Value::Ascii("SONY".to_owned()));
        ifd0.set(tags::SUB_IFDS, Value::Long(vec![0xFFFF_0000]));
        let data = write(&TiffFile {
            order: ByteOrder::LittleEndian,
            variant: Variant::Classic,
            ifds: vec![ifd0],
        })
        .expect("write");

        let tree = parse_tree(&data, "test").expect("parse survives bad child");
        assert!(sub_ifd_group(&tree.ifds[0], tags::SUB_IFDS).is_empty());
    }

    #[test]
    fn metadata_extractors_match_legacy_shapes() {
        let data = build_tree();
        let tree = parse_tree(&data, "test").expect("parse");
        let ifd0 = &tree.ifds[0];

        let exif = extract_exif(ifd0);
        assert_eq!(exif.iso, Some(800));
        assert_eq!(exif.exposure_time, Some(URational::new(1, 250)));

        let (lens_make, lens_model) = extract_lens_info(ifd0);
        assert_eq!(lens_make, None);
        assert_eq!(lens_model.as_deref(), Some("E 35mm F1.8 OSS"));

        assert_eq!(extract_orientation(ifd0), Some(6));
        let dt = extract_datetime(ifd0);
        assert_eq!(dt.modify_date.as_deref(), Some("2023:05:01 12:00:00"));

        // No GPS group: defaults, not a failure.
        let gps = extract_gps(ifd0);
        assert_eq!(gps.latitude, None);
    }

    #[test]
    fn clean_ascii_matches_legacy_trimming() {
        assert_eq!(clean_ascii("SONY\0\0"), "SONY");
        assert_eq!(clean_ascii("  ILCE-6700 \0"), "ILCE-6700");
        assert_eq!(clean_ascii("plain"), "plain");
    }

    #[test]
    fn read_range_bounds_checks() {
        let data = [1u8, 2, 3, 4];
        assert_eq!(read_range(&data, 1, 2).expect("in bounds"), &[2, 3]);
        assert!(read_range(&data, 3, 2).is_err());
        assert!(read_range(&data, u64::MAX, 1).is_err());
    }

    #[test]
    fn jpeg_thumbnail_reads_offset_length_pair() {
        // A fake "file": 8 header-ish bytes then a 4-byte JPEG-ish payload.
        let mut data = vec![0u8; 8];
        data.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xD9]);
        let mut ifd0 = Ifd::new();
        ifd0.set(tags::JPEG_INTERCHANGE_FORMAT, Value::Long(vec![8]));
        ifd0.set(tags::JPEG_INTERCHANGE_FORMAT_LENGTH, Value::Long(vec![4]));
        let thumb = jpeg_thumbnail(&data, &ifd0).expect("read");
        assert_eq!(thumb, Some(vec![0xFF, 0xD8, 0xFF, 0xD9]));

        // Absent tags or a zero length are None, not an error.
        assert_eq!(jpeg_thumbnail(&data, &Ifd::new()).expect("ok"), None);
        ifd0.set(tags::JPEG_INTERCHANGE_FORMAT_LENGTH, Value::Long(vec![0]));
        assert_eq!(jpeg_thumbnail(&data, &ifd0).expect("ok"), None);

        // An out-of-bounds pair is a typed error.
        ifd0.set(tags::JPEG_INTERCHANGE_FORMAT_LENGTH, Value::Long(vec![64]));
        assert!(jpeg_thumbnail(&data, &ifd0).is_err());
    }
}
