//! EXIF metadata parsing, generation, and embedding.
//!
//! Builds and parses EXIF blobs with `gamut-exif` (the upstream home for the
//! EXIF model — see the Upstream-First Policy) and converts them to and from
//! [`ImageMetadata`]. Container-level concerns stay on this side only for the
//! formats whose codec has not yet migrated to gamut: AVIF embedding goes
//! through the crate's ISOBMFF box splicing ([`crate::metadata::isobmff`]),
//! and the decode-side blob *location* (`eXIf` chunk, `EXIF` chunk, `Exif`
//! item) is scanned here. JPEG APP segments are read and written by
//! `gamut-jpeg` itself (`gamut_jpeg::metadata` / `JpegEncoder::with_exif`);
//! the remaining container surgery migrates behind the gamut codec boundaries
//! with the per-format codec issues.

use crate::core::metadata::ImageMetadata;
use gamut_exif::{ByteOrder, Exif, ExifTag, ExifWriter, Ifd, Value};

/// Error type for EXIF operations.
#[derive(Debug)]
pub enum ExifError {
    /// Failed to serialize EXIF data
    Serialization(String),
    /// Failed to manipulate image container
    Container(String),
}

impl std::fmt::Display for ExifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExifError::Serialization(msg) => write!(f, "EXIF serialization error: {}", msg),
            ExifError::Container(msg) => write!(f, "Container manipulation error: {}", msg),
        }
    }
}

impl std::error::Error for ExifError {}

impl From<std::io::Error> for ExifError {
    fn from(e: std::io::Error) -> Self {
        ExifError::Container(e.to_string())
    }
}

/// Builder for EXIF metadata from `ImageMetadata`.
///
/// Creates EXIF data that can be embedded in JPEG, WebP, and other formats.
pub struct ExifBuilder<'a> {
    metadata: &'a ImageMetadata,
}

impl<'a> ExifBuilder<'a> {
    /// Create a new ExifBuilder from ImageMetadata.
    pub fn new(metadata: &'a ImageMetadata) -> Self {
        Self { metadata }
    }

