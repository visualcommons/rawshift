//! Helpers for extracting structured metadata from parsed TIFF IFDs.
//!
//! These functions bridge the gap between raw TIFF IFD entries and
//! the unified metadata types in `core::metadata`.

use std::io::{Read, Seek};

use crate::core::metadata::{DateTimeInfo, ExifInfo, GpsInfo, SRational, URational};
use crate::tiff::parser::{Ifd, TiffParser};
use crate::tiff::tags::TiffTag;
use crate::tiff::types::TiffValue;

// ============================================================================
// Primitive tag readers
// ============================================================================

fn read_u16_tag<R: Read + Seek>(
    parser: &mut TiffParser<R>,
    ifd: &Ifd,
    tag: TiffTag,
) -> Option<u16> {
    let entry = ifd.get(tag)?;
    let value = parser.read_value(entry).ok()?;
    match &value {
        TiffValue::Shorts(v) if !v.is_empty() => Some(v[0]),
        TiffValue::Longs(v) if !v.is_empty() => Some(v[0] as u16),
        TiffValue::Bytes(v) if !v.is_empty() => Some(v[0] as u16),
        _ => None,
    }
}

fn read_u8_tag<R: Read + Seek>(parser: &mut TiffParser<R>, ifd: &Ifd, tag: TiffTag) -> Option<u8> {
    let entry = ifd.get(tag)?;
    let value = parser.read_value(entry).ok()?;
    match &value {
        TiffValue::Bytes(v) if !v.is_empty() => Some(v[0]),
        TiffValue::Shorts(v) if !v.is_empty() => Some(v[0] as u8),
        _ => None,
    }
}

fn read_ascii_tag<R: Read + Seek>(
    parser: &mut TiffParser<R>,
    ifd: &Ifd,
    tag: TiffTag,
) -> Option<String> {
    let entry = ifd.get(tag)?;
    let value = parser.read_value(entry).ok()?;
    value.as_str().map(|s| s.to_string())
}

fn read_urational_tag<R: Read + Seek>(
    parser: &mut TiffParser<R>,
    ifd: &Ifd,
    tag: TiffTag,
) -> Option<URational> {
    let entry = ifd.get(tag)?;
    let value = parser.read_value(entry).ok()?;
    match &value {
        TiffValue::Rationals(v) if !v.is_empty() => Some(v[0].into()),
        _ => None,
    }
}

fn read_srational_tag<R: Read + Seek>(
    parser: &mut TiffParser<R>,
    ifd: &Ifd,
    tag: TiffTag,
) -> Option<SRational> {
    let entry = ifd.get(tag)?;
    let value = parser.read_value(entry).ok()?;
    match &value {
        TiffValue::SRationals(v) if !v.is_empty() => Some(v[0].into()),
        _ => None,
    }
}

fn read_urational3_tag<R: Read + Seek>(
    parser: &mut TiffParser<R>,
    ifd: &Ifd,
    tag: TiffTag,
) -> Option<[URational; 3]> {
    let entry = ifd.get(tag)?;
    let value = parser.read_value(entry).ok()?;
    match &value {
        TiffValue::Rationals(v) if v.len() >= 3 => Some([v[0].into(), v[1].into(), v[2].into()]),
        _ => None,
    }
}

// ============================================================================
// High-level metadata extractors
// ============================================================================

/// Extract EXIF exposure/capture settings from IFD0 and its EXIF sub-IFD.
pub fn extract_exif<R: Read + Seek>(parser: &mut TiffParser<R>, ifd0: &Ifd) -> ExifInfo {
    let exif_ifd = match ifd0.exif_ifd.as_deref() {
        Some(ifd) => ifd,
        None => return ExifInfo::default(),
    };

    ExifInfo {
        iso: read_u16_tag(parser, exif_ifd, TiffTag::ISOSpeedRatings).map(|v| v as u32),
        exposure_time: read_urational_tag(parser, exif_ifd, TiffTag::ExposureTime),
        f_number: read_urational_tag(parser, exif_ifd, TiffTag::FNumber),
        focal_length: read_urational_tag(parser, exif_ifd, TiffTag::FocalLength),
        focal_length_35mm: read_u16_tag(parser, exif_ifd, TiffTag::FocalLengthIn35mmFilm),
        exposure_program: read_u16_tag(parser, exif_ifd, TiffTag::ExposureProgram),
        metering_mode: read_u16_tag(parser, exif_ifd, TiffTag::MeteringMode),
        flash: read_u16_tag(parser, exif_ifd, TiffTag::Flash),
        exposure_compensation: read_srational_tag(parser, exif_ifd, TiffTag::ExposureBiasValue),
        max_aperture: read_urational_tag(parser, exif_ifd, TiffTag::MaxApertureValue),
        brightness_value: read_srational_tag(parser, exif_ifd, TiffTag::BrightnessValue),
    }
}

