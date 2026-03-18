//! XMP metadata embedding for image export.
//!
//! Provides XMP embedding functions for JPEG, AVIF, JXL, and PNG formats.

/// Error type for XMP embedding operations.
#[derive(Debug)]
pub enum XmpError {
    /// Failed to manipulate image container
    Container(String),
}

impl std::fmt::Display for XmpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XmpError::Container(msg) => write!(f, "XMP container error: {}", msg),
        }
    }
}

impl std::error::Error for XmpError {}

impl From<img_parts::Error> for XmpError {
    fn from(e: img_parts::Error) -> Self {
        XmpError::Container(e.to_string())
    }
}

impl From<std::io::Error> for XmpError {
    fn from(e: std::io::Error) -> Self {
        XmpError::Container(e.to_string())
    }
}

/// Adobe XMP namespace marker used in JPEG APP1 segments.
const XMP_JPEG_NS: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// Append XMP metadata to existing JPEG data.
///
/// XMP is embedded as an APP1 segment (0xE1) with the Adobe XMP namespace prefix
/// `http://ns.adobe.com/xap/1.0/\0` followed by the raw XMP packet bytes.
pub fn append_xmp_to_jpeg(xmp_bytes: &[u8], jpeg_data: Vec<u8>) -> Result<Vec<u8>, XmpError> {
    use img_parts::Bytes;
    use img_parts::jpeg::{Jpeg, JpegSegment, markers};
    use std::io::Cursor;

    let mut contents = Vec::with_capacity(XMP_JPEG_NS.len() + xmp_bytes.len());
    contents.extend_from_slice(XMP_JPEG_NS);
    contents.extend_from_slice(xmp_bytes);

    let mut jpeg = Jpeg::from_bytes(Bytes::from(jpeg_data))?;
    let xmp_segment = JpegSegment::new_with_contents(markers::APP1, Bytes::from(contents));

    // Insert just before the first segment with entropy (SOS), or at the end.
    let pos = jpeg
        .segments()
        .iter()
        .position(|s| s.has_entropy())
        .unwrap_or_else(|| jpeg.segments().len());
    jpeg.segments_mut().insert(pos, xmp_segment);

    let mut output = Cursor::new(Vec::new());
    jpeg.encoder().write_to(&mut output)?;
    Ok(output.into_inner())
}

/// Append XMP metadata to an AVIF file on disk.
///
/// A top-level `xml ` ISOBMFF box containing the XMP packet is appended to the
/// end of the file.  Since the new box follows all existing boxes (including
/// `mdat`), no `iloc` extent offsets need to be patched.
#[cfg_attr(not(feature = "avif"), allow(dead_code))]
pub fn append_xmp_to_avif_file(path: &std::path::Path, xmp_bytes: &[u8]) -> Result<(), XmpError> {
    let mut data = std::fs::read(path)?;
    let box_size = (8 + xmp_bytes.len()) as u32;
    data.reserve(box_size as usize);
    data.extend_from_slice(&box_size.to_be_bytes());
    data.extend_from_slice(b"xml ");
    data.extend_from_slice(xmp_bytes);
    std::fs::write(path, data)?;
    Ok(())
}