    /// Build an EXIF model using `gamut-exif`.
    pub fn build(&self) -> Exif {
        let mut exif = Exif::new(ByteOrder::LittleEndian);

        // Camera info
        if !self.metadata.camera.make.is_empty() {
            exif.set_tag(
                ExifTag::Make,
                Value::Ascii(self.metadata.camera.make.clone()),
            );
        }
        if !self.metadata.camera.model.is_empty() {
            exif.set_tag(
                ExifTag::Model,
                Value::Ascii(self.metadata.camera.model.clone()),
            );
        }
        if let Some(ref lens_make) = self.metadata.camera.lens_make {
            exif.set_tag(ExifTag::LensMake, Value::Ascii(lens_make.clone()));
        }
        if let Some(ref lens_model) = self.metadata.camera.lens_model {
            exif.set_tag(ExifTag::LensModel, Value::Ascii(lens_model.clone()));
        }
        if let Some(ref serial) = self.metadata.camera.serial_number {
            exif.set_tag(ExifTag::BodySerialNumber, Value::Ascii(serial.clone()));
        }

        // EXIF exposure info
        if let Some(iso) = self.metadata.exif.iso {
            let value = match u16::try_from(iso) {
                Ok(short) => Value::Short(vec![short]),
                Err(_) => Value::Long(vec![iso]),
            };
            exif.set_tag(ExifTag::PhotographicSensitivity, value);
        }
        if let Some(r) = self.metadata.exif.exposure_time {
            exif.set_tag(
                ExifTag::ExposureTime,
                Value::Rational(vec![(r.numerator, r.denominator)]),
            );
        }
        if let Some(r) = self.metadata.exif.f_number {
            exif.set_tag(
                ExifTag::FNumber,
                Value::Rational(vec![(r.numerator, r.denominator)]),
            );
        }
        if let Some(r) = self.metadata.exif.focal_length {
            exif.set_tag(
                ExifTag::FocalLength,
                Value::Rational(vec![(r.numerator, r.denominator)]),
            );
        }
        if let Some(fl_35mm) = self.metadata.exif.focal_length_35mm {
            exif.set_tag(ExifTag::FocalLengthIn35mmFilm, Value::Short(vec![fl_35mm]));
        }
        if let Some(program) = self.metadata.exif.exposure_program {
            exif.set_tag(ExifTag::ExposureProgram, Value::Short(vec![program]));
        }
        if let Some(metering) = self.metadata.exif.metering_mode {
            exif.set_tag(ExifTag::MeteringMode, Value::Short(vec![metering]));
        }
        if let Some(flash) = self.metadata.exif.flash {
            exif.set_tag(ExifTag::Flash, Value::Short(vec![flash]));
        }
        if let Some(r) = self.metadata.exif.exposure_compensation {
            exif.set_tag(
                ExifTag::ExposureBiasValue,
                Value::SRational(vec![(r.numerator, r.denominator)]),
            );
        }
        if let Some(r) = self.metadata.exif.max_aperture {
            exif.set_tag(
                ExifTag::MaxApertureValue,
                Value::Rational(vec![(r.numerator, r.denominator)]),
            );
        }
        if let Some(r) = self.metadata.exif.brightness_value {
            exif.set_tag(
                ExifTag::BrightnessValue,
                Value::SRational(vec![(r.numerator, r.denominator)]),
            );
        }

        // Date/time info
        if let Some(ref dt) = self.metadata.datetime.datetime_original {
            exif.set_tag(ExifTag::DateTimeOriginal, Value::Ascii(dt.clone()));
        }
        if let Some(ref dt) = self.metadata.datetime.create_date {
            exif.set_tag(ExifTag::DateTimeDigitized, Value::Ascii(dt.clone()));
        }
        if let Some(ref dt) = self.metadata.datetime.modify_date {
            exif.set_tag(ExifTag::DateTime, Value::Ascii(dt.clone()));
        }
        if let Some(ref offset) = self.metadata.datetime.offset_time {
            exif.set_tag(ExifTag::OffsetTime, Value::Ascii(offset.clone()));
        }
        if let Some(ref subsec) = self.metadata.datetime.subsec_time {
            exif.set_tag(ExifTag::SubSecTime, Value::Ascii(subsec.clone()));
        }

        // GPS info
        self.build_gps(&mut exif);

        // Image info
        if let Some(orient) = self.metadata.image.orientation {
            exif.set_tag(ExifTag::Orientation, Value::Short(vec![orient]));
        }

        exif
    }

    /// Build GPS-related EXIF tags.
    fn build_gps(&self, exif: &mut Exif) {
        let gps = &self.metadata.gps;
        let triple = |v: &[crate::core::metadata::URational; 3]| {
            Value::Rational(vec![
                (v[0].numerator, v[0].denominator),
                (v[1].numerator, v[1].denominator),
                (v[2].numerator, v[2].denominator),
            ])
        };

        if let Some(ref lat) = gps.latitude {
            exif.set_tag(ExifTag::GpsLatitude, triple(lat));
        }
        if let Some(lat_ref) = gps.latitude_ref {
            exif.set_tag(ExifTag::GpsLatitudeRef, Value::Ascii(lat_ref.to_string()));
        }
        if let Some(ref lon) = gps.longitude {
            exif.set_tag(ExifTag::GpsLongitude, triple(lon));
        }
        if let Some(lon_ref) = gps.longitude_ref {
            exif.set_tag(ExifTag::GpsLongitudeRef, Value::Ascii(lon_ref.to_string()));
        }
        if let Some(r) = gps.altitude {
            exif.set_tag(
                ExifTag::GpsAltitude,
                Value::Rational(vec![(r.numerator, r.denominator)]),
            );
        }
        if let Some(alt_ref) = gps.altitude_ref {
            exif.set_tag(ExifTag::GpsAltitudeRef, Value::Byte(vec![alt_ref]));
        }
        if let Some(ref timestamp) = gps.timestamp {
            exif.set_tag(ExifTag::GpsTimeStamp, triple(timestamp));
        }
        if let Some(ref datestamp) = gps.datestamp {
            exif.set_tag(ExifTag::GpsDateStamp, Value::Ascii(datestamp.clone()));
        }
        if let Some(r) = gps.speed {
            exif.set_tag(
                ExifTag::GpsSpeed,
                Value::Rational(vec![(r.numerator, r.denominator)]),
            );
        }
        if let Some(r) = gps.img_direction {
            exif.set_tag(
                ExifTag::GpsImgDirection,
                Value::Rational(vec![(r.numerator, r.denominator)]),
            );
        }
    }

