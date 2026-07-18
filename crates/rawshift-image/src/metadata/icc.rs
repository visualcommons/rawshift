//! ICC profile handling for image export.
//!
//! The profile bytes themselves are built and validated with `gamut-icc` (the
//! upstream home for ICC parsing/serialization); the AVIF box-splicing
//! embedding path stays on this side until the per-format codec migrations
//! move it behind the gamut codec boundaries (JPEG, PNG, and JXL already
//! embed through their gamut encoders).

use crate::metadata::isobmff::{find_box, patch_iloc_extents, read_u32_be, write_u32_be};

/// Error type for ICC operations.
#[derive(Debug)]
#[allow(dead_code)]
pub enum IccError {
    /// Invalid ICC profile data
    InvalidProfile(String),
    /// Failed to manipulate image container
    Container(String),
}

impl std::fmt::Display for IccError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IccError::InvalidProfile(msg) => write!(f, "Invalid ICC profile: {}", msg),
            IccError::Container(msg) => write!(f, "Container manipulation error: {}", msg),
        }
    }
}

impl std::error::Error for IccError {}

impl From<std::io::Error> for IccError {
    fn from(e: std::io::Error) -> Self {
        IccError::Container(e.to_string())
    }
}

/// Embedded ICC profile data.
#[derive(Debug, Clone)]
pub struct IccProfile {
    data: Vec<u8>,
}

impl IccProfile {
    /// Create an sRGB ICC profile.
    ///
    /// This is a minimal sRGB profile suitable for embedding in most images.
    /// The profile is based on the sRGB IEC61966-2.1 specification.
    pub fn srgb() -> Self {
        Self {
            data: build_srgb_profile(),
        }
    }

    /// Create an ICC profile from raw bytes.
    #[allow(dead_code)]
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Get the ICC profile bytes.
    #[allow(dead_code)]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Check if this is a valid ICC profile.
    ///
    /// Validates by parsing the profile with `gamut-icc` (header, tag table,
    /// and every tag's element data).
    #[allow(dead_code)]
    pub fn is_valid(&self) -> bool {
        gamut_icc::IccProfile::parse(&self.data).is_ok()
    }

    /// Embed ICC profile into AVIF (ISOBMFF) data.
    ///
    /// Replaces or inserts a `colr rICC` box in `meta/iprp/ipco` and patches
    /// `iloc` extent offsets to account for the file-size change.
    pub fn append_to_avif(&self, avif_data: Vec<u8>) -> Result<Vec<u8>, IccError> {
        let mut data = avif_data;

        // Find top-level meta box
        let data_len = data.len();
        let meta_start = find_box(&data, 0, data_len, b"meta")
            .ok_or_else(|| IccError::Container("no meta box in AVIF".into()))?;
        let meta_size = read_u32_be(&data, meta_start) as usize;
        let meta_end = meta_start + meta_size;
        // meta is FullBox: size(4)+type(4)+version+flags(4) = 12-byte header
        let meta_content_start = meta_start + 12;

        // Find iprp inside meta
        let iprp_start = find_box(&data, meta_content_start, meta_end, b"iprp")
            .ok_or_else(|| IccError::Container("no iprp box in AVIF".into()))?;
        let iprp_size = read_u32_be(&data, iprp_start) as usize;
        let iprp_end = iprp_start + iprp_size;
        let iprp_content_start = iprp_start + 8;

        // Find ipco inside iprp
        let ipco_start = find_box(&data, iprp_content_start, iprp_end, b"ipco")
            .ok_or_else(|| IccError::Container("no ipco box in AVIF".into()))?;
        let ipco_size = read_u32_be(&data, ipco_start) as usize;
        let ipco_end = ipco_start + ipco_size;
        let ipco_content_start = ipco_start + 8;

        // Build new colr rICC box: size(4) + "colr"(4) + "rICC"(4) + icc_bytes
        let icc_bytes = self.as_bytes();
        let new_colr_size = 12 + icc_bytes.len();
        let mut new_colr_box = Vec::with_capacity(new_colr_size);
        new_colr_box.extend_from_slice(&(new_colr_size as u32).to_be_bytes());
        new_colr_box.extend_from_slice(b"colr");
        new_colr_box.extend_from_slice(b"rICC");
        new_colr_box.extend_from_slice(icc_bytes);

        // Replace existing colr box or insert at start of ipco content
        let (splice_start, splice_end) =
            if let Some(colr_start) = find_box(&data, ipco_content_start, ipco_end, b"colr") {
                let old_size = read_u32_be(&data, colr_start) as usize;
                (colr_start, colr_start + old_size)
            } else {
                (ipco_content_start, ipco_content_start)
            };

        let delta = new_colr_size as isize - (splice_end - splice_start) as isize;
        data.splice(splice_start..splice_end, new_colr_box);

        // Patch parent box sizes (all starts are before splice point, still valid)
        write_u32_be(&mut data, ipco_start, (ipco_size as isize + delta) as u32);
        write_u32_be(&mut data, iprp_start, (iprp_size as isize + delta) as u32);
        write_u32_be(&mut data, meta_start, (meta_size as isize + delta) as u32);

        // Patch iloc extent offsets: mdat shifted by delta bytes
        let new_meta_end = meta_start + (meta_size as isize + delta) as usize;
        if let Some(iloc_start) = find_box(&data, meta_content_start, new_meta_end, b"iloc") {
            patch_iloc_extents(&mut data, iloc_start, delta).map_err(IccError::Container)?;
        }

        Ok(data)
    }
}

