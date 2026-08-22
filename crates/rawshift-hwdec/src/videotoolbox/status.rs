//! `OSStatus` naming and the mapping from platform status codes to
//! [`HwDecodeError`].
//!
//! **Safe Rust only** — this module never calls into a framework; it just
//! interprets the integers they return.
//!
//! The `MacTypes.h` aliases are re-declared here because the objc2 crates
//! keep theirs `pub(crate)`. They are plain integer aliases, so naming them
//! locally is a readability convenience, not a redefinition of the ABI.

use crate::{HwCodec, HwDecodeError};

/// `MacTypes.h` `OSStatus` — the return type of every VideoToolbox and
/// CoreMedia entry point.
pub(super) type OSStatus = i32;

/// `MacTypes.h` `OSType` — a four-character code, e.g. a pixel format.
pub(super) type OSType = u32;

/// `noErr`: the operation succeeded.
pub(super) const NO_ERR: OSStatus = 0;

// ── VideoToolbox status codes (VTErrors.h) ──────────────────────────────────
//
// Transcribed from `objc2-video-toolbox`'s generated `VTErrors.rs`, which in
// turn mirrors the SDK header. The `Unavailable` set below is the subset that
// means "this machine cannot decode this codec/configuration right now"; every
// other status describes a failed decode of data we did hand over.

const K_VT_PROPERTY_NOT_SUPPORTED_ERR: OSStatus = -12900;
const K_VT_PROPERTY_READ_ONLY_ERR: OSStatus = -12901;
const K_VT_PARAMETER_ERR: OSStatus = -12902;
const K_VT_INVALID_SESSION_ERR: OSStatus = -12903;
const K_VT_ALLOCATION_FAILED_ERR: OSStatus = -12904;
const K_VT_PIXEL_TRANSFER_NOT_SUPPORTED_ERR: OSStatus = -12905;
const K_VT_COULD_NOT_FIND_VIDEO_DECODER_ERR: OSStatus = -12906;
const K_VT_COULD_NOT_CREATE_INSTANCE_ERR: OSStatus = -12907;
const K_VT_COULD_NOT_FIND_VIDEO_ENCODER_ERR: OSStatus = -12908;
const K_VT_VIDEO_DECODER_BAD_DATA_ERR: OSStatus = -12909;
const K_VT_VIDEO_DECODER_UNSUPPORTED_DATA_FORMAT_ERR: OSStatus = -12910;
const K_VT_VIDEO_DECODER_MALFUNCTION_ERR: OSStatus = -12911;
const K_VT_VIDEO_ENCODER_MALFUNCTION_ERR: OSStatus = -12912;
const K_VT_VIDEO_DECODER_NOT_AVAILABLE_NOW_ERR: OSStatus = -12913;
const K_VT_PIXEL_ROTATION_NOT_SUPPORTED_ERR: OSStatus = -12914;
const K_VT_VIDEO_ENCODER_NOT_AVAILABLE_NOW_ERR: OSStatus = -12915;
const K_VT_FORMAT_DESCRIPTION_CHANGE_NOT_SUPPORTED_ERR: OSStatus = -12916;
const K_VT_INSUFFICIENT_SOURCE_COLOR_DATA_ERR: OSStatus = -12917;
const K_VT_COULD_NOT_CREATE_COLOR_CORRECTION_DATA_ERR: OSStatus = -12918;
const K_VT_COLOR_SYNC_TRANSFORM_CONVERT_FAILED_ERR: OSStatus = -12919;
const K_VT_VIDEO_DECODER_AUTHORIZATION_ERR: OSStatus = -12210;
const K_VT_VIDEO_ENCODER_AUTHORIZATION_ERR: OSStatus = -12211;
const K_VT_COLOR_CORRECTION_PIXEL_TRANSFER_FAILED_ERR: OSStatus = -12212;
const K_VT_MULTI_PASS_STORAGE_IDENTIFIER_MISMATCH_ERR: OSStatus = -12213;
const K_VT_MULTI_PASS_STORAGE_INVALID_ERR: OSStatus = -12214;
const K_VT_FRAME_SILO_INVALID_TIME_STAMP_ERR: OSStatus = -12215;
const K_VT_FRAME_SILO_INVALID_TIME_RANGE_ERR: OSStatus = -12216;
const K_VT_COULD_NOT_FIND_TEMPORAL_FILTER_ERR: OSStatus = -12217;
const K_VT_PIXEL_TRANSFER_NOT_PERMITTED_ERR: OSStatus = -12218;
const K_VT_COLOR_CORRECTION_IMAGE_ROTATION_FAILED_ERR: OSStatus = -12219;
const K_VT_VIDEO_DECODER_REMOVED_ERR: OSStatus = -17690;
const K_VT_SESSION_MALFUNCTION_ERR: OSStatus = -17691;
const K_VT_VIDEO_DECODER_NEEDS_ROSETTA_ERR: OSStatus = -17692;
const K_VT_VIDEO_ENCODER_NEEDS_ROSETTA_ERR: OSStatus = -17693;
const K_VT_VIDEO_DECODER_REFERENCE_MISSING_ERR: OSStatus = -17694;
const K_VT_VIDEO_DECODER_CALLBACK_MESSAGING_ERR: OSStatus = -17695;
const K_VT_VIDEO_DECODER_UNKNOWN_ERR: OSStatus = -17696;
const K_VT_EXTENSION_DISABLED_ERR: OSStatus = -17697;
const K_VT_VIDEO_ENCODER_MV_REQUIRED_ERR: OSStatus = -17698;

