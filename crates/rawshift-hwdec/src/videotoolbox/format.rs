//! Building the `CMVideoFormatDescription` that a decompression session is
//! created from, for both codecs.
//!
//! ## HEVC
//!
//! `CMVideoFormatDescriptionCreateFromHEVCParameterSets` takes the VPS/SPS/PPS
//! NAL units out of the `hvcC` record plus the record's NAL length prefix
//! width. Two things fall out of that choice:
//!
//! - VideoToolbox parses the SPS itself, so the coded dimensions never have to
//!   come from [`StillDecodeRequest::width`]/`height` — which the HEIC adapter
//!   passes as `0` precisely because `hvcC` carries no picture size.
//! - `nal_unit_header_length` *is* the "length-prefixed samples, no Annex-B
//!   conversion" contract: the payload is handed to the decoder byte for byte.
//!
//! ## AV1
//!
//! CoreMedia has no `…CreateFromAV1ParameterSets`, so the generic
//! `CMVideoFormatDescriptionCreate` is used with the whole `av1C` record body
//! carried in the standard `SampleDescriptionExtensionAtoms` dictionary. That
//! call *does* require non-zero dimensions, which come from the AV1 sequence
//! header (authoritative, and what VideoToolbox itself will parse) with the
//! advisory request fields as a last resort.

use std::ffi::c_int;
use std::ptr::{self, NonNull};

use objc2_core_foundation::{CFData, CFDictionary, CFRetained, CFString, CFType};
use objc2_core_media::{
    CMFormatDescription, CMVideoFormatDescriptionCreate,
    CMVideoFormatDescriptionCreateFromHEVCParameterSets,
    kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms, kCMVideoCodecType_AV1,
};

use super::status::{NO_ERR, decode_err, map_status};
use crate::bitstream::{av1, hevc};
use crate::{HwCodec, HwDecodeError, StillDecodeRequest};

/// The NAL unit types `CMVideoFormatDescriptionCreateFromHEVCParameterSets`
/// accepts: VPS (32), SPS (33), PPS (34), prefix SEI (39), suffix SEI (40).
const PARAMETER_SET_NALS: [u8; 5] = [32, 33, 34, 39, 40];

/// Erase a typed dictionary's phantom generics for the C signatures, which all
/// take the fully opaque `CFDictionary`.
pub(super) fn as_opaque_dict<K: ?Sized, V: ?Sized>(dict: &CFDictionary<K, V>) -> &CFDictionary {
    // SAFETY: `CFDictionary<K, V>` is a zero-sized opaque wrapper whose type
    // parameters are pure `PhantomData` — every instantiation has identical
    // layout and wraps the same underlying `CFDictionaryRef`. The C entry
    // points only read it as an opaque dictionary.
    unsafe { &*(dict as *const CFDictionary<K, V>).cast::<CFDictionary>() }
}

/// Take ownership of a `Create`-rule format description returned through an
/// out-parameter.
fn adopt(
    codec: HwCodec,
    operation: &str,
    status: i32,
    raw: *const CMFormatDescription,
) -> Result<CFRetained<CMFormatDescription>, HwDecodeError> {
    if status != NO_ERR {
        return Err(map_status(codec, operation, status));
    }
    let ptr = NonNull::new(raw.cast_mut())
        .ok_or_else(|| decode_err(codec, format!("{operation} returned no format description")))?;
    // SAFETY: the call succeeded, so `ptr` is a format description the Create
    // rule already gave us a +1 reference to; `CFRetained::from_raw` adopts
    // that reference and releases it exactly once on drop.
    Ok(unsafe { CFRetained::from_raw(ptr) })
}