/// Build the minimal sRGB profile bytes from gamut-icc typed parts.
///
/// A v2.1 display profile carrying the sRGB colorants under the D50 PCS
/// (chromatically adapted, the exact `s15Fixed16` encodings the previous
/// hand-rolled profile used), a shared gamma-2.2 tone curve (`u8Fixed8`
/// `0x0238`), a description, and a copyright tag.
fn build_srgb_profile() -> Vec<u8> {
    use gamut_icc::{
        ColorSpace, Curve, DeviceClass, IccProfile as GamutIccProfile, ProfileHeader,
        ProfileVersion, S15Fixed16, Signature, TagData, TextDescription, U8Fixed8, XyzNumber,
    };

    let xyz = |x: i32, y: i32, z: i32| {
        TagData::Xyz(vec![XyzNumber {
            x: S15Fixed16(x),
            y: S15Fixed16(y),
            z: S15Fixed16(z),
        }])
    };
    // Gamma 2.2 approximation as u8Fixed8 (0x0238 ≈ 2.21875).
    let trc = TagData::Curve(Curve::Gamma(U8Fixed8(0x0238)));

    let mut header = ProfileHeader::new(DeviceClass::Display, ColorSpace::Rgb);
    // v2.1, matching the widest-compatibility profile this crate always
    // embedded (v2 uses the `desc`/`text` element types below).
    header.version = ProfileVersion {
        major: 2,
        minor: 1,
        bugfix: 0,
    };
    // `ProfileHeader::new` already sets the mandated D50 PCS illuminant.
    debug_assert_eq!(header.pcs_illuminant, XyzNumber::D50);

    let profile = GamutIccProfile {
        header,
        tags: vec![
            (
                Signature(*b"desc"),
                TagData::TextDescription(TextDescription {
                    ascii: "sRGB".into(),
                    unicode_language: 0,
                    unicode: String::new(),
                    script_code: 0,
                    macintosh: Vec::new(),
                }),
            ),
            (Signature(*b"cprt"), TagData::Text("Public Domain".into())),
            // Media white point: D50 (ICC PCS standard).
            (
                Signature(*b"wtpt"),
                xyz(0x0000_F6D6, 0x0001_0000, 0x0000_D32D),
            ),
            // sRGB colorants, D50-adapted.
            (
                Signature(*b"rXYZ"),
                xyz(0x0000_6FA2, 0x0000_38F5, 0x0000_0390),
            ),
            (
                Signature(*b"gXYZ"),
                xyz(0x0000_6299, 0x0000_B786, 0x0000_1852),
            ),
            (
                Signature(*b"bXYZ"),
                xyz(0x0000_24A0, 0x0000_0F84, 0x0000_B6CF),
            ),
            (Signature(*b"rTRC"), trc.clone()),
            (Signature(*b"gTRC"), trc.clone()),
            (Signature(*b"bTRC"), trc),
        ],
    };

    // The model above is fixed, spec-valid data; serialization can only fail
    // on hand-built invariant violations (duplicate signatures, contradictory
    // LUT shapes), none of which apply here.
    profile
        .to_bytes()
        .expect("static sRGB profile must serialize")
}