// ── CoreMedia status codes (CMBlockBuffer.h / CMSampleBuffer.h) ─────────────

const K_CM_BLOCK_BUFFER_STRUCTURE_ALLOCATION_FAILED_ERR: OSStatus = -12700;
const K_CM_BLOCK_BUFFER_BLOCK_ALLOCATION_FAILED_ERR: OSStatus = -12701;
const K_CM_BLOCK_BUFFER_BAD_CUSTOM_BLOCK_SOURCE_ERR: OSStatus = -12702;
const K_CM_BLOCK_BUFFER_BAD_OFFSET_PARAMETER_ERR: OSStatus = -12703;
const K_CM_BLOCK_BUFFER_BAD_LENGTH_PARAMETER_ERR: OSStatus = -12704;
const K_CM_BLOCK_BUFFER_BAD_POINTER_PARAMETER_ERR: OSStatus = -12705;
const K_CM_BLOCK_BUFFER_EMPTY_B_BUF_ERR: OSStatus = -12706;
const K_CM_BLOCK_BUFFER_UNALLOCATED_BLOCK_ERR: OSStatus = -12707;
const K_CM_BLOCK_BUFFER_INSUFFICIENT_SPACE_ERR: OSStatus = -12708;
const K_CM_SAMPLE_BUFFER_ERROR_ALLOCATION_FAILED: OSStatus = -12730;
const K_CM_SAMPLE_BUFFER_ERROR_REQUIRED_PARAMETER_MISSING: OSStatus = -12731;
const K_CM_SAMPLE_BUFFER_ERROR_INVALID_MEDIA_TYPE_FOR_OPERATION: OSStatus = -12734;
const K_CM_SAMPLE_BUFFER_ERROR_INVALID_SAMPLE_DATA: OSStatus = -12735;
const K_CM_FORMAT_DESCRIPTION_ERROR_INVALID_PARAMETER: OSStatus = -12710;

/// Statuses that mean "no usable hardware decoder for this codec or this
/// configuration on this machine", as opposed to "the data we handed over did
/// not decode".
///
/// These become [`HwDecodeError::Unavailable`], which
/// `rawshift-image-heic`/`-avif` surface as `RawError::HwDecoderUnavailable`
/// — the matchable, honest "this build/machine cannot do it" signal — rather
/// than a decode failure that blames the file.
const UNAVAILABLE: &[OSStatus] = &[
    // No decoder exists for the codec. On macOS this is also what
    // `RequireHardwareAcceleratedVideoDecoder` produces when the machine has
    // no hardware decode block for the codec.
    K_VT_COULD_NOT_FIND_VIDEO_DECODER_ERR,
    // A decoder exists but could not be instantiated.
    K_VT_COULD_NOT_CREATE_INSTANCE_ERR,
    // The hardware will not accept this profile / bit depth / chroma.
    K_VT_VIDEO_DECODER_UNSUPPORTED_DATA_FORMAT_ERR,
    // Hardware decode resources are busy — transient, but from the caller's
    // side still "no decoder available".
    K_VT_VIDEO_DECODER_NOT_AVAILABLE_NOW_ERR,
    // The decoder went away mid-session (e.g. an eGPU was unplugged).
    K_VT_VIDEO_DECODER_REMOVED_ERR,
    // The decoder is unusable in this process's architecture.
    K_VT_VIDEO_DECODER_NEEDS_ROSETTA_ERR,
    // The decoder is not authorised for this process.
    K_VT_VIDEO_DECODER_AUTHORIZATION_ERR,
    // The decoder lives in a system extension that is switched off.
    K_VT_EXTENSION_DISABLED_ERR,
];