/// Append XMP metadata to JXL container data.
///
/// If `jxl_data` is a naked codestream (starts with `[0xFF, 0x0A]`), it is
/// first wrapped in a JXL container.  An `xml ` box containing the XMP packet
/// is then appended at the end of the container.
#[cfg_attr(not(feature = "jxl-encode"), allow(dead_code))]
pub fn append_xmp_to_jxl(xmp_bytes: &[u8], jxl_data: Vec<u8>) -> Result<Vec<u8>, XmpError> {
    let mut data = jxl_data;

    // Wrap naked codestream in a JXL container if needed.
    if data.starts_with(&[0xFF, 0x0A]) {
        let codestream = std::mem::take(&mut data);
        let jxlc_size = (8 + codestream.len()) as u32;
        let mut container = Vec::new();
        // JXL signature box (12 bytes)
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
        return Err(XmpError::Container("unrecognized JXL format".into()));
    }

    // Append xml box at end of container.
    let xml_size = (8 + xmp_bytes.len()) as u32;
    data.reserve(xml_size as usize);
    data.extend_from_slice(&xml_size.to_be_bytes());
    data.extend_from_slice(b"xml ");
    data.extend_from_slice(xmp_bytes);

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append_xmp_to_jpeg_basic() {
        // Build a minimal valid JPEG: SOI + APP0 (JFIF) + EOI
        let mut jpeg = Vec::new();
        jpeg.extend_from_slice(&[0xFF, 0xD8]); // SOI
        // APP0 (JFIF marker)
        jpeg.extend_from_slice(&[0xFF, 0xE0]);
        let app0_len: u16 = 16;
        jpeg.extend_from_slice(&app0_len.to_be_bytes());
        jpeg.extend_from_slice(b"JFIF\0");
        jpeg.extend_from_slice(&[1, 1, 0, 0, 1, 0, 1, 0, 0]); // JFIF header rest
        jpeg.extend_from_slice(&[0xFF, 0xD9]); // EOI

        let xmp = b"<x:xmpmeta><rdf:RDF/></x:xmpmeta>";
        let result = append_xmp_to_jpeg(xmp, jpeg).expect("XMP embed should succeed");

        // Must still be a valid JPEG (starts with SOI)
        assert_eq!(&result[0..2], &[0xFF, 0xD8]);
        // Must contain the XMP namespace prefix
        let has_ns = result.windows(XMP_JPEG_NS.len()).any(|w| w == XMP_JPEG_NS);
        assert!(has_ns, "output must contain XMP namespace marker");
        // Must contain the XMP payload
        let has_xmp = result.windows(xmp.len()).any(|w| w == xmp);
        assert!(has_xmp, "output must contain XMP payload");
    }

    #[test]
    fn test_append_xmp_to_jxl_naked_codestream() {
        let mut naked = vec![0xFF, 0x0A];
        naked.extend_from_slice(&[0u8; 16]);

        let xmp = b"<x:xmpmeta/>";
        let result = append_xmp_to_jxl(xmp, naked).expect("JXL XMP embed should succeed");

        assert_eq!(result.get(4..8), Some(b"JXL " as &[u8]));
        let has_xml_box = result.windows(4).any(|w| w == b"xml ");
        assert!(has_xml_box, "output must contain xml box");
    }

    #[test]
    fn test_append_xmp_to_jxl_container() {
        let jxlc_size = 11u32;
        let mut container = Vec::new();
        container.extend_from_slice(&[0x00, 0x00, 0x00, 0x0C]);
        container.extend_from_slice(b"JXL ");
        container.extend_from_slice(&[0x0D, 0x0A, 0x87, 0x0A]);
        container.extend_from_slice(&[0x00, 0x00, 0x00, 0x14]);
        container.extend_from_slice(b"ftyp");
        container.extend_from_slice(b"jxl ");
        container.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        container.extend_from_slice(b"jxl ");
        container.extend_from_slice(&jxlc_size.to_be_bytes());
        container.extend_from_slice(b"jxlc");
        container.extend_from_slice(&[0xFF, 0x0A, 0x00]);

        let xmp = b"<x:xmpmeta/>";
        let result = append_xmp_to_jxl(xmp, container).expect("JXL XMP embed should succeed");

        let has_xml_box = result.windows(4).any(|w| w == b"xml ");
        assert!(has_xml_box, "output must contain xml box");
    }

    #[test]
    fn test_append_xmp_to_jxl_invalid() {
        let bad = b"not a jxl file at all!!";
        let result = append_xmp_to_jxl(b"xmp", bad.to_vec());
        assert!(result.is_err());
    }
}