impl Default for IccProfile {
    fn default() -> Self {
        Self::srgb()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srgb_profile_valid() {
        let profile = IccProfile::srgb();
        assert!(profile.is_valid(), "sRGB profile should be valid");
    }

    #[test]
    fn test_srgb_profile_size() {
        let profile = IccProfile::srgb();
        let bytes = profile.as_bytes();
        // Minimal sRGB profile should be ~300-600 bytes
        assert!(
            bytes.len() > 200,
            "sRGB profile should be at least 200 bytes, got {}",
            bytes.len()
        );
        assert!(
            bytes.len() < 2000,
            "sRGB profile should be under 2KB, got {}",
            bytes.len()
        );
    }

    #[test]
    fn test_srgb_profile_header() {
        let profile = IccProfile::srgb();
        let bytes = profile.as_bytes();

        // Check magic 'acsp' at offset 36
        assert_eq!(&bytes[36..40], b"acsp", "ICC magic should be 'acsp'");

        // Check profile version (2.1.0.0)
        assert_eq!(bytes[8], 2, "Major version should be 2");
        assert_eq!(bytes[9], 0x10, "Minor version should be 1.0");

        // Check device class 'mntr' at offset 12
        assert_eq!(&bytes[12..16], b"mntr", "Device class should be 'mntr'");

        // Check color space 'RGB ' at offset 16
        assert_eq!(&bytes[16..20], b"RGB ", "Color space should be 'RGB '");
    }

    #[test]
    fn test_srgb_profile_content() {
        // The typed values must survive a parse round-trip with the exact
        // fixed-point encodings the previous hand-rolled profile carried.
        use gamut_icc::{KnownTag, S15Fixed16, TagData};

        let parsed = gamut_icc::IccProfile::parse(IccProfile::srgb().as_bytes()).expect("parse");
        match parsed.get(KnownTag::RedColorant) {
            Some(TagData::Xyz(v)) => {
                assert_eq!(v[0].x, S15Fixed16(0x0000_6FA2));
                assert_eq!(v[0].y, S15Fixed16(0x0000_38F5));
                assert_eq!(v[0].z, S15Fixed16(0x0000_0390));
            }
            other => panic!("rXYZ must be an XYZ tag, got {other:?}"),
        }
        match parsed.get(KnownTag::MediaWhitePoint) {
            Some(TagData::Xyz(v)) => {
                assert_eq!(v[0].y, S15Fixed16(0x0001_0000));
            }
            other => panic!("wtpt must be an XYZ tag, got {other:?}"),
        }
        assert!(
            matches!(parsed.get(KnownTag::RedTrc), Some(TagData::Curve(_))),
            "rTRC must be a curve tag"
        );
    }

    #[test]
    fn test_profile_from_bytes() {
        let fake_profile = vec![0u8; 100];
        let profile = IccProfile::from_bytes(fake_profile);
        assert!(!profile.is_valid(), "Fake profile should not be valid");
    }
}
