//! Bridge between rawshift's [`ImageMetadata`] and gamut's unified
//! [`gamut_metadata::Metadata`] model.
//!
//! [`to_gamut`] projects the typed EXIF fields plus the stored XMP/ICC blobs
//! into gamut's model; [`from_gamut`] maps a gamut model back onto
//! [`ImageMetadata`] via the same tag mapping the EXIF parser uses.
//!
//! # What does not survive `to_gamut`
//!
//! gamut's `Metadata` carries exactly the three container serializations
//! (EXIF, XMP, ICC), so `ImageMetadata` fields with no gamut home are dropped
//! by the projection and must be preserved on the rawshift side when needed
//! (gamut#258 tracks typed camera/DNG structs upstream):
//!
//! - the DNG structs (`dng_color`, `dng_calibration`, `dng_profile`);
//! - the sensor fields of `image` (`bit_depth`, `black_levels`,
//!   `white_level`, `default_crop_*`) — only `orientation` is projected;
//! - the generic `extra` table (including the `Heic` container facts);
//! - the raw blobs `makernote_raw` and `iptc_raw`;
//! - `camera.unique_camera_model` and `camera.lens_info`;
//! - `exif_raw` (the projection re-serializes from the typed fields rather
//!   than passing the source blob through).
//!
//! The reverse direction is narrower still: [`from_gamut`] fills the typed
//! EXIF fields, mirrors every EXIF tag into `extra`, and stores the XMP/ICC
//! carriers back as blobs.

use crate::exif::{ExifBuilder, ExifParser};
use gamut_exif::ExifWriter;
use gamut_metadata::Metadata;
use rawshift_core::metadata::ImageMetadata;

/// Project an [`ImageMetadata`] into gamut's unified [`Metadata`] model.
///
/// EXIF is rebuilt from the typed fields with [`ExifBuilder`]; the stored XMP
/// and ICC blobs are parsed into their typed gamut forms (and skipped when
/// malformed). See the module docs for what the projection drops.
#[allow(dead_code)] // consumed by format crates that opt into metadata support
pub fn to_gamut(md: &ImageMetadata) -> Metadata {
    let exif_model = ExifBuilder::new(md).build();
    let exif = (!exif_model.image().fields().is_empty()
        || exif_model.exif_ifd().is_some()
        || exif_model.gps_ifd().is_some())
    .then_some(exif_model);

    let xmp = md
        .xmp
        .as_deref()
        .and_then(|blob| gamut_xmp::XmpMeta::from_packet(blob).ok());
    let icc = md
        .icc_profile
        .as_deref()
        .and_then(|blob| gamut_icc::IccProfile::parse(blob).ok());

    Metadata { exif, xmp, icc }
}

