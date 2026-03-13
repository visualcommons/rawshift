//! ICC profile handling for image export.
//!
//! Provides ICC profile embedding for JPEG and other formats.

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

impl From<img_parts::Error> for IccError {
    fn from(e: img_parts::Error) -> Self {
        IccError::Container(e.to_string())
    }
}

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
        Self::create_minimal_srgb()
    }

    /// Create a minimal sRGB ICC profile programmatically.
    fn create_minimal_srgb() -> Self {
        // sRGB primaries and white point (D65) in fixed-point XYZ (s15Fixed16Number)
        // Values are multiplied by 65536 (16.16 fixed point)
        const SRGB_RED_X: u32 = 0x00006FA2; // 0.4360
        const SRGB_RED_Y: u32 = 0x000038F5; // 0.2224
        const SRGB_RED_Z: u32 = 0x00000390; // 0.0139

        const SRGB_GREEN_X: u32 = 0x00006299; // 0.3851
        const SRGB_GREEN_Y: u32 = 0x0000B786; // 0.7169
        const SRGB_GREEN_Z: u32 = 0x00001852; // 0.0971

        const SRGB_BLUE_X: u32 = 0x000024A0; // 0.1431
        const SRGB_BLUE_Y: u32 = 0x00000F84; // 0.0606
        const SRGB_BLUE_Z: u32 = 0x0000B6CF; // 0.7141

        const D50_X: u32 = 0x0000F6D6; // 0.9642 (D50 for ICC PCS)
        const D50_Y: u32 = 0x00010000; // 1.0000
        const D50_Z: u32 = 0x0000D32D; // 0.8249

        let mut profile = Vec::with_capacity(560);

        // === HEADER (128 bytes) ===
        let profile_size: u32 = 0; // Will update at end
        profile.extend_from_slice(&profile_size.to_be_bytes()); // Profile size (offset 0)
        profile.extend_from_slice(b"\0\0\0\0"); // Preferred CMM (offset 4)
        profile.extend_from_slice(&[0x02, 0x10, 0x00, 0x00]); // Version 2.1.0 (offset 8)
        profile.extend_from_slice(b"mntr"); // Device class: monitor (offset 12)
        profile.extend_from_slice(b"RGB "); // Color space: RGB (offset 16)
        profile.extend_from_slice(b"XYZ "); // PCS: XYZ (offset 20)
        profile.extend_from_slice(&[0u8; 12]); // Creation date/time (offset 24)
        profile.extend_from_slice(b"acsp"); // Profile signature (offset 36)
        profile.extend_from_slice(b"\0\0\0\0"); // Platform (offset 40)
        profile.extend_from_slice(&[0u8; 4]); // Flags (offset 44)
        profile.extend_from_slice(&[0u8; 4]); // Device manufacturer (offset 48)
        profile.extend_from_slice(&[0u8; 4]); // Device model (offset 52)
        profile.extend_from_slice(&[0u8; 8]); // Device attributes (offset 56)
        profile.extend_from_slice(&[0, 0, 0, 0]); // Rendering intent: perceptual (offset 64)
        // PCS illuminant (D50) (offset 68)
        profile.extend_from_slice(&D50_X.to_be_bytes());
        profile.extend_from_slice(&D50_Y.to_be_bytes());
        profile.extend_from_slice(&D50_Z.to_be_bytes());
        profile.extend_from_slice(&[0u8; 4]); // Profile creator (offset 80)
        profile.extend_from_slice(&[0u8; 16]); // Profile ID/MD5 (offset 84)
        profile.extend_from_slice(&[0u8; 28]); // Reserved (offset 100)

        assert_eq!(profile.len(), 128, "Header must be 128 bytes");

        // === TAG TABLE ===
        // Tags: wtpt, rXYZ, gXYZ, bXYZ, rTRC, gTRC, bTRC, cprt, desc
        let tag_count: u32 = 9;
        profile.extend_from_slice(&tag_count.to_be_bytes());

        // Calculate data offsets
        // Tag table starts at 128, each entry is 12 bytes
        // Data starts after 128 + 4 + (9 * 12) = 128 + 4 + 108 = 240
        let tag_data_start: u32 = 128 + 4 + (tag_count * 12);
        let mut data_offset = tag_data_start;

        // Helper to add tag entry
        fn add_tag_entry(buf: &mut Vec<u8>, sig: &[u8; 4], offset: u32, size: u32) {
            buf.extend_from_slice(sig);
            buf.extend_from_slice(&offset.to_be_bytes());
            buf.extend_from_slice(&size.to_be_bytes());
        }

        // XYZ type is 20 bytes: sig(4) + reserved(4) + X(4) + Y(4) + Z(4)
        // curv type with 1 entry is 14 bytes: sig(4) + reserved(4) + count(4) + gamma(2)
        // text type is 8 + strlen + 1
        // desc type is 12 + strlen + 1 (simplified)

        let wtpt_offset = data_offset;
        add_tag_entry(&mut profile, b"wtpt", wtpt_offset, 20);
        data_offset += 20;

        let rxyz_offset = data_offset;
        add_tag_entry(&mut profile, b"rXYZ", rxyz_offset, 20);
        data_offset += 20;

        let gxyz_offset = data_offset;
        add_tag_entry(&mut profile, b"gXYZ", gxyz_offset, 20);
        data_offset += 20;

        let bxyz_offset = data_offset;
        add_tag_entry(&mut profile, b"bXYZ", bxyz_offset, 20);
        data_offset += 20;

        // TRC tags can share the same data if identical
        let trc_offset = data_offset;
        let trc_size: u32 = 14;
        add_tag_entry(&mut profile, b"rTRC", trc_offset, trc_size);
        data_offset += (trc_size + 3) & !3; // Align to 4 bytes
        add_tag_entry(&mut profile, b"gTRC", trc_offset, trc_size); // Share same data
        add_tag_entry(&mut profile, b"bTRC", trc_offset, trc_size); // Share same data

        let cprt_text = b"Public Domain";
        let cprt_size: u32 = 8 + cprt_text.len() as u32 + 1;
        let cprt_offset = data_offset;
        add_tag_entry(&mut profile, b"cprt", cprt_offset, cprt_size);
        data_offset += (cprt_size + 3) & !3;

        let desc_text = b"sRGB";
        let desc_size: u32 = 12 + desc_text.len() as u32 + 1;
        let desc_offset = data_offset;
        add_tag_entry(&mut profile, b"desc", desc_offset, desc_size);
        let _data_offset = data_offset + ((desc_size + 3) & !3);

        assert_eq!(
            profile.len(),
            tag_data_start as usize,
            "Tag table size mismatch"
        );

        // === TAG DATA ===

        // wtpt (white point)
        fn write_xyz(buf: &mut Vec<u8>, x: u32, y: u32, z: u32) {
            buf.extend_from_slice(b"XYZ ");
            buf.extend_from_slice(&[0u8; 4]); // Reserved
            buf.extend_from_slice(&x.to_be_bytes());
            buf.extend_from_slice(&y.to_be_bytes());
            buf.extend_from_slice(&z.to_be_bytes());
        }

        // D50 white point (ICC PCS standard)
        write_xyz(&mut profile, D50_X, D50_Y, D50_Z);

        // rXYZ (red primary)
        write_xyz(&mut profile, SRGB_RED_X, SRGB_RED_Y, SRGB_RED_Z);

        // gXYZ (green primary)
        write_xyz(&mut profile, SRGB_GREEN_X, SRGB_GREEN_Y, SRGB_GREEN_Z);

        // bXYZ (blue primary)
        write_xyz(&mut profile, SRGB_BLUE_X, SRGB_BLUE_Y, SRGB_BLUE_Z);

        // TRC (gamma curve) - using gamma 2.2 approximation
        // u8Fixed8Number: 0x0238 ≈ 2.21875
        let gamma_22: u16 = 0x0238;
        profile.extend_from_slice(b"curv");
        profile.extend_from_slice(&[0u8; 4]); // Reserved
        profile.extend_from_slice(&1u32.to_be_bytes()); // Count = 1 means gamma value
        profile.extend_from_slice(&gamma_22.to_be_bytes());
        // Pad to 4 bytes
        profile.extend_from_slice(&[0u8; 2]);

        // cprt (copyright)
        profile.extend_from_slice(b"text");
        profile.extend_from_slice(&[0u8; 4]); // Reserved
        profile.extend_from_slice(cprt_text);
        profile.push(0); // Null terminator
        // Pad to 4 bytes
        while profile.len() % 4 != 0 {
            profile.push(0);
        }

        // desc (description)
        profile.extend_from_slice(b"desc");
        profile.extend_from_slice(&[0u8; 4]); // Reserved
        profile.extend_from_slice(&(desc_text.len() as u32 + 1).to_be_bytes()); // Count
        profile.extend_from_slice(desc_text);
        profile.push(0); // Null terminator
        // Pad to 4 bytes
        while profile.len() % 4 != 0 {
            profile.push(0);
        }

        // Update profile size in header
        let final_size = profile.len() as u32;
        profile[0..4].copy_from_slice(&final_size.to_be_bytes());

        Self { data: profile }
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
    /// Performs basic validation by checking the magic bytes.
    #[allow(dead_code)]
    pub fn is_valid(&self) -> bool {
        if self.data.len() < 40 {
            return false;
        }
        // ICC magic at offset 36: 'acsp'
        &self.data[36..40] == b"acsp"
    }

    /// Append ICC profile to existing JPEG data.
    ///
    /// Uses img-parts for segment manipulation.
    pub fn append_to_jpeg(&self, jpeg_data: Vec<u8>) -> Result<Vec<u8>, IccError> {
        use img_parts::jpeg::Jpeg;
        use img_parts::{Bytes, ImageICC};
        use std::io::Cursor;

        let mut jpeg = Jpeg::from_bytes(Bytes::from(jpeg_data))?;
        jpeg.set_icc_profile(Some(Bytes::from(self.data.clone())));

        let mut output = Cursor::new(Vec::new());
        jpeg.encoder().write_to(&mut output)?;
        Ok(output.into_inner())
    }
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
    fn test_profile_from_bytes() {
        let fake_profile = vec![0u8; 100];
        let profile = IccProfile::from_bytes(fake_profile);
        assert!(!profile.is_valid(), "Fake profile should not be valid");
    }
}
