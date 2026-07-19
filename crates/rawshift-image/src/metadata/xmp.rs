//! XMP metadata embedding for image export.
//!
//! Provides the XMP embedding function for the AVIF container (JPEG, PNG, and
//! JXL embed XMP through their gamut encoders instead). Every payload is
//! validated with `gamut-xmp` before it is embedded, so a malformed packet is
//! rejected instead of being spliced into the output.

/// Error type for XMP embedding operations.
#[derive(Debug)]
pub enum XmpError {
    /// The XMP payload is not a well-formed XMP packet
    Invalid(String),
    /// Failed to manipulate image container
    Container(String),
}

impl std::fmt::Display for XmpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XmpError::Invalid(msg) => write!(f, "Invalid XMP packet: {}", msg),
            XmpError::Container(msg) => write!(f, "XMP container error: {}", msg),
        }
    }
}

impl std::error::Error for XmpError {}

impl From<std::io::Error> for XmpError {
    fn from(e: std::io::Error) -> Self {
        XmpError::Container(e.to_string())
    }
}

/// Validate an XMP payload with `gamut-xmp` before embedding it.
///
/// Rejecting malformed packets here keeps garbage out of the output
/// containers; a payload that round-trips through
/// [`gamut_xmp::XmpMeta::from_packet`] is embeddable.
fn validate_xmp(xmp_bytes: &[u8]) -> Result<(), XmpError> {
    gamut_xmp::XmpMeta::from_packet(xmp_bytes)
        .map(|_| ())
        .map_err(|e| XmpError::Invalid(e.to_string()))
}

/// Append XMP metadata to an in-memory AVIF byte stream.
///
/// A top-level `xml ` ISOBMFF box containing the XMP packet is appended to the
/// end of the data.  Since the new box follows all existing boxes (including
/// `mdat`), no `iloc` extent offsets need to be patched.
/// The payload is validated with `gamut-xmp` first.
#[cfg_attr(not(feature = "avif"), allow(dead_code))]
pub fn append_xmp_to_avif(xmp_bytes: &[u8], avif_data: Vec<u8>) -> Result<Vec<u8>, XmpError> {
    validate_xmp(xmp_bytes)?;

    let mut data = avif_data;
    let box_size = (8 + xmp_bytes.len()) as u32;
    data.reserve(box_size as usize);
    data.extend_from_slice(&box_size.to_be_bytes());
    data.extend_from_slice(b"xml ");
    data.extend_from_slice(xmp_bytes);
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal well-formed XMP body (`rdf:RDF` is required by the validator).
    const VALID_XMP: &[u8] = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
        <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"/>\
        </x:xmpmeta>";

    #[test]
    fn test_malformed_xmp_is_rejected() {
        // Not XML at all → every embed path must refuse to splice it in.
        let avif = vec![0u8; 32];
        assert!(matches!(
            append_xmp_to_avif(b"not xmp", avif),
            Err(XmpError::Invalid(_))
        ));
        // XML without an rdf:RDF element is not an XMP packet either.
        assert!(matches!(
            append_xmp_to_avif(b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>", vec![0u8; 8]),
            Err(XmpError::Invalid(_))
        ));
    }

    #[test]
    fn test_append_xmp_to_avif() {
        // `append_xmp_to_avif` only appends a trailing `xml ` box, so the
        // leading bytes need not form a real container for this unit test.
        let avif = vec![0u8; 32];
        let xmp = VALID_XMP;
        let result = append_xmp_to_avif(xmp, avif).expect("AVIF XMP embed should succeed");

        let box_size = (8 + xmp.len()) as u32;
        assert_eq!(result.len(), 32 + box_size as usize);
        assert_eq!(&result[32..36], &box_size.to_be_bytes());
        assert_eq!(&result[36..40], b"xml ");
        assert_eq!(&result[40..], xmp);
    }
}