/// Map a gamut [`Metadata`] model onto an [`ImageMetadata`].
///
/// The EXIF model is converted with the same tag mapping the decode-side
/// parser uses (typed fields plus the generic `extra` mirror, and the
/// re-serialized blob in `exif_raw`); XMP and ICC are serialized back into
/// their blob fields.
#[allow(dead_code)] // consumed by format crates that opt into metadata support
pub fn from_gamut(metadata: &Metadata) -> ImageMetadata {
    let mut md = match &metadata.exif {
        Some(exif) => ExifParser::parse_metadata(exif),
        None => ImageMetadata::default(),
    };

    if let Some(exif) = &metadata.exif {
        // A bare TIFF stream, matching what decode paths store in `exif_raw`.
        md.exif_raw = ExifWriter::new().marker(false).write(exif).ok();
    }
    if let Some(xmp) = &metadata.xmp {
        md.xmp = Some(xmp.to_packet());
    }
    if let Some(icc) = &metadata.icc {
        md.icc_profile = icc.to_bytes().ok();
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawshift_core::metadata::*;

    fn sample_metadata() -> ImageMetadata {
        ImageMetadata {
            camera: CameraInfo {
                make: "SONY".into(),
                model: "ILCE-6700".into(),
                lens_model: Some("E 18-135mm F3.5-5.6 OSS".into()),
                serial_number: Some("0123456".into()),
                ..Default::default()
            },
            exif: ExifInfo {
                iso: Some(800),
                exposure_time: Some(URational::new(1, 250)),
                f_number: Some(URational::new(56, 10)),
                focal_length: Some(URational::new(35, 1)),
                focal_length_35mm: Some(52),
                exposure_program: Some(3),
                metering_mode: Some(5),
                exposure_compensation: Some(SRational::new(-1, 3)),
                ..Default::default()
            },
            datetime: DateTimeInfo {
                datetime_original: Some("2025:12:01 14:30:00".into()),
                offset_time: Some("-05:00".into()),
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
                altitude: Some(URational::new(10, 1)),
                altitude_ref: Some(0),
                datestamp: Some("2025:12:01".into()),
                ..Default::default()
            },
            image: ImageInfo {
                orientation: Some(6),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn round_trip_preserves_typed_fields() {
        let md = sample_metadata();
        let gamut = to_gamut(&md);
        assert!(gamut.exif.is_some(), "EXIF must be projected");

        let back = from_gamut(&gamut);
        assert_eq!(back.camera.make, md.camera.make);
        assert_eq!(back.camera.model, md.camera.model);
        assert_eq!(back.camera.lens_model, md.camera.lens_model);
        assert_eq!(back.camera.serial_number, md.camera.serial_number);
        assert_eq!(back.exif.iso, md.exif.iso);
        assert_eq!(back.exif.exposure_time, md.exif.exposure_time);
        assert_eq!(back.exif.f_number, md.exif.f_number);
        assert_eq!(back.exif.focal_length, md.exif.focal_length);
        assert_eq!(back.exif.focal_length_35mm, md.exif.focal_length_35mm);
        assert_eq!(back.exif.exposure_program, md.exif.exposure_program);
        assert_eq!(back.exif.metering_mode, md.exif.metering_mode);
        assert_eq!(
            back.exif.exposure_compensation,
            md.exif.exposure_compensation
        );
        assert_eq!(
            back.datetime.datetime_original,
            md.datetime.datetime_original
        );
        assert_eq!(back.datetime.offset_time, md.datetime.offset_time);
        assert_eq!(back.gps.latitude, md.gps.latitude);
        assert_eq!(back.gps.latitude_ref, md.gps.latitude_ref);
        assert_eq!(back.gps.longitude, md.gps.longitude);
        assert_eq!(back.gps.longitude_ref, md.gps.longitude_ref);
        assert_eq!(back.gps.altitude, md.gps.altitude);
        assert_eq!(back.gps.altitude_ref, md.gps.altitude_ref);
        assert_eq!(back.gps.datestamp, md.gps.datestamp);
        assert_eq!(back.image.orientation, md.image.orientation);
        assert!(back.exif_raw.is_some(), "exif_raw must carry the blob");
    }

    #[test]
    fn empty_metadata_projects_to_empty_model() {
        let gamut = to_gamut(&ImageMetadata::default());
        assert!(gamut.is_empty());
        assert_eq!(from_gamut(&gamut), ImageMetadata::default());
    }

    #[test]
    fn xmp_and_icc_blobs_round_trip() {
        let xmp_packet = gamut_xmp::XmpMeta::new().to_packet();
        let md = ImageMetadata {
            xmp: Some(xmp_packet),
            icc_profile: Some(crate::icc::IccProfile::srgb().as_bytes().to_vec()),
            ..Default::default()
        };

        let gamut = to_gamut(&md);
        assert!(gamut.xmp.is_some(), "valid XMP must be projected");
        assert!(gamut.icc.is_some(), "valid ICC must be projected");

        let back = from_gamut(&gamut);
        assert!(back.xmp.is_some());
        let icc = back.icc_profile.expect("ICC blob must come back");
        assert!(gamut_icc::IccProfile::parse(&icc).is_ok());
    }

    #[test]
    fn malformed_blobs_are_skipped() {
        let md = ImageMetadata {
            xmp: Some(b"not xmp".to_vec()),
            icc_profile: Some(vec![0u8; 16]),
            ..Default::default()
        };
        let gamut = to_gamut(&md);
        assert!(gamut.xmp.is_none());
        assert!(gamut.icc.is_none());
    }
}