/// Build the format description for an HEVC still from its `hvcC` record.
pub(super) fn hevc_format_description(
    hvcc_bytes: &[u8],
    request: &StillDecodeRequest<'_>,
) -> Result<CFRetained<CMFormatDescription>, HwDecodeError> {
    let codec = HwCodec::Hevc;
    let to_err = |e: crate::bitstream::bits::ParseError| decode_err(codec, e.0);
    let hvcc = hevc::parse_hvcc(hvcc_bytes).map_err(to_err)?;

    // Parameter sets from the hvcC arrays, in record order.
    let mut sets: Vec<&[u8]> = Vec::new();
    let (mut has_vps, mut has_sps, mut has_pps) = (false, false, false);
    for nal in &hvcc.nal_units {
        let nal_type = hevc::nal_type(nal).map_err(to_err)?;
        if !PARAMETER_SET_NALS.contains(&nal_type) {
            continue;
        }
        match nal_type {
            32 => has_vps = true,
            33 => has_sps = true,
            34 => has_pps = true,
            _ => {}
        }
        sets.push(nal.as_slice());
    }

    // Some encoders leave one or more parameter sets out of the hvcC arrays
    // and send them in band instead; the VAAPI backend accepts either source,
    // so this one does too.
    if !(has_vps && has_sps && has_pps) {
        let payload_nals =
            hevc::split_length_prefixed(request.payload, hvcc.nal_length_size).map_err(to_err)?;
        for nal in payload_nals {
            let nal_type = hevc::nal_type(nal).map_err(to_err)?;
            let wanted = match nal_type {
                32 if !has_vps => {
                    has_vps = true;
                    true
                }
                33 if !has_sps => {
                    has_sps = true;
                    true
                }
                34 if !has_pps => {
                    has_pps = true;
                    true
                }
                _ => false,
            };
            if wanted {
                sets.push(nal);
            }
        }
    }

    if !(has_vps && has_sps && has_pps) {
        return Err(decode_err(
            codec,
            "hvcC and payload carry no complete VPS/SPS/PPS set \
             (VideoToolbox requires all three)",
        ));
    }

    let pointers: Vec<NonNull<u8>> = sets
        .iter()
        .map(|nal| {
            NonNull::new(nal.as_ptr().cast_mut())
                .ok_or_else(|| decode_err(codec, "empty parameter set NAL"))
        })
        .collect::<Result<_, _>>()?;
    let sizes: Vec<usize> = sets.iter().map(|nal| nal.len()).collect();

    let mut raw: *const CMFormatDescription = ptr::null();
    // SAFETY: `pointers` and `sizes` are non-empty parallel arrays of exactly
    // `pointers.len()` entries. Each pointer is the start of a live parameter
    // set NAL borrowed from `hvcc` or `request.payload`, both of which outlive
    // this call, and the matching `sizes` entry is that NAL's length.
    // `nal_length_size` is 1, 2 or 4 (`parse_hvcc` rejects 3). `raw` is a live
    // local that the framework writes exactly one +1 reference into.
    let status = unsafe {
        CMVideoFormatDescriptionCreateFromHEVCParameterSets(
            None,
            pointers.len(),
            NonNull::from(&pointers[0]),
            NonNull::from(&sizes[0]),
            hvcc.nal_length_size as c_int,
            None,
            NonNull::from(&mut raw),
        )
    };
    adopt(
        codec,
        "CMVideoFormatDescriptionCreateFromHEVCParameterSets",
        status,
        raw,
    )
}

/// `OBU_SEQUENCE_HEADER` (AV1 §6.2.2).
const OBU_SEQUENCE_HEADER: u8 = 1;