/// Extract date/time information from IFD0 and its EXIF sub-IFD.
pub fn extract_datetime<R: Read + Seek>(parser: &mut TiffParser<R>, ifd0: &Ifd) -> DateTimeInfo {
    let modify_date = read_ascii_tag(parser, ifd0, TiffTag::DateTime);

    let exif_ifd = ifd0.exif_ifd.as_deref();

    let datetime_original =
        exif_ifd.and_then(|ifd| read_ascii_tag(parser, ifd, TiffTag::DateTimeOriginal));
    let create_date =
        exif_ifd.and_then(|ifd| read_ascii_tag(parser, ifd, TiffTag::DateTimeDigitized));
    let offset_time = exif_ifd.and_then(|ifd| {
        read_ascii_tag(parser, ifd, TiffTag::OffsetTimeOriginal)
            .or_else(|| read_ascii_tag(parser, ifd, TiffTag::OffsetTime))
    });
    let subsec_time = exif_ifd.and_then(|ifd| {
        read_ascii_tag(parser, ifd, TiffTag::SubSecTimeOriginal)
            .or_else(|| read_ascii_tag(parser, ifd, TiffTag::SubSecTime))
    });

    DateTimeInfo {
        datetime_original,
        create_date,
        modify_date,
        offset_time,
        subsec_time,
    }
}

/// Extract GPS location data from the GPS sub-IFD.
pub fn extract_gps<R: Read + Seek>(parser: &mut TiffParser<R>, ifd0: &Ifd) -> GpsInfo {
    let gps_ifd = match ifd0.gps_ifd.as_deref() {
        Some(ifd) => ifd,
        None => return GpsInfo::default(),
    };

    GpsInfo {
        latitude: read_urational3_tag(parser, gps_ifd, TiffTag::GPSLatitude),
        latitude_ref: read_ascii_tag(parser, gps_ifd, TiffTag::GPSLatitudeRef)
            .and_then(|s| s.chars().next()),
        longitude: read_urational3_tag(parser, gps_ifd, TiffTag::GPSLongitude),
        longitude_ref: read_ascii_tag(parser, gps_ifd, TiffTag::GPSLongitudeRef)
            .and_then(|s| s.chars().next()),
        altitude: read_urational_tag(parser, gps_ifd, TiffTag::GPSAltitude),
        altitude_ref: read_u8_tag(parser, gps_ifd, TiffTag::GPSAltitudeRef),
        timestamp: read_urational3_tag(parser, gps_ifd, TiffTag::GPSTimeStamp),
        datestamp: read_ascii_tag(parser, gps_ifd, TiffTag::GPSDateStamp),
        speed: read_urational_tag(parser, gps_ifd, TiffTag::GPSSpeed),
        img_direction: read_urational_tag(parser, gps_ifd, TiffTag::GPSImgDirection),
    }
}

/// Extract lens make/model from the EXIF sub-IFD.
pub fn extract_lens_info<R: Read + Seek>(
    parser: &mut TiffParser<R>,
    ifd0: &Ifd,
) -> (Option<String>, Option<String>) {
    let exif_ifd = match ifd0.exif_ifd.as_deref() {
        Some(ifd) => ifd,
        None => return (None, None),
    };

    (
        read_ascii_tag(parser, exif_ifd, TiffTag::LensMake),
        read_ascii_tag(parser, exif_ifd, TiffTag::LensModel),
    )
}

/// Extract orientation from IFD0.
pub fn extract_orientation<R: Read + Seek>(parser: &mut TiffParser<R>, ifd0: &Ifd) -> Option<u16> {
    read_u16_tag(parser, ifd0, TiffTag::Orientation)
}
