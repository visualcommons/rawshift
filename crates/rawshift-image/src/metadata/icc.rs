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

#[cfg(feature = "container-embed")]
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
    #[cfg(feature = "container-embed")]
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
            patch_iloc_extents(&mut data, iloc_start, delta)?;
        }

        Ok(data)
    }

    /// Embed ICC profile into JXL data.
    ///
    /// If `jxl_data` is a naked codestream (starts with `[0xFF, 0x0A]`), it is
    /// first wrapped in a JXL container. An `iccp` box is then inserted before
    /// the first `Exif`/`xml ` box, or appended at the end.
    pub fn append_to_jxl(&self, jxl_data: Vec<u8>) -> Result<Vec<u8>, IccError> {
        let mut data = jxl_data;

        // Ensure container format
        if data.starts_with(&[0xFF, 0x0A]) {
            let codestream = std::mem::take(&mut data);
            let jxlc_size = (8 + codestream.len()) as u32;
            let mut container = Vec::new();
            // JXL signature box (12 bytes): size=12, type="JXL ", data=[0D 0A 87 0A]
            container.extend_from_slice(&[0x00, 0x00, 0x00, 0x0C]);
            container.extend_from_slice(b"JXL ");
            container.extend_from_slice(&[0x0D, 0x0A, 0x87, 0x0A]);
            // ftyp box (20 bytes)
            container.extend_from_slice(&[0x00, 0x00, 0x00, 0x14]);
            container.extend_from_slice(b"ftyp");
            container.extend_from_slice(b"jxl ");
            container.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
            container.extend_from_slice(b"jxl ");
            // jxlc box
            container.extend_from_slice(&jxlc_size.to_be_bytes());
            container.extend_from_slice(b"jxlc");
            container.extend_from_slice(&codestream);
            data = container;
        } else if data.get(4..8) != Some(b"JXL ") {
            return Err(IccError::Container("unrecognized JXL format".into()));
        }

        // Find insert position (before first Exif/xml /jbrd box, or end)
        let insert_pos = find_jxl_insert_pos(&data);

        // Build and splice iccp box
        let icc_bytes = self.as_bytes();
        let iccp_size = (8 + icc_bytes.len()) as u32;
        let mut iccp_box = Vec::with_capacity(iccp_size as usize);
        iccp_box.extend_from_slice(&iccp_size.to_be_bytes());
        iccp_box.extend_from_slice(b"iccp");
        iccp_box.extend_from_slice(icc_bytes);
        data.splice(insert_pos..insert_pos, iccp_box);

        Ok(data)
    }
}

// --- Private ISOBMFF / JXL container helpers ---