/// The conventional constant name for a VideoToolbox / CoreMedia status, for
/// error messages that a reader can grep the SDK headers for.
pub(super) fn status_name(status: OSStatus) -> Option<&'static str> {
    let name = match status {
        NO_ERR => "noErr",
        K_VT_PROPERTY_NOT_SUPPORTED_ERR => "kVTPropertyNotSupportedErr",
        K_VT_PROPERTY_READ_ONLY_ERR => "kVTPropertyReadOnlyErr",
        K_VT_PARAMETER_ERR => "kVTParameterErr",
        K_VT_INVALID_SESSION_ERR => "kVTInvalidSessionErr",
        K_VT_ALLOCATION_FAILED_ERR => "kVTAllocationFailedErr",
        K_VT_PIXEL_TRANSFER_NOT_SUPPORTED_ERR => "kVTPixelTransferNotSupportedErr",
        K_VT_COULD_NOT_FIND_VIDEO_DECODER_ERR => "kVTCouldNotFindVideoDecoderErr",
        K_VT_COULD_NOT_CREATE_INSTANCE_ERR => "kVTCouldNotCreateInstanceErr",
        K_VT_COULD_NOT_FIND_VIDEO_ENCODER_ERR => "kVTCouldNotFindVideoEncoderErr",
        K_VT_VIDEO_DECODER_BAD_DATA_ERR => "kVTVideoDecoderBadDataErr",
        K_VT_VIDEO_DECODER_UNSUPPORTED_DATA_FORMAT_ERR => "kVTVideoDecoderUnsupportedDataFormatErr",
        K_VT_VIDEO_DECODER_MALFUNCTION_ERR => "kVTVideoDecoderMalfunctionErr",
        K_VT_VIDEO_ENCODER_MALFUNCTION_ERR => "kVTVideoEncoderMalfunctionErr",
        K_VT_VIDEO_DECODER_NOT_AVAILABLE_NOW_ERR => "kVTVideoDecoderNotAvailableNowErr",
        K_VT_PIXEL_ROTATION_NOT_SUPPORTED_ERR => "kVTPixelRotationNotSupportedErr",
        K_VT_VIDEO_ENCODER_NOT_AVAILABLE_NOW_ERR => "kVTVideoEncoderNotAvailableNowErr",
        K_VT_FORMAT_DESCRIPTION_CHANGE_NOT_SUPPORTED_ERR => {
            "kVTFormatDescriptionChangeNotSupportedErr"
        }
        K_VT_INSUFFICIENT_SOURCE_COLOR_DATA_ERR => "kVTInsufficientSourceColorDataErr",
        K_VT_COULD_NOT_CREATE_COLOR_CORRECTION_DATA_ERR => {
            "kVTCouldNotCreateColorCorrectionDataErr"
        }
        K_VT_COLOR_SYNC_TRANSFORM_CONVERT_FAILED_ERR => "kVTColorSyncTransformConvertFailedErr",
        K_VT_VIDEO_DECODER_AUTHORIZATION_ERR => "kVTVideoDecoderAuthorizationErr",
        K_VT_VIDEO_ENCODER_AUTHORIZATION_ERR => "kVTVideoEncoderAuthorizationErr",
        K_VT_COLOR_CORRECTION_PIXEL_TRANSFER_FAILED_ERR => {
            "kVTColorCorrectionPixelTransferFailedErr"
        }
        K_VT_MULTI_PASS_STORAGE_IDENTIFIER_MISMATCH_ERR => {
            "kVTMultiPassStorageIdentifierMismatchErr"
        }
        K_VT_MULTI_PASS_STORAGE_INVALID_ERR => "kVTMultiPassStorageInvalidErr",
        K_VT_FRAME_SILO_INVALID_TIME_STAMP_ERR => "kVTFrameSiloInvalidTimeStampErr",
        K_VT_FRAME_SILO_INVALID_TIME_RANGE_ERR => "kVTFrameSiloInvalidTimeRangeErr",
        K_VT_COULD_NOT_FIND_TEMPORAL_FILTER_ERR => "kVTCouldNotFindTemporalFilterErr",
        K_VT_PIXEL_TRANSFER_NOT_PERMITTED_ERR => "kVTPixelTransferNotPermittedErr",
        K_VT_COLOR_CORRECTION_IMAGE_ROTATION_FAILED_ERR => {
            "kVTColorCorrectionImageRotationFailedErr"
        }
        K_VT_VIDEO_DECODER_REMOVED_ERR => "kVTVideoDecoderRemovedErr",
        K_VT_SESSION_MALFUNCTION_ERR => "kVTSessionMalfunctionErr",
        K_VT_VIDEO_DECODER_NEEDS_ROSETTA_ERR => "kVTVideoDecoderNeedsRosettaErr",
        K_VT_VIDEO_ENCODER_NEEDS_ROSETTA_ERR => "kVTVideoEncoderNeedsRosettaErr",
        K_VT_VIDEO_DECODER_REFERENCE_MISSING_ERR => "kVTVideoDecoderReferenceMissingErr",
        K_VT_VIDEO_DECODER_CALLBACK_MESSAGING_ERR => "kVTVideoDecoderCallbackMessagingErr",
        K_VT_VIDEO_DECODER_UNKNOWN_ERR => "kVTVideoDecoderUnknownErr",
        K_VT_EXTENSION_DISABLED_ERR => "kVTExtensionDisabledErr",
        K_VT_VIDEO_ENCODER_MV_REQUIRED_ERR => "kVTVideoEncoderMVRequiredErr",
        K_CM_BLOCK_BUFFER_STRUCTURE_ALLOCATION_FAILED_ERR => {
            "kCMBlockBufferStructureAllocationFailedErr"
        }
        K_CM_BLOCK_BUFFER_BLOCK_ALLOCATION_FAILED_ERR => "kCMBlockBufferBlockAllocationFailedErr",
        K_CM_BLOCK_BUFFER_BAD_CUSTOM_BLOCK_SOURCE_ERR => "kCMBlockBufferBadCustomBlockSourceErr",
        K_CM_BLOCK_BUFFER_BAD_OFFSET_PARAMETER_ERR => "kCMBlockBufferBadOffsetParameterErr",
        K_CM_BLOCK_BUFFER_BAD_LENGTH_PARAMETER_ERR => "kCMBlockBufferBadLengthParameterErr",
        K_CM_BLOCK_BUFFER_BAD_POINTER_PARAMETER_ERR => "kCMBlockBufferBadPointerParameterErr",
        K_CM_BLOCK_BUFFER_EMPTY_B_BUF_ERR => "kCMBlockBufferEmptyBBufErr",
        K_CM_BLOCK_BUFFER_UNALLOCATED_BLOCK_ERR => "kCMBlockBufferUnallocatedBlockErr",
        K_CM_BLOCK_BUFFER_INSUFFICIENT_SPACE_ERR => "kCMBlockBufferInsufficientSpaceErr",
        K_CM_SAMPLE_BUFFER_ERROR_ALLOCATION_FAILED => "kCMSampleBufferError_AllocationFailed",
        K_CM_SAMPLE_BUFFER_ERROR_REQUIRED_PARAMETER_MISSING => {
            "kCMSampleBufferError_RequiredParameterMissing"
        }
        K_CM_SAMPLE_BUFFER_ERROR_INVALID_MEDIA_TYPE_FOR_OPERATION => {
            "kCMSampleBufferError_InvalidMediaTypeForOperation"
        }
        K_CM_SAMPLE_BUFFER_ERROR_INVALID_SAMPLE_DATA => "kCMSampleBufferError_InvalidSampleData",
        K_CM_FORMAT_DESCRIPTION_ERROR_INVALID_PARAMETER => {
            "kCMFormatDescriptionError_InvalidParameter"
        }
        _ => return None,
    };
    Some(name)
}