    /// Build raw TIFF-level EXIF bytes (no APP1 wrapper or `Exif\0\0` prefix).
    ///
    /// These bytes are the form the gamut encoders take (`with_exif` on the
    /// JPEG/PNG/JXL encoders wraps them format-specifically). For WebP,
    /// prepend `b"Exif\0\0"` before passing to the muxer.
    pub fn build_bytes(&self) -> Result<Vec<u8>, ExifError> {
        let exif = self.build();
        ExifWriter::new()
            .marker(false)
            .write(&exif)
            .map_err(|e| ExifError::Serialization(e.to_string()))
    }

    /// Append EXIF metadata to an in-memory AVIF byte stream.
    ///
    /// AVIF uses the HEIF/ISOBMFF container; the EXIF payload is stored as an
    /// `Exif` item (an `ExifDataBlock`: a 4-byte TIFF-header offset followed by
    /// the TIFF stream) and wired into `iinf`/`iloc`/`iref` — see
    /// [`crate::metadata::isobmff::insert_item`].
    #[cfg_attr(not(feature = "avif"), allow(dead_code))]
    pub fn append_to_avif(&self, avif_data: Vec<u8>) -> Result<Vec<u8>, ExifError> {
        let tiff_bytes = self.build_bytes()?;
        // ExifDataBlock (ISO 23008-12): exif_tiff_header_offset then the payload.
        // The offset is 0 because the payload is a bare TIFF stream.
        let mut payload = Vec::with_capacity(4 + tiff_bytes.len());
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&tiff_bytes);
        crate::metadata::isobmff::insert_item(avif_data, *b"Exif", &payload)
            .map_err(|e| ExifError::Container(format!("AVIF EXIF embedding failed: {e}")))
    }
}

// ── ExifParser ────────────────────────────────────────────────────────────────

/// The container an EXIF blob is located in before parsing.
///
/// Selects the decode-side blob-location strategy of
/// [`ExifParser::parse_from_bytes`]: which segment/chunk/item of the file
/// carries the EXIF TIFF stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExifContainer {
    /// PNG — `eXIf` chunk.
    Png,
    /// TIFF — the whole file is the TIFF stream.
    Tiff,
    /// WebP — RIFF `EXIF` chunk.
    WebP,
    /// AVIF — HEIF/ISOBMFF `Exif` item.
    Avif,
}

/// Parses EXIF metadata from image file bytes into an [`ImageMetadata`].
///
/// Locates the EXIF blob in the container ([`ExifContainer`]) and parses it
/// with `gamut-exif`.
pub struct ExifParser;

impl ExifParser {
    /// Read EXIF from `file_data` and convert to [`ImageMetadata`].
    ///
    /// Returns a default (empty) [`ImageMetadata`] if the file has no EXIF or
    /// the container/blob is malformed.
    pub fn parse_from_bytes(file_data: &[u8], container: ExifContainer) -> ImageMetadata {
        let blob = match container {
            ExifContainer::Png => extract_exif_from_png(file_data),
            ExifContainer::Tiff => Some(file_data.to_vec()),
            ExifContainer::WebP => extract_exif_from_webp(file_data),
            ExifContainer::Avif => extract_exif_from_avif(file_data),
        };
        match blob {
            Some(blob) => Self::parse_exif_blob(&blob),
            None => ImageMetadata::default(),
        }
    }

    /// Parse a raw EXIF blob (a TIFF stream, with or without the `Exif\0\0`
    /// marker) into [`ImageMetadata`].
    ///
    /// Returns a default (empty) [`ImageMetadata`] if the blob is malformed.
    pub fn parse_exif_blob(blob: &[u8]) -> ImageMetadata {
        match Exif::parse(blob) {
            Ok(exif) => Self::parse_metadata(&exif),
            Err(_) => ImageMetadata::default(),
        }
    }