fn read_u32_be(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn write_u32_be(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

fn read_uint_be(data: &[u8], offset: usize, size: usize) -> u64 {
    match size {
        0 => 0,
        1 => data[offset] as u64,
        2 => u16::from_be_bytes(data[offset..offset + 2].try_into().unwrap()) as u64,
        4 => u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as u64,
        8 => u64::from_be_bytes(data[offset..offset + 8].try_into().unwrap()),
        _ => 0,
    }
}

fn write_uint_be(data: &mut [u8], offset: usize, size: usize, value: u64) {
    match size {
        0 => {}
        1 => data[offset] = value as u8,
        2 => data[offset..offset + 2].copy_from_slice(&(value as u16).to_be_bytes()),
        4 => data[offset..offset + 4].copy_from_slice(&(value as u32).to_be_bytes()),
        8 => data[offset..offset + 8].copy_from_slice(&value.to_be_bytes()),
        _ => {}
    }
}

/// Find the first box of `box_type` in byte range `[start, end)`.
fn find_box(data: &[u8], start: usize, end: usize, box_type: &[u8; 4]) -> Option<usize> {
    let mut pos = start;
    while pos + 8 <= end.min(data.len()) {
        let size = read_u32_be(data, pos) as usize;
        if size < 8 || pos + size > data.len() {
            break;
        }
        if &data[pos + 4..pos + 8] == box_type {
            return Some(pos);
        }
        pos += size;
    }
    None
}

/// Patch iloc extent offsets by adding `delta` (mdat shifted by this amount).
fn patch_iloc_extents(data: &mut [u8], iloc_start: usize, delta: isize) -> Result<(), IccError> {
    if iloc_start + 16 > data.len() {
        return Err(IccError::Container("iloc box too small".into()));
    }
    // FullBox: size(4)+type(4)+version(1)+flags(3); version at +8
    let version = data[iloc_start + 8];
    // Nibble fields at +12 and +13
    let offset_size = ((data[iloc_start + 12] >> 4) & 0xF) as usize;
    let length_size = (data[iloc_start + 12] & 0xF) as usize;
    let base_offset_size = ((data[iloc_start + 13] >> 4) & 0xF) as usize;
    let index_size = if version >= 1 {
        (data[iloc_start + 13] & 0xF) as usize
    } else {
        0
    };
    let (item_count, mut pos) = if version < 2 {
        let count = u16::from_be_bytes([data[iloc_start + 14], data[iloc_start + 15]]) as usize;
        (count, iloc_start + 16)
    } else {
        let count = read_u32_be(data, iloc_start + 14) as usize;
        (count, iloc_start + 18)
    };

    for _ in 0..item_count {
        // item_id
        pos += if version < 2 { 2 } else { 4 };
        // construction_method (v1/2)
        if version >= 1 {
            pos += 2;
        }
        // data_reference_index
        pos += 2;
        // base_data_offset (patch if stored and non-zero)
        if base_offset_size > 0 {
            if pos + base_offset_size > data.len() {
                return Err(IccError::Container("iloc base_data_offset OOB".into()));
            }
            let v = read_uint_be(data, pos, base_offset_size);
            if v > 0 {
                write_uint_be(
                    data,
                    pos,
                    base_offset_size,
                    (v as i64 + delta as i64) as u64,
                );
            }
        }
        pos += base_offset_size;
        // extent_count
        if pos + 2 > data.len() {
            return Err(IccError::Container("iloc extent_count OOB".into()));
        }
        let extent_count = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        for _ in 0..extent_count {
            if version >= 1 {
                pos += index_size;
            }
            // extent_offset — patch
            if offset_size > 0 {
                if pos + offset_size > data.len() {
                    return Err(IccError::Container("iloc extent_offset OOB".into()));
                }
                let v = read_uint_be(data, pos, offset_size);
                write_uint_be(data, pos, offset_size, (v as i64 + delta as i64) as u64);
            }
            pos += offset_size;
            pos += length_size;
        }
    }
    Ok(())
}

/// Return the offset at which to insert an `iccp` box in a JXL container.
fn find_jxl_insert_pos(data: &[u8]) -> usize {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let size = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        if size < 8 || pos + size > data.len() {
            break;
        }
        if matches!(&data[pos + 4..pos + 8], b"Exif" | b"xml " | b"jbrd") {
            return pos;
        }
        pos += size;
    }
    data.len()
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

    // Build a minimal valid JXL container with just a signature + ftyp + jxlc box.
    fn make_jxl_container(codestream: &[u8]) -> Vec<u8> {
        let jxlc_size = (8 + codestream.len()) as u32;
        let mut c = Vec::new();
        // signature box
        c.extend_from_slice(&[0x00, 0x00, 0x00, 0x0C]);
        c.extend_from_slice(b"JXL ");
        c.extend_from_slice(&[0x0D, 0x0A, 0x87, 0x0A]);
        // ftyp
        c.extend_from_slice(&[0x00, 0x00, 0x00, 0x14]);
        c.extend_from_slice(b"ftyp");
        c.extend_from_slice(b"jxl ");
        c.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        c.extend_from_slice(b"jxl ");
        // jxlc
        c.extend_from_slice(&jxlc_size.to_be_bytes());
        c.extend_from_slice(b"jxlc");
        c.extend_from_slice(codestream);
        c
    }

    fn jxl_has_iccp(data: &[u8]) -> bool {
        let mut pos = 0;
        while pos + 8 <= data.len() {
            let sz = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            if sz < 8 {
                break;
            }
            if &data[pos + 4..pos + 8] == b"iccp" {
                return true;
            }
            pos += sz;
        }
        false
    }

    #[test]
    fn test_append_to_jxl_naked_codestream() {
        // Fake naked JXL codestream (just the magic bytes + padding)
        let mut naked = vec![0xFF, 0x0A];
        naked.extend_from_slice(&[0u8; 16]);

        let profile = IccProfile::srgb();
        let result = profile.append_to_jxl(naked).expect("append_to_jxl failed");

        // Must be a container
        assert!(
            result.get(4..8) == Some(b"JXL "),
            "Output should be JXL container"
        );
        // Must contain iccp box
        assert!(jxl_has_iccp(&result), "Output should have iccp box");
    }

    #[test]
    fn test_append_to_jxl_container_form() {
        let container = make_jxl_container(&[0xFF, 0x0A, 0x00]);
        let profile = IccProfile::srgb();
        let result = profile
            .append_to_jxl(container)
            .expect("append_to_jxl failed");

        assert!(
            result.get(4..8) == Some(b"JXL "),
            "Output should still be JXL container"
        );
        assert!(jxl_has_iccp(&result), "Output should have iccp box");
    }

    #[test]
    fn test_append_to_jxl_invalid_data() {
        let bad_data = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
        let profile = IccProfile::srgb();
        let result = profile.append_to_jxl(bad_data);
        assert!(result.is_err(), "Should error on unrecognized JXL data");
    }
}