/// Render a status for a human: the SDK constant name when we know it, always
/// with the numeric value so an unknown code is still actionable.
pub(super) fn describe(status: OSStatus) -> String {
    match status_name(status) {
        Some(name) => format!("{name} ({status})"),
        None => format!("OSStatus {status}"),
    }
}

/// Map a failed platform call to the crate's error type.
///
/// `operation` names the C entry point that failed, mirroring the VAAPI
/// backend's `format!("vaCreateConfig: {}", ...)` convention.
pub(super) fn map_status(codec: HwCodec, operation: &str, status: OSStatus) -> HwDecodeError {
    if UNAVAILABLE.contains(&status) {
        HwDecodeError::Unavailable {
            codec,
            reason: format!("{operation}: {}", describe(status)),
        }
    } else {
        HwDecodeError::Decode {
            codec,
            message: format!("{operation}: {}", describe(status)),
        }
    }
}

/// `HwDecodeError::Decode` shorthand, matching the VAAPI backend's helper.
pub(super) fn decode_err(codec: HwCodec, message: impl Into<String>) -> HwDecodeError {
    HwDecodeError::Decode {
        codec,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_statuses_map_to_unavailable() {
        for &status in UNAVAILABLE {
            let err = map_status(HwCodec::Av1, "VTDecompressionSessionCreate", status);
            assert!(
                matches!(err, HwDecodeError::Unavailable { .. }),
                "{status} must be Unavailable, got {err:?}"
            );
            // The message must stay actionable: it names the failed call.
            assert!(err.to_string().contains("VTDecompressionSessionCreate"));
        }
    }

    #[test]
    fn decode_statuses_map_to_decode() {
        for status in [
            K_VT_VIDEO_DECODER_BAD_DATA_ERR,
            K_VT_VIDEO_DECODER_MALFUNCTION_ERR,
            K_VT_PARAMETER_ERR,
            K_VT_INVALID_SESSION_ERR,
            K_VT_VIDEO_DECODER_REFERENCE_MISSING_ERR,
            K_CM_BLOCK_BUFFER_BAD_POINTER_PARAMETER_ERR,
            // An unknown code must still be an error, never a panic.
            -99_999,
        ] {
            let err = map_status(HwCodec::Hevc, "VTDecompressionSessionDecodeFrame", status);
            assert!(
                matches!(err, HwDecodeError::Decode { .. }),
                "{status} must be Decode, got {err:?}"
            );
        }
    }

    /// The four codes the design most easily transposes — `-12911` is
    /// *Malfunction*, `-12913` is *NotAvailableNow*. Getting these backwards
    /// would silently move a hardware-busy failure into the "bad file" bucket.
    #[test]
    fn status_names_match_the_headers() {
        assert_eq!(status_name(-12906), Some("kVTCouldNotFindVideoDecoderErr"));
        assert_eq!(
            status_name(-12910),
            Some("kVTVideoDecoderUnsupportedDataFormatErr")
        );
        assert_eq!(status_name(-12911), Some("kVTVideoDecoderMalfunctionErr"));
        assert_eq!(
            status_name(-12913),
            Some("kVTVideoDecoderNotAvailableNowErr")
        );
        assert_eq!(status_name(-99_999), None);
    }

    #[test]
    fn describe_always_carries_the_numeric_code() {
        assert_eq!(describe(-12906), "kVTCouldNotFindVideoDecoderErr (-12906)");
        assert_eq!(describe(-99_999), "OSStatus -99999");
    }

    /// `NotAvailableNow` is "no decoder right now", not "bad data" — the
    /// distinction the HEIC/AVIF adapters rely on to report
    /// `RawError::HwDecoderUnavailable` instead of blaming the file.
    #[test]
    fn busy_hardware_is_unavailable_not_a_decode_failure() {
        let err = map_status(
            HwCodec::Hevc,
            "op",
            K_VT_VIDEO_DECODER_NOT_AVAILABLE_NOW_ERR,
        );
        assert!(matches!(err, HwDecodeError::Unavailable { .. }));
        let err = map_status(HwCodec::Hevc, "op", K_VT_VIDEO_DECODER_MALFUNCTION_ERR);
        assert!(matches!(err, HwDecodeError::Decode { .. }));
    }
}