    /// Convert an already-parsed [`gamut_exif::Exif`] into [`ImageMetadata`].
    pub fn parse_metadata(exif: &Exif) -> ImageMetadata {
        use crate::core::metadata::*;

        let mut md = ImageMetadata::default();

        // ── Camera info ───────────────────────────────────────────────────────
        if let Some(s) = exif.get_tag(ExifTag::Make).and_then(value_text) {
            md.camera.make = s;
        }
        if let Some(s) = exif.get_tag(ExifTag::Model).and_then(value_text) {
            md.camera.model = s;
        }
        md.camera.lens_make = nonempty_text(exif, ExifTag::LensMake);
        md.camera.lens_model = nonempty_text(exif, ExifTag::LensModel);
        md.camera.serial_number = nonempty_text(exif, ExifTag::BodySerialNumber);

        // ── EXIF exposure settings ────────────────────────────────────────────
        md.exif.iso = exif
            .get_tag(ExifTag::PhotographicSensitivity)
            .and_then(first_u32);
        md.exif.exposure_time = exif
            .get_tag(ExifTag::ExposureTime)
            .and_then(first_urational);
        md.exif.f_number = exif.get_tag(ExifTag::FNumber).and_then(first_urational);
        md.exif.focal_length = exif.get_tag(ExifTag::FocalLength).and_then(first_urational);
        md.exif.focal_length_35mm = exif
            .get_tag(ExifTag::FocalLengthIn35mmFilm)
            .and_then(first_u16);
        md.exif.exposure_program = exif.get_tag(ExifTag::ExposureProgram).and_then(first_u16);
        md.exif.metering_mode = exif.get_tag(ExifTag::MeteringMode).and_then(first_u16);
        md.exif.flash = exif.get_tag(ExifTag::Flash).and_then(first_u16);
        md.exif.exposure_compensation = exif
            .get_tag(ExifTag::ExposureBiasValue)
            .and_then(first_srational);
        md.exif.max_aperture = exif
            .get_tag(ExifTag::MaxApertureValue)
            .and_then(first_urational);
        md.exif.brightness_value = exif
            .get_tag(ExifTag::BrightnessValue)
            .and_then(first_srational);

        // ── Date/time ─────────────────────────────────────────────────────────
        md.datetime.datetime_original = nonempty_text(exif, ExifTag::DateTimeOriginal);
        md.datetime.create_date = nonempty_text(exif, ExifTag::DateTimeDigitized);
        md.datetime.modify_date = nonempty_text(exif, ExifTag::DateTime);
        md.datetime.offset_time = nonempty_text(exif, ExifTag::OffsetTime);
        md.datetime.subsec_time = nonempty_text(exif, ExifTag::SubSecTime);

        // ── GPS ───────────────────────────────────────────────────────────────
        // Positioning tags come from the typed view; the remaining tags the
        // typed view does not model are read from the GPS sub-IFD directly.
        if let Some(gps) = exif.gps() {
            let to_ur = |r: gamut_exif::Rational| URational::new(r.num, r.den);
            if let Some(lat) = gps.latitude {
                md.gps.latitude =
                    Some([to_ur(lat.degrees), to_ur(lat.minutes), to_ur(lat.seconds)]);
                md.gps.latitude_ref = Some(gps_reference_char(lat.reference));
            }
            if let Some(lon) = gps.longitude {
                md.gps.longitude =
                    Some([to_ur(lon.degrees), to_ur(lon.minutes), to_ur(lon.seconds)]);
                md.gps.longitude_ref = Some(gps_reference_char(lon.reference));
            }
            if let Some(alt) = gps.altitude {
                md.gps.altitude = Some(to_ur(alt.meters));
                md.gps.altitude_ref = Some(u8::from(alt.below_sea_level));
            }
        }
        if let Some(gps_ifd) = exif.gps_ifd() {
            if let Some(Value::Rational(v)) = gps_ifd.get(ExifTag::GpsTimeStamp.tag_id())
                && v.len() >= 3
            {
                md.gps.timestamp = Some([
                    URational::new(v[0].0, v[0].1),
                    URational::new(v[1].0, v[1].1),
                    URational::new(v[2].0, v[2].1),
                ]);
            }
            md.gps.datestamp = gps_ifd
                .get(ExifTag::GpsDateStamp.tag_id())
                .and_then(value_text)
                .filter(|s| !s.is_empty());
            md.gps.speed = gps_ifd
                .get(ExifTag::GpsSpeed.tag_id())
                .and_then(first_urational);
            md.gps.img_direction = gps_ifd
                .get(ExifTag::GpsImgDirection.tag_id())
                .and_then(first_urational);
        }

        // ── Image info ────────────────────────────────────────────────────────
        md.image.orientation = exif.orientation();

        // ── Generic tag table ─────────────────────────────────────────────────
        // Mirror every EXIF tag into the typed `extra` table so nothing is lost,
        // even tags the curated fields above do not model.
        Self::populate_extra(exif, &mut md);

        md
    }

