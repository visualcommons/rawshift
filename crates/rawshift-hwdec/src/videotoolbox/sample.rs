//! Wrapping a coded picture payload in the `CMSampleBuffer` a decompression
//! session consumes.
//!
//! The payload is wrapped **zero-copy**: `CMBlockBufferCreateWithMemoryBlock`
//! is given `kCFAllocatorNull` as the block allocator, so CoreMedia never
//! takes ownership of or frees the caller's bytes. That is sound because the
//! lifetime is bounded on both ends — the decode is synchronous (no
//! asynchronous or temporal-processing flags, plus an explicit
//! `VTDecompressionSessionWaitForAsynchronousFrames`), and the sample buffer
//! is released before `decode_still` returns.
//!
//! Both codecs share this path unchanged: HEVC payloads are length-prefixed
//! NAL units and AV1 payloads are raw OBU temporal units, and in each case the
//! bytes go to the decoder exactly as the container stored them.

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use objc2_core_foundation::{CFRetained, kCFAllocatorNull};
use objc2_core_media::{
    CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMSampleTimingInfo, CMTime, CMTimeFlags,
};

use super::status::{NO_ERR, decode_err, map_status};
use crate::{HwCodec, HwDecodeError};

/// Presentation timestamp 0/1. A *valid* PTS matters: some decoders drop a
/// frame whose PTS is `kCMTimeInvalid`.
const PTS_ZERO: CMTime = CMTime {
    value: 0,
    timescale: 1,
    flags: CMTimeFlags::Valid,
    epoch: 0,
};

/// `kCMTimeInvalid`: a still has no duration, and its samples are already in
/// presentation order so no decode timestamp is meaningful.
const TIME_INVALID: CMTime = CMTime {
    value: 0,
    timescale: 0,
    flags: CMTimeFlags::empty(),
    epoch: 0,
};

/// A ready sample buffer and the block buffer it reads through.
///
/// Field order is load-bearing: `buffer` is declared before `_block` so the
/// sample buffer is released first, and the block buffer it references
/// outlives it.
pub(super) struct Sample {
    pub(super) buffer: CFRetained<CMSampleBuffer>,
    _block: CFRetained<CMBlockBuffer>,
}

/// Wrap `payload` in a one-sample `CMSampleBuffer` described by `format`.
pub(super) fn wrap(
    codec: HwCodec,
    format: &CMFormatDescription,
    payload: &[u8],
) -> Result<Sample, HwDecodeError> {
    if payload.is_empty() {
        return Err(decode_err(codec, "coded picture payload is empty"));
    }

    let mut raw_block: *mut CMBlockBuffer = ptr::null_mut();
    // SAFETY: `payload` is a live immutable slice for the whole of this call
    // and of the decode that consumes the returned sample. `kCFAllocatorNull`
    // tells CoreMedia never to free those bytes, and the decoder only reads
    // them. The `*const -> *mut` cast is provenance-only: nothing writes
    // through the pointer. `custom_block_source` is null, which the API
    // permits when a memory block is supplied, and `raw_block` is a live local
    // the framework writes exactly one +1 reference into.
    let status = unsafe {
        CMBlockBuffer::create_with_memory_block(
            None,
            payload.as_ptr().cast_mut().cast::<c_void>(),
            payload.len(),
            kCFAllocatorNull,
            ptr::null(),
            0,
            payload.len(),
            0,
            NonNull::from(&mut raw_block),
        )
    };
    if status != NO_ERR {
        return Err(map_status(
            codec,
            "CMBlockBufferCreateWithMemoryBlock",
            status,
        ));
    }
    let block_ptr = NonNull::new(raw_block).ok_or_else(|| {
        decode_err(
            codec,
            "CMBlockBufferCreateWithMemoryBlock returned no buffer",
        )
    })?;
    // SAFETY: the call succeeded, so `block_ptr` carries the +1 reference the
    // Create rule granted; `from_raw` adopts it and releases it once on drop.
    let block = unsafe { CFRetained::from_raw(block_ptr) };

    let timing = CMSampleTimingInfo {
        duration: TIME_INVALID,
        presentationTimeStamp: PTS_ZERO,
        decodeTimeStamp: TIME_INVALID,
    };
    let sample_size = payload.len();

    let mut raw_sample: *mut CMSampleBuffer = ptr::null_mut();
    // SAFETY: one sample spanning the whole block buffer, so the timing and
    // size arrays each have exactly the single entry their count declares and
    // both outlive the call (the framework copies them). `block` and `format`
    // are live. `raw_sample` is a live local the framework writes exactly one
    // +1 reference into.
    let status = unsafe {
        CMSampleBuffer::create_ready(
            None,
            Some(&block),
            Some(format),
            1,
            1,
            &raw const timing,
            1,
            &raw const sample_size,
            NonNull::from(&mut raw_sample),
        )
    };
    if status != NO_ERR {
        return Err(map_status(codec, "CMSampleBufferCreateReady", status));
    }
    let sample_ptr = NonNull::new(raw_sample)
        .ok_or_else(|| decode_err(codec, "CMSampleBufferCreateReady returned no buffer"))?;
    // SAFETY: as above — adopt the Create-rule +1 reference.
    let buffer = unsafe { CFRetained::from_raw(sample_ptr) };

    Ok(Sample {
        buffer,
        _block: block,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A still carries no duration and no decode timestamp, but its
    /// presentation timestamp must be *valid* or decoders may drop the frame.
    #[test]
    fn still_timing_has_a_valid_pts_and_invalid_duration() {
        assert!(PTS_ZERO.flags.contains(CMTimeFlags::Valid));
        assert_eq!(PTS_ZERO.timescale, 1);
        assert!(!TIME_INVALID.flags.contains(CMTimeFlags::Valid));
    }
}