/// Encode a length as a LEB128 `obu_size` field (AV1 §4.10.5).
fn leb128(mut value: usize, out: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Re-frame a sequence-header payload as a canonical OBU with a size field.
///
/// `split_obus` hands back payloads rather than byte ranges, so a sequence
/// header recovered from the sample is rebuilt here instead of being sliced
/// out of the original stream. The result is byte-identical in meaning: OBU
/// type 1, `obu_has_size_field = 1`, no extension.
fn sequence_header_obu(payload: &[u8]) -> Vec<u8> {
    let mut obu = Vec::with_capacity(payload.len() + 3);
    obu.push((OBU_SEQUENCE_HEADER << 3) | 0b10); // type, obu_has_size_field
    leb128(payload.len(), &mut obu);
    obu.extend_from_slice(payload);
    obu
}

/// The sequence header for an AV1 still, from `av1C.configOBUs` if it is
/// there and from the sample otherwise.
///
/// AVIF permits the sequence header in either place, but **VideoToolbox
/// requires it in the `av1C` config atom**: given a record whose `configOBUs`
/// are empty it fails `VTDecompressionSessionCreate` outright with
/// `kVTVideoDecoderMalfunctionErr`, at every picture size. So when the sample
/// is carrying it, the record is rebuilt with it (see [`av1_format_description`]).
fn av1_sequence_header(
    config_obus: &[u8],
    payload: &[u8],
) -> Result<(av1::SequenceHeader, Option<Vec<u8>>), HwDecodeError> {
    let codec = HwCodec::Av1;
    let to_err = |e: crate::bitstream::bits::ParseError| decode_err(codec, e.0);
    // `parse_sequence_header` takes an OBU *payload*, so the stream has to be
    // split first — handing it the raw stream would parse the OBU header bytes
    // as sequence-header fields and yield nonsense dimensions.
    for (source, from_config) in [(config_obus, true), (payload, false)] {
        if source.is_empty() {
            continue;
        }
        let obus = av1::split_obus(source).map_err(to_err)?;
        let Some(obu) = obus.iter().find(|obu| obu.obu_type == OBU_SEQUENCE_HEADER) else {
            continue;
        };
        let seq = av1::parse_sequence_header(obu.payload).map_err(to_err)?;
        let rebuilt = (!from_config).then(|| sequence_header_obu(obu.payload));
        return Ok((seq, rebuilt));
    }
    Err(decode_err(
        codec,
        "AV1 stream carries no sequence header in either the av1C record or \
         the sample; VideoToolbox cannot be configured without one",
    ))
}

/// Build the format description for an AV1 still from its `av1C` record.
pub(super) fn av1_format_description(
    av1c_bytes: &[u8],
    request: &StillDecodeRequest<'_>,
) -> Result<CFRetained<CMFormatDescription>, HwDecodeError> {
    let codec = HwCodec::Av1;
    let to_err = |e: crate::bitstream::bits::ParseError| decode_err(codec, e.0);
    let av1c = av1::parse_av1c(av1c_bytes).map_err(to_err)?;
    if av1c.seq_profile != 0 {
        return Err(decode_err(
            codec,
            format!(
                "AV1 profile {} is outside the still-picture scope \
                 (VideoToolbox decodes Main / Profile 0)",
                av1c.seq_profile
            ),
        ));
    }

    let (seq, rebuilt) = av1_sequence_header(av1c.config_obus, request.payload)?;

    // The sequence header is authoritative for the coded size — and it is what
    // VideoToolbox itself parses. The container's advisory `ispe` size is only
    // a fallback, because it can legitimately disagree (rotation, cropping).
    let (width, height) = match (seq.max_frame_width, seq.max_frame_height) {
        (w, h) if w > 0 && h > 0 => (w, h),
        _ if request.width > 0 && request.height > 0 => (request.width, request.height),
        _ => {
            return Err(decode_err(
                codec,
                "AV1 sequence header declares no picture size and the container \
                 supplied none",
            ));
        }
    };
    let width = i32::try_from(width)
        .map_err(|_| decode_err(codec, "AV1 picture width does not fit in i32"))?;
    let height = i32::try_from(height)
        .map_err(|_| decode_err(codec, "AV1 picture height does not fit in i32"))?;

    // `SampleDescriptionExtensionAtoms = { "av1C": <record body> }` — the
    // whole record including its four-byte header, not just `configOBUs`. When
    // the sequence header lived in the sample rather than the record, it is
    // appended here so VideoToolbox gets the config it insists on.
    let record: Vec<u8>;
    let atom_bytes = match rebuilt {
        Some(seq_obu) => {
            record = av1c_bytes
                .get(..4)
                .unwrap_or(av1c_bytes)
                .iter()
                .copied()
                .chain(seq_obu)
                .collect();
            record.as_slice()
        }
        None => av1c_bytes,
    };
    let atom_key = CFString::from_static_str("av1C");
    let atom_value = CFData::from_bytes(atom_bytes);
    let atoms: CFRetained<CFDictionary<CFString, CFType>> =
        CFDictionary::from_slices(&[&*atom_key], &[atom_value.as_ref() as &CFType]);
    // SAFETY: an immortal constant `CFStringRef` exported by CoreMedia.
    let atoms_key = unsafe { kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms };
    let extensions: CFRetained<CFDictionary<CFString, CFType>> =
        CFDictionary::from_slices(&[atoms_key], &[atoms.as_ref() as &CFType]);

    let mut raw: *const CMFormatDescription = ptr::null();
    // SAFETY: `extensions` is a live dictionary whose keys are `CFString` and
    // whose values are CF objects, which is what the "extensions generics must
    // be of the correct type" precondition asks for; it is deep-copied by the
    // call, so it need not outlive it. `raw` is a live local the framework
    // writes exactly one +1 reference into.
    let status = unsafe {
        CMVideoFormatDescriptionCreate(
            None,
            kCMVideoCodecType_AV1,
            width,
            height,
            Some(as_opaque_dict(&extensions)),
            NonNull::from(&mut raw),
        )
    };
    adopt(codec, "CMVideoFormatDescriptionCreate", status, raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChromaSubsampling, CodecConfig};

    fn request<'a>(config: CodecConfig<'a>, payload: &'a [u8]) -> StillDecodeRequest<'a> {
        StillDecodeRequest {
            config,
            payload,
            width: 0,
            height: 0,
            bit_depth: 8,
            chroma: ChromaSubsampling::Cs420,
        }
    }

    /// A 23-byte hvcC header plus one array per parameter set, built the way
    /// `tests/vaapi_device.rs` builds them from an Annex-B stream.
    fn hvcc_with(sets: &[(u8, &[u8])]) -> Vec<u8> {
        let mut hvcc = vec![0u8; 23];
        hvcc[0] = 1; // configurationVersion
        hvcc[21] = 0x03; // lengthSizeMinusOne = 3 -> 4-byte prefixes
        hvcc[22] = sets.len() as u8;
        for (nal_type, nal) in sets {
            hvcc.push(0x80 | nal_type);
            hvcc.extend_from_slice(&1u16.to_be_bytes());
            hvcc.extend_from_slice(&(nal.len() as u16).to_be_bytes());
            hvcc.extend_from_slice(nal);
        }
        hvcc
    }

    /// A minimal NAL with the given type in its two-byte header.
    fn nal(nal_type: u8) -> Vec<u8> {
        vec![nal_type << 1, 0x01, 0xAA, 0xBB]
    }

    #[test]
    fn hevc_requires_a_complete_parameter_set() {
        // VPS + SPS but no PPS, and nothing in band to make up for it.
        let vps = nal(32);
        let sps = nal(33);
        let hvcc = hvcc_with(&[(32, &vps), (33, &sps)]);
        let err = hevc_format_description(&hvcc, &request(CodecConfig::Hvcc(&hvcc), &[]))
            .expect_err("an incomplete parameter set must be rejected");
        assert!(
            err.to_string().contains("VPS/SPS/PPS"),
            "message must say what is missing: {err}"
        );
    }

    #[test]
    fn av1_rejects_profiles_outside_main() {
        // av1C: marker/version, seq_profile = 1 in the top three bits.
        let av1c = [0x81u8, 0x20, 0x0c, 0x00];
        let err = av1_format_description(&av1c, &request(CodecConfig::Av1c(&av1c), &[]))
            .expect_err("profile 1 must be rejected");
        assert!(
            err.to_string().contains("profile 1"),
            "message must name the profile: {err}"
        );
    }

    #[test]
    fn av1_without_any_sequence_header_is_rejected() {
        let av1c = [0x81u8, 0x00, 0x0c, 0x00];
        let err = av1_format_description(&av1c, &request(CodecConfig::Av1c(&av1c), &[]))
            .expect_err("no sequence header anywhere must be an error");
        assert!(
            err.to_string().contains("no sequence header"),
            "message must say what is missing: {err}"
        );
    }

    #[test]
    fn leb128_matches_the_spec_encoding() {
        let mut out = Vec::new();
        leb128(0, &mut out);
        assert_eq!(out, [0x00]);
        out.clear();
        leb128(127, &mut out);
        assert_eq!(out, [0x7f]);
        out.clear();
        leb128(128, &mut out);
        assert_eq!(out, [0x80, 0x01]);
        out.clear();
        leb128(300, &mut out);
        assert_eq!(out, [0xac, 0x02]);
    }

    #[test]
    fn rebuilt_sequence_header_obu_round_trips() {
        let payload = [0xAAu8; 13];
        let obu = sequence_header_obu(&payload);
        // type 1, obu_has_size_field set, no extension.
        assert_eq!(obu[0], 0x0A);
        let parsed = av1::split_obus(&obu).expect("rebuilt OBU parses");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].obu_type, OBU_SEQUENCE_HEADER);
        assert_eq!(parsed[0].payload, &payload[..]);
    }
}