    /// Populate [`ImageMetadata::extra`] with a typed mirror of every EXIF tag.
    fn populate_extra(exif: &Exif, md: &mut crate::core::metadata::ImageMetadata) {
        use crate::core::metadata::{MetadataKey, MetadataNamespace};

        let directories: [(MetadataNamespace, Option<&Ifd>); 5] = [
            (MetadataNamespace::Exif, Some(exif.image())),
            (MetadataNamespace::Exif, exif.exif_ifd()),
            (MetadataNamespace::Gps, exif.gps_ifd()),
            (MetadataNamespace::Exif, exif.interop_ifd()),
            (MetadataNamespace::Exif, exif.thumbnail_ifd()),
        ];
        for (namespace, ifd) in directories {
            let Some(ifd) = ifd else { continue };
            for field in ifd.fields() {
                let key = MetadataKey::new(namespace, format!("0x{:04x}", field.tag));
                md.insert(key, exif_value_to_metadata(&field.value));
            }
        }
    }
}

/// `N`/`S`/`E`/`W` for a typed GPS hemisphere reference.
fn gps_reference_char(reference: gamut_exif::GpsReference) -> char {
    match reference {
        gamut_exif::GpsReference::North => 'N',
        gamut_exif::GpsReference::South => 'S',
        gamut_exif::GpsReference::East => 'E',
        gamut_exif::GpsReference::West => 'W',
    }
}

/// A text value with trailing NUL padding stripped.
fn value_text(value: &Value) -> Option<String> {
    value.as_str().map(|s| s.trim_end_matches('\0').to_string())
}

/// A non-empty text value of `tag`, NUL padding stripped.
fn nonempty_text(exif: &Exif, tag: ExifTag) -> Option<String> {
    exif.get_tag(tag)
        .and_then(value_text)
        .filter(|s| !s.is_empty())
}

/// The first element of an unsigned-integer value, as `u32`.
fn first_u32(value: &Value) -> Option<u32> {
    match value {
        Value::Byte(v) => v.first().map(|&b| u32::from(b)),
        Value::Short(v) => v.first().map(|&s| u32::from(s)),
        Value::Long(v) | Value::Ifd(v) => v.first().copied(),
        _ => None,
    }
}

/// The first element of an unsigned-integer value, as `u16`.
fn first_u16(value: &Value) -> Option<u16> {
    first_u32(value).and_then(|v| u16::try_from(v).ok())
}

/// The first element of a `RATIONAL` value, as a [`URational`].
fn first_urational(value: &Value) -> Option<crate::core::metadata::URational> {
    match value {
        Value::Rational(v) => v
            .first()
            .map(|&(n, d)| crate::core::metadata::URational::new(n, d)),
        _ => None,
    }
}

/// The first element of an `SRATIONAL` value, as an [`SRational`].
fn first_srational(value: &Value) -> Option<crate::core::metadata::SRational> {
    match value {
        Value::SRational(v) => v
            .first()
            .map(|&(n, d)| crate::core::metadata::SRational::new(n, d)),
        _ => None,
    }
}

/// Convert a single `gamut_ifd::Value` into a typed [`MetadataValue`].
///
/// Single-element values collapse to a scalar; multi-element values become a
/// [`MetadataValue::Array`].
fn exif_value_to_metadata(value: &Value) -> crate::core::metadata::MetadataValue {
    use crate::core::metadata::{MetadataValue, SRational, URational};

    fn collapse(mut vals: Vec<MetadataValue>) -> MetadataValue {
        if vals.len() == 1 {
            vals.pop().unwrap()
        } else {
            MetadataValue::Array(vals)
        }
    }

    match value {
        Value::Ascii(s) | Value::Utf8(s) => {
            MetadataValue::Text(s.trim_end_matches('\0').to_string())
        }
        Value::Undefined(b) => MetadataValue::Bytes(b.clone()),
        Value::Byte(v) => collapse(
            v.iter()
                .map(|&b| MetadataValue::U64(u64::from(b)))
                .collect(),
        ),
        Value::SByte(v) => collapse(
            v.iter()
                .map(|&b| MetadataValue::I64(i64::from(b)))
                .collect(),
        ),
        Value::Short(v) => collapse(
            v.iter()
                .map(|&s| MetadataValue::U64(u64::from(s)))
                .collect(),
        ),
        Value::SShort(v) => collapse(
            v.iter()
                .map(|&s| MetadataValue::I64(i64::from(s)))
                .collect(),
        ),
        Value::Long(v) | Value::Ifd(v) => collapse(
            v.iter()
                .map(|&l| MetadataValue::U64(u64::from(l)))
                .collect(),
        ),
        Value::SLong(v) => collapse(
            v.iter()
                .map(|&l| MetadataValue::I64(i64::from(l)))
                .collect(),
        ),
        Value::Float(v) => collapse(
            v.iter()
                .map(|&f| MetadataValue::F64(f64::from(f)))
                .collect(),
        ),
        Value::Double(v) => collapse(v.iter().map(|&f| MetadataValue::F64(f)).collect()),
        Value::Rational(v) => collapse(
            v.iter()
                .map(|&(n, d)| MetadataValue::URational(URational::new(n, d)))
                .collect(),
        ),
        Value::SRational(v) => collapse(
            v.iter()
                .map(|&(n, d)| MetadataValue::SRational(SRational::new(n, d)))
                .collect(),
        ),
        // BigTIFF 64-bit types: these `gamut_ifd::Value` variants exist only
        // when *something* in the build graph enables gamut-ifd's `bigtiff`
        // (the RAW decoders' `ifd-parser` feature, or the dev-dependency used
        // by integration tests — Cargo unifies features across the graph, so
        // a cfg on our own feature cannot track their presence). EXIF streams
        // themselves are classic TIFF, so these arms are effectively dead
        // here; they exist for exhaustiveness under every feature unification.
        #[cfg(feature = "ifd-parser")]
        Value::Long8(v) | Value::Ifd8(v) => {
            collapse(v.iter().map(|&l| MetadataValue::U64(l)).collect())
        }
        #[cfg(feature = "ifd-parser")]
        Value::SLong8(v) => collapse(v.iter().map(|&l| MetadataValue::I64(l)).collect()),
        // An entry whose field type is unrecognised: keep the verbatim
        // value/offset word so nothing is silently dropped. The wildcard also
        // absorbs the bigtiff variants when they exist but `ifd-parser` is off
        // (see above) — without it, that feature unification fails to compile.
        #[allow(unreachable_patterns)]
        other => match other {
            Value::Unknown(u) => MetadataValue::Bytes(u.word().to_vec()),
            _ => MetadataValue::Bytes(Vec::new()),
        },
    }
}

// ── Container-side EXIF blob location ─────────────────────────────────────────
//
// These scanners only *locate* the EXIF payload inside a container; parsing is
// gamut-exif's job. They migrate behind the gamut codec boundaries (codec-side
// `MetadataBlock`) with the per-format codec migrations. (JPEG already did:
// `gamut_jpeg::metadata` locates and strips the APP1/APP2 payloads.)

/// Extract the payload of a PNG `eXIf` chunk (a bare TIFF stream).
fn extract_exif_from_png(data: &[u8]) -> Option<Vec<u8>> {
    const PNG_SIG: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if data.get(..8) != Some(PNG_SIG) {
        return None;
    }
    let mut pos = 8usize;
    while let Some(header) = data.get(pos..pos + 8) {
        let len = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let chunk_type = &header[4..8];
        let payload = data.get(pos + 8..pos + 8 + len)?;
        if chunk_type == b"eXIf" {
            return Some(payload.to_vec());
        }
        if chunk_type == b"IEND" {
            return None;
        }
        pos += 8 + len + 4; // header + data + CRC
    }
    None
}

/// Extract the payload of a WebP RIFF `EXIF` chunk.
fn extract_exif_from_webp(data: &[u8]) -> Option<Vec<u8>> {
    if data.get(..4) != Some(b"RIFF") || data.get(8..12) != Some(b"WEBP") {
        return None;
    }
    let riff_len = u32::from_le_bytes(data.get(4..8)?.try_into().unwrap()) as usize;
    let end = (8 + riff_len).min(data.len());
    let mut pos = 12usize;
    while let Some(header) = data.get(pos..pos + 8) {
        if pos + 8 > end {
            return None;
        }
        let chunk_type = &header[..4];
        let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let payload = data.get(pos + 8..pos + 8 + len)?;
        if chunk_type == b"EXIF" {
            return Some(payload.to_vec());
        }
        pos += 8 + len + (len & 1); // chunks are padded to even sizes
    }
    None
}

/// Extract the EXIF TIFF stream of an AVIF `Exif` item.
fn extract_exif_from_avif(data: &[u8]) -> Option<Vec<u8>> {
    let payload = crate::metadata::isobmff::extract_item(data, *b"Exif")?;
    // ExifDataBlock: a 4-byte offset to the TIFF header, then the payload.
    let offset = u32::from_be_bytes(payload.get(..4)?.try_into().unwrap()) as usize;
    let blob = payload
        .get(4 + offset..)
        .or_else(|| payload.get(4..))?
        .to_vec();
    Some(blob)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::metadata::*;

    fn sample_metadata() -> ImageMetadata {
        ImageMetadata {
            camera: CameraInfo {
                make: "SONY".into(),
                model: "ILCE-6700".into(),
                lens_model: Some("E 18-135mm F3.5-5.6 OSS".into()),
                ..Default::default()
            },
            exif: ExifInfo {
                iso: Some(800),
                exposure_time: Some(URational::new(1, 250)),
                f_number: Some(URational::new(56, 10)),
                focal_length: Some(URational::new(35, 1)),
                focal_length_35mm: Some(52),
                exposure_program: Some(3), // Aperture priority
                metering_mode: Some(5),    // Pattern
                ..Default::default()
            },
            datetime: DateTimeInfo {
                datetime_original: Some("2025:12:01 14:30:00".into()),
                ..Default::default()
            },
            gps: GpsInfo {
                latitude: Some([
                    URational::new(40, 1),
                    URational::new(44, 1),
                    URational::new(0, 1),
                ]),
                latitude_ref: Some('N'),
                longitude: Some([
                    URational::new(73, 1),
                    URational::new(59, 1),
                    URational::new(0, 1),
                ]),
                longitude_ref: Some('W'),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_exif_builder_creates_metadata() {
        let md = sample_metadata();
        let builder = ExifBuilder::new(&md);
        let exif = builder.build();

        assert_eq!(exif.make(), Some("SONY"));
        assert_eq!(exif.iso(), Some(800));
        assert!(exif.gps_ifd().is_some());
    }

    #[test]
    fn test_exif_builder_produces_bytes() {
        let md = sample_metadata();
        let builder = ExifBuilder::new(&md);
        let bytes = builder.build_bytes().expect("Should build EXIF bytes");

        // Should produce non-empty bytes
        assert!(!bytes.is_empty(), "EXIF bytes should not be empty");
    }

    #[test]
    fn test_empty_metadata_no_panic() {
        let md = ImageMetadata::default();
        let builder = ExifBuilder::new(&md);

        // gamut-exif serialises an empty model to a valid (empty-IFD) TIFF
        // stream — unlike little_exif, which panicked on this input.
        let bytes = builder.build_bytes().expect("empty EXIF should serialise");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_exif_builder_with_partial_data() {
        let md = ImageMetadata {
            camera: CameraInfo {
                make: "Apple".into(),
                model: "iPhone 17 Pro Max".into(),
                ..Default::default()
            },
            exif: ExifInfo {
                iso: Some(100),
                ..Default::default()
            },
            ..Default::default()
        };

        let builder = ExifBuilder::new(&md);
        let bytes = builder.build_bytes().expect("Should build EXIF bytes");

        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_build_parse_round_trip() {
        let md = sample_metadata();
        let bytes = ExifBuilder::new(&md).build_bytes().expect("build");
        let parsed = ExifParser::parse_exif_blob(&bytes);

        assert_eq!(parsed.camera.make, "SONY");
        assert_eq!(parsed.camera.model, "ILCE-6700");
        assert_eq!(
            parsed.camera.lens_model.as_deref(),
            Some("E 18-135mm F3.5-5.6 OSS")
        );
        assert_eq!(parsed.exif.iso, Some(800));
        assert_eq!(parsed.exif.exposure_time, Some(URational::new(1, 250)));
        assert_eq!(parsed.exif.f_number, Some(URational::new(56, 10)));
        assert_eq!(parsed.exif.focal_length_35mm, Some(52));
        assert_eq!(parsed.exif.exposure_program, Some(3));
        assert_eq!(parsed.exif.metering_mode, Some(5));
        assert_eq!(
            parsed.datetime.datetime_original.as_deref(),
            Some("2025:12:01 14:30:00")
        );
        assert_eq!(
            parsed.gps.latitude,
            Some([
                URational::new(40, 1),
                URational::new(44, 1),
                URational::new(0, 1),
            ])
        );
        assert_eq!(parsed.gps.latitude_ref, Some('N'));
        assert_eq!(parsed.gps.longitude_ref, Some('W'));
        // The generic table mirrors the tags too.
        assert!(
            parsed.get(MetadataNamespace::Exif, "0x010f").is_some(),
            "Make must be mirrored into `extra`"
        );
        assert!(
            parsed.get(MetadataNamespace::Gps, "0x0002").is_some(),
            "GPSLatitude must be mirrored into `extra`"
        );
    }

    #[test]
    fn test_parse_garbage_returns_default() {
        assert_eq!(
            ExifParser::parse_exif_blob(b"not a tiff stream"),
            ImageMetadata::default()
        );
        assert_eq!(
            ExifParser::parse_from_bytes(b"\x00\x01\x02\x03", ExifContainer::Png),
            ImageMetadata::default()
        );
    }

    #[test]
    fn test_extract_from_png_exif_chunk() {
        let md = sample_metadata();
        let tiff = ExifBuilder::new(&md).build_bytes().expect("bare tiff");

        let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        // IHDR (contents irrelevant to the scanner)
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0u8; 13]);
        png.extend_from_slice(&[0u8; 4]); // CRC
        // eXIf
        png.extend_from_slice(&(tiff.len() as u32).to_be_bytes());
        png.extend_from_slice(b"eXIf");
        png.extend_from_slice(&tiff);
        png.extend_from_slice(&[0u8; 4]); // CRC
        // IEND
        png.extend_from_slice(&0u32.to_be_bytes());
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&[0u8; 4]);

        let parsed = ExifParser::parse_from_bytes(&png, ExifContainer::Png);
        assert_eq!(parsed.camera.model, "ILCE-6700");
    }

    #[test]
    fn test_extract_from_webp_exif_chunk() {
        let md = sample_metadata();
        let tiff = ExifBuilder::new(&md).build_bytes().expect("bare tiff");

        let mut chunks = Vec::new();
        chunks.extend_from_slice(b"EXIF");
        chunks.extend_from_slice(&(tiff.len() as u32).to_le_bytes());
        chunks.extend_from_slice(&tiff);
        if tiff.len() % 2 == 1 {
            chunks.push(0);
        }

        let mut webp = Vec::new();
        webp.extend_from_slice(b"RIFF");
        webp.extend_from_slice(&((4 + chunks.len()) as u32).to_le_bytes());
        webp.extend_from_slice(b"WEBP");
        webp.extend_from_slice(&chunks);

        let parsed = ExifParser::parse_from_bytes(&webp, ExifContainer::WebP);
        assert_eq!(parsed.exif.exposure_time, Some(URational::new(1, 250)));
    }
}
