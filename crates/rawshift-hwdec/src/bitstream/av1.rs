//! AV1 still-picture parsing shared by hardware backends — **safe Rust only**
//! (no `unsafe`; platform FFI stays in backend modules).
//!
//! ## Scope
//!
//! AVIF still images: AV1 **Profile 0** (Main — 8/10-bit 4:2:0 or
//! monochrome), one intra frame (`KEY_FRAME` or `INTRA_ONLY_FRAME`,
//! `show_existing_frame == 0`), any tiling, optional film grain. Inter
//! frames, `OBU_TILE_LIST` (large-scale tile), and profiles 1/2 fail with a
//! clear `HwDecodeError::Decode` message.
//!
//! Structures parsed: the `av1C` AV1CodecConfigurationRecord (AV1-ISOBMFF
//! §2.3), the OBU framing (AV1 §5.3), `sequence_header_obu` (§5.5), the
//! uncompressed frame header (§5.9), and tile group OBUs (§5.11) far enough
//! to fill VAAPI's `VADecPictureParameterBufferAV1` and one
//! `VASliceParameterBufferAV1` per tile.

use super::bits::{BitReader, PResult, ParseError, clip3};
#[cfg(hwdec_backend = "vaapi")]
use crate::vaapi::sys;

// ── OBU framing (§5.3) ──────────────────────────────────────────────────────

pub const OBU_SEQUENCE_HEADER: u8 = 1;
pub const OBU_TEMPORAL_DELIMITER: u8 = 2;
pub const OBU_FRAME_HEADER: u8 = 3;
pub const OBU_TILE_GROUP: u8 = 4;
pub const OBU_METADATA: u8 = 5;
pub const OBU_FRAME: u8 = 6;
pub const OBU_REDUNDANT_FRAME_HEADER: u8 = 7;
pub const OBU_TILE_LIST: u8 = 8;

/// One OBU: its type and payload bytes (header + size field stripped).
#[derive(Debug, Clone, Copy)]
pub struct Obu<'a> {
    pub obu_type: u8,
    pub payload: &'a [u8],
}

/// Split a byte stream into OBUs. An OBU without a size field (legal only
/// for the last OBU of a unit) takes the rest of the stream.
pub fn split_obus(data: &[u8]) -> PResult<Vec<Obu<'_>>> {
    let mut obus = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let b0 = data[pos];
        if b0 & 0x80 != 0 {
            return Err(ParseError("AV1 OBU forbidden bit set"));
        }
        let obu_type = (b0 >> 3) & 0x0f;
        let has_extension = b0 & 0x04 != 0;
        let has_size = b0 & 0x02 != 0;
        pos += 1;
        if has_extension {
            pos += 1; // temporal/spatial id byte
        }
        if pos > data.len() {
            return Err(ParseError("AV1 OBU header truncated"));
        }
        let payload = if has_size {
            let mut r = BitReader::new(&data[pos..]);
            let size = r.leb128()? as usize;
            pos += r.bit_pos() / 8;
            let payload = data
                .get(pos..pos + size)
                .ok_or(ParseError("AV1 OBU payload truncated"))?;
            pos += size;
            payload
        } else {
            let payload = &data[pos..];
            pos = data.len();
            payload
        };
        obus.push(Obu { obu_type, payload });
    }
    Ok(obus)
}

// ── av1C record (AV1-ISOBMFF §2.3) ──────────────────────────────────────────

/// The `av1C` parts the backend needs: the advisory profile and the
/// `configOBUs` bytes (usually the sequence header).
#[derive(Debug)]
pub struct Av1c<'a> {
    pub seq_profile: u8,
    pub config_obus: &'a [u8],
}

/// Parse the raw `av1C` record body.
pub fn parse_av1c(data: &[u8]) -> PResult<Av1c<'_>> {
    if data.len() < 4 {
        return Err(ParseError("av1C record shorter than 4 bytes"));
    }
    if data[0] != 0x81 {
        return Err(ParseError("av1C marker/version is not 1/1"));
    }
    Ok(Av1c {
        seq_profile: data[1] >> 5,
        config_obus: &data[4..],
    })
}

// ── Sequence header (§5.5) ──────────────────────────────────────────────────

/// The sequence-header fields the frame-header parser and VAAPI need.
#[derive(Debug, Clone, Default)]
pub struct SequenceHeader {
    pub seq_profile: u8,
    pub still_picture: bool,
    pub reduced_still_picture_header: bool,
    pub operating_point_idc: Vec<u16>,
    pub decoder_model_present_for_op: Vec<bool>,
    pub decoder_model_info_present: bool,
    pub buffer_delay_length_minus_1: u32,
    pub buffer_removal_time_length_minus_1: u32,
    pub equal_picture_interval: bool,
    pub frame_width_bits: u32,
    pub frame_height_bits: u32,
    pub max_frame_width: u32,
    pub max_frame_height: u32,
    pub frame_id_numbers_present: bool,
    pub delta_frame_id_length: u32,
    pub additional_frame_id_length: u32,
    pub use_128x128_superblock: bool,
    pub enable_filter_intra: bool,
    pub enable_intra_edge_filter: bool,
    pub enable_interintra_compound: bool,
    pub enable_masked_compound: bool,
    pub enable_warped_motion: bool,
    pub enable_dual_filter: bool,
    pub enable_order_hint: bool,
    pub enable_jnt_comp: bool,
    pub enable_ref_frame_mvs: bool,
    /// 2 = per-frame choice (`SELECT_SCREEN_CONTENT_TOOLS`).
    pub seq_force_screen_content_tools: u32,
    /// 2 = per-frame choice (`SELECT_INTEGER_MV`).
    pub seq_force_integer_mv: u32,
    pub order_hint_bits: u32,
    pub enable_superres: bool,
    pub enable_cdef: bool,
    pub enable_restoration: bool,
    pub bit_depth: u8,
    pub mono_chrome: bool,
    pub matrix_coefficients: u8,
    pub color_range_full: bool,
    pub subsampling_x: bool,
    pub subsampling_y: bool,
    pub separate_uv_delta_q: bool,
    pub film_grain_params_present: bool,
}

/// Parse a `sequence_header_obu` payload.
// Fields are assigned in the spec's exact syntax order — clearer for a
// parser than one giant struct literal.
#[allow(clippy::field_reassign_with_default)]
pub fn parse_sequence_header(payload: &[u8]) -> PResult<SequenceHeader> {
    let r = &mut BitReader::new(payload);
    let mut s = SequenceHeader::default();

    s.seq_profile = r.bits(3)? as u8;
    if s.seq_profile != 0 {
        return Err(ParseError(
            "AV1 profile 1/2 is outside the VAAPI backend scope (Profile 0 only)",
        ));
    }
    s.still_picture = r.flag()?;
    s.reduced_still_picture_header = r.flag()?;

    if s.reduced_still_picture_header {
        let _seq_level_idx_0 = r.bits(5)?;
        s.operating_point_idc = vec![0];
        s.decoder_model_present_for_op = vec![false];
    } else {
        let timing_info_present = r.flag()?;
        if timing_info_present {
            // timing_info(): num_units_in_display_tick + time_scale.
            r.skip(64)?;
            s.equal_picture_interval = r.flag()?;
            if s.equal_picture_interval {
                let _num_ticks_per_picture_minus_1 = r.uvlc()?;
            }
            s.decoder_model_info_present = r.flag()?;
            if s.decoder_model_info_present {
                s.buffer_delay_length_minus_1 = r.bits(5)?;
                r.skip(32)?; // num_units_in_decoding_tick
                s.buffer_removal_time_length_minus_1 = r.bits(5)?;
                let _frame_presentation_time_length_minus_1 = r.bits(5)?;
            }
        }
        let initial_display_delay_present = r.flag()?;
        let op_cnt = r.bits(5)? + 1;
        for _ in 0..op_cnt {
            s.operating_point_idc.push(r.bits(12)? as u16);
            let seq_level_idx = r.bits(5)?;
            if seq_level_idx > 7 {
                let _seq_tier = r.bit()?;
            }
            let mut model_present = false;
            if s.decoder_model_info_present {
                model_present = r.flag()?;
                if model_present {
                    let n = (s.buffer_delay_length_minus_1 + 1) as usize;
                    r.skip(n)?; // decoder_buffer_delay
                    r.skip(n)?; // encoder_buffer_delay
                    let _low_delay_mode = r.bit()?;
                }
            }
            s.decoder_model_present_for_op.push(model_present);
            if initial_display_delay_present && r.flag()? {
                let _initial_display_delay_minus_1 = r.bits(4)?;
            }
        }
    }

    s.frame_width_bits = r.bits(4)? + 1;
    s.frame_height_bits = r.bits(4)? + 1;
    s.max_frame_width = r.bits(s.frame_width_bits)? + 1;
    s.max_frame_height = r.bits(s.frame_height_bits)? + 1;
    if !s.reduced_still_picture_header {
        s.frame_id_numbers_present = r.flag()?;
    }
    if s.frame_id_numbers_present {
        s.delta_frame_id_length = r.bits(4)? + 2;
        s.additional_frame_id_length = r.bits(3)? + 1;
    }
    s.use_128x128_superblock = r.flag()?;
    s.enable_filter_intra = r.flag()?;
    s.enable_intra_edge_filter = r.flag()?;

    if s.reduced_still_picture_header {
        s.seq_force_screen_content_tools = 2; // SELECT_SCREEN_CONTENT_TOOLS
        s.seq_force_integer_mv = 2; // SELECT_INTEGER_MV
    } else {
        s.enable_interintra_compound = r.flag()?;
        s.enable_masked_compound = r.flag()?;
        s.enable_warped_motion = r.flag()?;
        s.enable_dual_filter = r.flag()?;
        s.enable_order_hint = r.flag()?;
        if s.enable_order_hint {
            s.enable_jnt_comp = r.flag()?;
            s.enable_ref_frame_mvs = r.flag()?;
        }
        s.seq_force_screen_content_tools = if r.flag()? { 2 } else { r.bit()? };
        s.seq_force_integer_mv = if s.seq_force_screen_content_tools > 0 {
            if r.flag()? { 2 } else { r.bit()? }
        } else {
            2
        };
        if s.enable_order_hint {
            s.order_hint_bits = r.bits(3)? + 1;
        }
    }

    s.enable_superres = r.flag()?;
    s.enable_cdef = r.flag()?;
    s.enable_restoration = r.flag()?;

    // color_config() (§5.5.2).
    let high_bitdepth = r.flag()?;
    // seq_profile == 2 && high_bitdepth would read twelve_bit here; profile 0
    // was enforced above, so BitDepth is 10 or 8.
    s.bit_depth = if high_bitdepth { 10 } else { 8 };
    // seq_profile == 1 forces mono_chrome = 0; profile 0 reads it.
    s.mono_chrome = r.flag()?;
    let color_description_present = r.flag()?;
    let (color_primaries, transfer_characteristics, matrix_coefficients) =
        if color_description_present {
            (r.bits(8)?, r.bits(8)?, r.bits(8)?)
        } else {
            (2, 2, 2) // CP/TC/MC_UNSPECIFIED
        };
    s.matrix_coefficients = matrix_coefficients as u8;
    if s.mono_chrome {
        s.color_range_full = r.flag()?;
        s.subsampling_x = true;
        s.subsampling_y = true;
    } else if color_primaries == 1 && transfer_characteristics == 13 && matrix_coefficients == 0 {
        // sRGB/identity: 4:4:4 full range — requires profile 1.
        return Err(ParseError(
            "AV1 identity-matrix 4:4:4 requires profile 1, outside the backend scope",
        ));
    } else {
        s.color_range_full = r.flag()?;
        // Profile 0: 4:2:0.
        s.subsampling_x = true;
        s.subsampling_y = true;
        if s.subsampling_x && s.subsampling_y {
            let _chroma_sample_position = r.bits(2)?;
        }
    }
    if !s.mono_chrome {
        s.separate_uv_delta_q = r.flag()?;
    }
    s.film_grain_params_present = r.flag()?;
    Ok(s)
}

// ── Frame header (§5.9) ─────────────────────────────────────────────────────

pub const FRAME_KEY: u32 = 0;
pub const FRAME_INTRA_ONLY: u32 = 2;
const PRIMARY_REF_NONE: u32 = 7;

/// Loop-restoration `FrameRestorationType` remap (§5.9.20):
/// `lr_type` 0..=3 → NONE(0) / SWITCHABLE(3) / WIENER(1) / SGRPROJ(2).
const REMAP_LR_TYPE: [u8; 4] = [0, 3, 1, 2];

const SEG_LVL_ALT_Q: usize = 0;
const SEG_LVL_MAX: usize = 8;
const SEGMENTATION_FEATURE_BITS: [u32; SEG_LVL_MAX] = [8, 6, 6, 6, 6, 3, 0, 0];
const SEGMENTATION_FEATURE_SIGNED: [bool; SEG_LVL_MAX] =
    [true, true, true, true, true, false, false, false];
const SEGMENTATION_FEATURE_MAX: [i32; SEG_LVL_MAX] = [255, 63, 63, 63, 63, 7, 0, 0];

/// Everything the VAAPI AV1 picture parameter buffer needs from the
/// uncompressed frame header of one intra frame.
#[derive(Debug, Clone, Default)]
pub struct FrameHeader {
    pub frame_type: u32,
    pub show_frame: bool,
    pub showable_frame: bool,
    pub error_resilient_mode: bool,
    pub disable_cdf_update: bool,
    pub allow_screen_content_tools: bool,
    pub force_integer_mv: bool,
    pub allow_intrabc: bool,
    pub order_hint: u32,
    /// Post-superres (decoded) width; the surface is created at
    /// `upscaled_width` x `frame_height`.
    pub frame_width: u32,
    pub frame_height: u32,
    pub upscaled_width: u32,
    pub use_superres: bool,
    pub superres_denom: u32,
    pub disable_frame_end_update_cdf: bool,
    // Tiles.
    pub uniform_tile_spacing: bool,
    pub tile_cols: u32,
    pub tile_rows: u32,
    pub tile_cols_log2: u32,
    pub tile_rows_log2: u32,
    pub width_in_sbs_minus_1: Vec<u32>,
    pub height_in_sbs_minus_1: Vec<u32>,
    pub context_update_tile_id: u32,
    pub tile_size_bytes: u32,
    // Quantization.
    pub base_q_idx: u32,
    pub delta_q_y_dc: i32,
    pub delta_q_u_dc: i32,
    pub delta_q_u_ac: i32,
    pub delta_q_v_dc: i32,
    pub delta_q_v_ac: i32,
    pub using_qmatrix: bool,
    pub qm_y: u32,
    pub qm_u: u32,
    pub qm_v: u32,
    // Segmentation.
    pub segmentation_enabled: bool,
    pub segmentation_update_map: bool,
    pub segmentation_temporal_update: bool,
    pub segmentation_update_data: bool,
    pub feature_enabled: [[bool; SEG_LVL_MAX]; 8],
    pub feature_data: [[i32; SEG_LVL_MAX]; 8],
    // Delta Q / LF.
    pub delta_q_present: bool,
    pub delta_q_res: u32,
    pub delta_lf_present: bool,
    pub delta_lf_res: u32,
    pub delta_lf_multi: bool,
    // Loop filter.
    pub loop_filter_level: [u32; 4],
    pub loop_filter_sharpness: u32,
    pub loop_filter_delta_enabled: bool,
    pub loop_filter_delta_update: bool,
    pub loop_filter_ref_deltas: [i32; 8],
    pub loop_filter_mode_deltas: [i32; 2],
    // CDEF.
    pub cdef_damping_minus_3: u32,
    pub cdef_bits: u32,
    pub cdef_y_pri: [u32; 8],
    pub cdef_y_sec: [u32; 8],
    pub cdef_uv_pri: [u32; 8],
    pub cdef_uv_sec: [u32; 8],
    // Loop restoration.
    pub lr_type: [u8; 3],
    pub lr_unit_shift: u32,
    pub lr_uv_shift: bool,
    // Modes.
    pub tx_mode: u32,
    pub reduced_tx_set: bool,
    pub allow_warped_motion: bool,
    // Film grain.
    pub film_grain: FilmGrain,
    /// Bytes consumed by the frame header (after byte alignment) — where an
    /// `OBU_FRAME`'s embedded tile group begins.
    pub header_bytes: usize,
}

/// Parsed `film_grain_params()` (§5.9.30).
#[derive(Debug, Clone, Default)]
pub struct FilmGrain {
    pub apply_grain: bool,
    pub grain_seed: u32,
    pub num_y_points: u32,
    pub point_y_value: [u32; 14],
    pub point_y_scaling: [u32; 14],
    pub chroma_scaling_from_luma: bool,
    pub num_cb_points: u32,
    pub point_cb_value: [u32; 10],
    pub point_cb_scaling: [u32; 10],
    pub num_cr_points: u32,
    pub point_cr_value: [u32; 10],
    pub point_cr_scaling: [u32; 10],
    pub grain_scaling_minus_8: u32,
    pub ar_coeff_lag: u32,
    pub ar_coeffs_y: [i32; 24],
    pub ar_coeffs_cb: [i32; 25],
    pub ar_coeffs_cr: [i32; 25],
    pub ar_coeff_shift_minus_6: u32,
    pub grain_scale_shift: u32,
    pub cb_mult: u32,
    pub cb_luma_mult: u32,
    pub cb_offset: u32,
    pub cr_mult: u32,
    pub cr_luma_mult: u32,
    pub cr_offset: u32,
    pub overlap_flag: bool,
    pub clip_to_restricted_range: bool,
}

impl FrameHeader {
    pub fn num_tiles(&self) -> u32 {
        self.tile_cols * self.tile_rows
    }
}

/// `read_delta_q()` (§5.9.13).
fn read_delta_q(r: &mut BitReader<'_>) -> PResult<i32> {
    if r.flag()? { r.su(7) } else { Ok(0) }
}

/// `tile_log2(blkSize, target)` (§5.9.15).
fn tile_log2(blk_size: u32, target: u32) -> u32 {
    let mut k = 0;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}

/// Parse the uncompressed header of an intra frame (§5.9.2) plus all
/// trailing per-frame parameter blocks, up to and including
/// `byte_alignment()`.
pub fn parse_frame_header(payload: &[u8], seq: &SequenceHeader) -> PResult<FrameHeader> {
    let r = &mut BitReader::new(payload);
    let mut fh = FrameHeader::default();

    let id_len = if seq.frame_id_numbers_present {
        seq.additional_frame_id_length + seq.delta_frame_id_length
    } else {
        0
    };

    let mut frame_size_override = false;
    if seq.reduced_still_picture_header {
        fh.frame_type = FRAME_KEY;
        fh.show_frame = true;
        fh.showable_frame = false;
    } else {
        let show_existing_frame = r.flag()?;
        if show_existing_frame {
            return Err(ParseError(
                "AV1 show_existing_frame is outside the still-picture scope",
            ));
        }
        fh.frame_type = r.bits(2)?;
        if fh.frame_type != FRAME_KEY && fh.frame_type != FRAME_INTRA_ONLY {
            return Err(ParseError(
                "AV1 inter frame is outside the still-picture scope",
            ));
        }
        fh.show_frame = r.flag()?;
        if fh.show_frame && seq.decoder_model_info_present && !seq.equal_picture_interval {
            // temporal_point_info(): frame_presentation_time.
            r.skip(32)?; // frame_presentation_time_length is ≤ 32; see below
            // NOTE: the length is frame_presentation_time_length_minus_1+1,
            // which we did not retain; still-picture streams do not carry
            // decoder model timing, so reject rather than misparse.
            return Err(ParseError(
                "AV1 decoder-model temporal_point_info is outside the still-picture scope",
            ));
        }
        fh.showable_frame = if fh.show_frame {
            fh.frame_type != FRAME_KEY
        } else {
            r.flag()?
        };
        fh.error_resilient_mode = if fh.frame_type == FRAME_KEY && fh.show_frame {
            true
        } else {
            r.flag()?
        };
    }

    fh.disable_cdf_update = r.flag()?;
    fh.allow_screen_content_tools = if seq.seq_force_screen_content_tools == 2 {
        r.flag()?
    } else {
        seq.seq_force_screen_content_tools == 1
    };
    // FrameIsIntra ⇒ force_integer_mv = 1 (derived; the syntax bit is only
    // read for inter frames).
    fh.force_integer_mv = true;
    if seq.frame_id_numbers_present {
        r.skip(id_len as usize)?; // current_frame_id
    }
    if !seq.reduced_still_picture_header {
        frame_size_override = r.flag()?;
    }
    fh.order_hint = r.bits(seq.order_hint_bits)?;
    // FrameIsIntra || error_resilient_mode: primary_ref_frame = NONE, no bit.
    let _primary_ref_frame = PRIMARY_REF_NONE;

    if seq.decoder_model_info_present && r.flag()? {
        // buffer_removal_time_present_flag
        let n = (seq.buffer_removal_time_length_minus_1 + 1) as usize;
        for (op_idc, model_present) in seq
            .operating_point_idc
            .iter()
            .zip(&seq.decoder_model_present_for_op)
        {
            if *model_present {
                // inTemporalLayer/inSpatialLayer check collapses to "always"
                // for op_idc 0; a still image has one temporal unit anyway.
                let _ = op_idc;
                r.skip(n)?; // buffer_removal_time[opNum]
            }
        }
    }

    let refresh_frame_flags = if fh.frame_type == FRAME_KEY && fh.show_frame {
        0xffu32
    } else {
        r.bits(8)?
    };
    if (fh.frame_type == FRAME_INTRA_ONLY || refresh_frame_flags != 0xff)
        && fh.error_resilient_mode
        && seq.enable_order_hint
    {
        for _ in 0..8 {
            r.skip(seq.order_hint_bits as usize)?; // ref_order_hint[i]
        }
    }

    // frame_size() + superres_params() (§5.9.5/5.9.8) — intra path.
    if frame_size_override {
        fh.frame_width = r.bits(seq.frame_width_bits)? + 1;
        fh.frame_height = r.bits(seq.frame_height_bits)? + 1;
    } else {
        fh.frame_width = seq.max_frame_width;
        fh.frame_height = seq.max_frame_height;
    }
    fh.use_superres = if seq.enable_superres {
        r.flag()?
    } else {
        false
    };
    fh.superres_denom = if fh.use_superres { r.bits(3)? + 9 } else { 8 };
    fh.upscaled_width = fh.frame_width;
    fh.frame_width = (fh.upscaled_width * 8 + fh.superres_denom / 2) / fh.superres_denom;
    // render_size(): not needed for decode.
    if r.flag()? {
        r.skip(32)?; // render_width_minus_1 + render_height_minus_1
    }
    if fh.allow_screen_content_tools && fh.upscaled_width == fh.frame_width {
        fh.allow_intrabc = r.flag()?;
    }

    fh.disable_frame_end_update_cdf =
        seq.reduced_still_picture_header || fh.disable_cdf_update || r.flag()?;

    parse_tile_info(r, seq, &mut fh)?;
    parse_quantization_params(r, seq, &mut fh)?;
    parse_segmentation_params(r, &mut fh)?;

    // delta_q_params() (§5.9.17).
    if fh.base_q_idx > 0 {
        fh.delta_q_present = r.flag()?;
    }
    if fh.delta_q_present {
        fh.delta_q_res = r.bits(2)?;
    }
    // delta_lf_params() (§5.9.18).
    if fh.delta_q_present && !fh.allow_intrabc {
        fh.delta_lf_present = r.flag()?;
        if fh.delta_lf_present {
            fh.delta_lf_res = r.bits(2)?;
            fh.delta_lf_multi = r.flag()?;
        }
    }

    let coded_lossless = compute_coded_lossless(&fh);
    let all_lossless = coded_lossless && fh.frame_width == fh.upscaled_width;

    parse_loop_filter_params(r, seq, &mut fh, coded_lossless)?;
    parse_cdef_params(r, seq, &mut fh, coded_lossless)?;
    parse_lr_params(r, seq, &mut fh, all_lossless)?;

    // read_tx_mode() (§5.9.21): ONLY_4X4(0) / LARGEST(1) / SELECT(2).
    fh.tx_mode = if coded_lossless {
        0
    } else if r.flag()? {
        2
    } else {
        1
    };
    // frame_reference_mode(): intra ⇒ reference_select = 0, no bit.
    // skip_mode_params(): intra ⇒ skipModeAllowed = 0, no bit.
    // allow_warped_motion: intra ⇒ 0, no bit.
    fh.reduced_tx_set = r.flag()?;
    // global_motion_params(): intra ⇒ defaults, no bits.
    parse_film_grain_params(r, seq, &mut fh)?;

    r.align_to_byte()?;
    fh.header_bytes = r.bit_pos() / 8;
    Ok(fh)
}

/// `tile_info()` (§5.9.15).
fn parse_tile_info(
    r: &mut BitReader<'_>,
    seq: &SequenceHeader,
    fh: &mut FrameHeader,
) -> PResult<()> {
    const MAX_TILE_WIDTH: u32 = 4096;
    const MAX_TILE_AREA: u32 = 4096 * 2304;
    const MAX_TILE_COLS: u32 = 64;
    const MAX_TILE_ROWS: u32 = 64;

    let mi_cols = 2 * fh.frame_width.div_ceil(8);
    let mi_rows = 2 * fh.frame_height.div_ceil(8);
    let (sb_cols, sb_rows, sb_size) = if seq.use_128x128_superblock {
        ((mi_cols + 31) >> 5, (mi_rows + 31) >> 5, 7u32)
    } else {
        ((mi_cols + 15) >> 4, (mi_rows + 15) >> 4, 6u32)
    };
    let sb_shift = sb_size - 2;
    let max_tile_width_sb = MAX_TILE_WIDTH >> sb_shift;
    let mut max_tile_area_sb = MAX_TILE_AREA >> (2 * sb_shift);
    let min_log2_tile_cols = tile_log2(max_tile_width_sb, sb_cols);
    let max_log2_tile_cols = tile_log2(1, sb_cols.min(MAX_TILE_COLS));
    let max_log2_tile_rows = tile_log2(1, sb_rows.min(MAX_TILE_ROWS));
    let min_log2_tiles = min_log2_tile_cols.max(tile_log2(max_tile_area_sb, sb_rows * sb_cols));

    fh.uniform_tile_spacing = r.flag()?;
    if fh.uniform_tile_spacing {
        fh.tile_cols_log2 = min_log2_tile_cols;
        while fh.tile_cols_log2 < max_log2_tile_cols {
            if r.flag()? {
                fh.tile_cols_log2 += 1;
            } else {
                break;
            }
        }
        let tile_width_sb = (sb_cols + (1 << fh.tile_cols_log2) - 1) >> fh.tile_cols_log2;
        fh.tile_cols = sb_cols.div_ceil(tile_width_sb);
        for _ in 0..fh.tile_cols {
            fh.width_in_sbs_minus_1.push(tile_width_sb - 1);
        }

        let min_log2_tile_rows = min_log2_tiles.saturating_sub(fh.tile_cols_log2);
        fh.tile_rows_log2 = min_log2_tile_rows;
        while fh.tile_rows_log2 < max_log2_tile_rows {
            if r.flag()? {
                fh.tile_rows_log2 += 1;
            } else {
                break;
            }
        }
        let tile_height_sb = (sb_rows + (1 << fh.tile_rows_log2) - 1) >> fh.tile_rows_log2;
        fh.tile_rows = sb_rows.div_ceil(tile_height_sb);
        for _ in 0..fh.tile_rows {
            fh.height_in_sbs_minus_1.push(tile_height_sb - 1);
        }
    } else {
        let mut widest_tile_sb = 0u32;
        let mut start_sb = 0u32;
        while start_sb < sb_cols {
            let max_width = (sb_cols - start_sb).min(max_tile_width_sb);
            let width_minus_1 = r.ns(max_width)?;
            let size_sb = width_minus_1 + 1;
            fh.width_in_sbs_minus_1.push(width_minus_1);
            widest_tile_sb = widest_tile_sb.max(size_sb);
            start_sb += size_sb;
        }
        fh.tile_cols = fh.width_in_sbs_minus_1.len() as u32;
        if fh.tile_cols > MAX_TILE_COLS {
            return Err(ParseError("AV1 too many tile columns"));
        }
        fh.tile_cols_log2 = tile_log2(1, fh.tile_cols);

        if min_log2_tiles > 0 {
            max_tile_area_sb = (sb_rows * sb_cols) >> (min_log2_tiles + 1);
        } else {
            max_tile_area_sb = sb_rows * sb_cols;
        }
        let max_tile_height_sb = (max_tile_area_sb / widest_tile_sb).max(1);
        let mut start_sb = 0u32;
        while start_sb < sb_rows {
            let max_height = (sb_rows - start_sb).min(max_tile_height_sb);
            let height_minus_1 = r.ns(max_height)?;
            fh.height_in_sbs_minus_1.push(height_minus_1);
            start_sb += height_minus_1 + 1;
        }
        fh.tile_rows = fh.height_in_sbs_minus_1.len() as u32;
        if fh.tile_rows > MAX_TILE_ROWS {
            return Err(ParseError("AV1 too many tile rows"));
        }
        fh.tile_rows_log2 = tile_log2(1, fh.tile_rows);
    }

    if fh.tile_cols_log2 > 0 || fh.tile_rows_log2 > 0 {
        fh.context_update_tile_id = r.bits(fh.tile_rows_log2 + fh.tile_cols_log2)?;
        fh.tile_size_bytes = r.bits(2)? + 1;
    } else {
        fh.tile_size_bytes = 1;
    }
    Ok(())
}

/// `quantization_params()` (§5.9.12).
fn parse_quantization_params(
    r: &mut BitReader<'_>,
    seq: &SequenceHeader,
    fh: &mut FrameHeader,
) -> PResult<()> {
    fh.base_q_idx = r.bits(8)?;
    fh.delta_q_y_dc = read_delta_q(r)?;
    if !seq.mono_chrome {
        let diff_uv_delta = if seq.separate_uv_delta_q {
            r.flag()?
        } else {
            false
        };
        fh.delta_q_u_dc = read_delta_q(r)?;
        fh.delta_q_u_ac = read_delta_q(r)?;
        if diff_uv_delta {
            fh.delta_q_v_dc = read_delta_q(r)?;
            fh.delta_q_v_ac = read_delta_q(r)?;
        } else {
            fh.delta_q_v_dc = fh.delta_q_u_dc;
            fh.delta_q_v_ac = fh.delta_q_u_ac;
        }
    }
    fh.using_qmatrix = r.flag()?;
    if fh.using_qmatrix {
        fh.qm_y = r.bits(4)?;
        fh.qm_u = r.bits(4)?;
        fh.qm_v = if seq.separate_uv_delta_q {
            r.bits(4)?
        } else {
            fh.qm_u
        };
    }
    Ok(())
}

/// `segmentation_params()` (§5.9.14) — primary_ref_frame is always NONE for
/// an intra still, so the update flags collapse to 1/0/1.
fn parse_segmentation_params(r: &mut BitReader<'_>, fh: &mut FrameHeader) -> PResult<()> {
    fh.segmentation_enabled = r.flag()?;
    if !fh.segmentation_enabled {
        return Ok(());
    }
    fh.segmentation_update_map = true;
    fh.segmentation_temporal_update = false;
    fh.segmentation_update_data = true;
    for seg in 0..8 {
        for feature in 0..SEG_LVL_MAX {
            let enabled = r.flag()?;
            fh.feature_enabled[seg][feature] = enabled;
            if !enabled {
                continue;
            }
            let bits = SEGMENTATION_FEATURE_BITS[feature];
            let max = SEGMENTATION_FEATURE_MAX[feature];
            let value = if SEGMENTATION_FEATURE_SIGNED[feature] {
                clip3(-max, max, r.su(1 + bits)?)
            } else if bits > 0 {
                clip3(0, max, r.bits(bits)? as i32)
            } else {
                0
            };
            fh.feature_data[seg][feature] = value;
        }
    }
    Ok(())
}

/// `CodedLossless` derivation (§7.12.2 via get_qindex()).
fn compute_coded_lossless(fh: &FrameHeader) -> bool {
    (0..8).all(|seg| {
        let qindex = if fh.segmentation_enabled && fh.feature_enabled[seg][SEG_LVL_ALT_Q] {
            clip3(
                0,
                255,
                fh.base_q_idx as i32 + fh.feature_data[seg][SEG_LVL_ALT_Q],
            )
        } else {
            fh.base_q_idx as i32
        };
        qindex == 0
            && fh.delta_q_y_dc == 0
            && fh.delta_q_u_ac == 0
            && fh.delta_q_u_dc == 0
            && fh.delta_q_v_ac == 0
            && fh.delta_q_v_dc == 0
    })
}

/// `loop_filter_params()` (§5.9.11).
fn parse_loop_filter_params(
    r: &mut BitReader<'_>,
    seq: &SequenceHeader,
    fh: &mut FrameHeader,
    coded_lossless: bool,
) -> PResult<()> {
    // setup_past_independence() defaults (primary_ref_frame is NONE).
    fh.loop_filter_ref_deltas = [1, 0, 0, 0, -1, 0, -1, -1];
    fh.loop_filter_mode_deltas = [0, 0];
    if coded_lossless || fh.allow_intrabc {
        return Ok(());
    }
    fh.loop_filter_level[0] = r.bits(6)?;
    fh.loop_filter_level[1] = r.bits(6)?;
    if !seq.mono_chrome && (fh.loop_filter_level[0] > 0 || fh.loop_filter_level[1] > 0) {
        fh.loop_filter_level[2] = r.bits(6)?;
        fh.loop_filter_level[3] = r.bits(6)?;
    }
    fh.loop_filter_sharpness = r.bits(3)?;
    fh.loop_filter_delta_enabled = r.flag()?;
    if fh.loop_filter_delta_enabled {
        fh.loop_filter_delta_update = r.flag()?;
        if fh.loop_filter_delta_update {
            for i in 0..8 {
                if r.flag()? {
                    fh.loop_filter_ref_deltas[i] = r.su(7)?;
                }
            }
            for i in 0..2 {
                if r.flag()? {
                    fh.loop_filter_mode_deltas[i] = r.su(7)?;
                }
            }
        }
    }
    Ok(())
}

/// `cdef_params()` (§5.9.19).
fn parse_cdef_params(
    r: &mut BitReader<'_>,
    seq: &SequenceHeader,
    fh: &mut FrameHeader,
    coded_lossless: bool,
) -> PResult<()> {
    if coded_lossless || fh.allow_intrabc || !seq.enable_cdef {
        fh.cdef_damping_minus_3 = 0;
        fh.cdef_bits = 0;
        return Ok(());
    }
    fh.cdef_damping_minus_3 = r.bits(2)?;
    fh.cdef_bits = r.bits(2)?;
    for i in 0..(1usize << fh.cdef_bits) {
        fh.cdef_y_pri[i] = r.bits(4)?;
        fh.cdef_y_sec[i] = r.bits(2)?;
        fh.cdef_uv_pri[i] = r.bits(4)?;
        fh.cdef_uv_sec[i] = r.bits(2)?;
    }
    Ok(())
}

/// `lr_params()` (§5.9.20).
fn parse_lr_params(
    r: &mut BitReader<'_>,
    seq: &SequenceHeader,
    fh: &mut FrameHeader,
    all_lossless: bool,
) -> PResult<()> {
    if all_lossless || fh.allow_intrabc || !seq.enable_restoration {
        return Ok(());
    }
    let num_planes = if seq.mono_chrome { 1 } else { 3 };
    let mut uses_lr = false;
    let mut uses_chroma_lr = false;
    for plane in 0..num_planes {
        let lr_type = r.bits(2)? as usize;
        fh.lr_type[plane] = REMAP_LR_TYPE[lr_type];
        if fh.lr_type[plane] != 0 {
            uses_lr = true;
            if plane > 0 {
                uses_chroma_lr = true;
            }
        }
    }
    if uses_lr {
        if seq.use_128x128_superblock {
            fh.lr_unit_shift = 1 + r.bit()?;
        } else {
            fh.lr_unit_shift = r.bit()?;
            if fh.lr_unit_shift > 0 {
                fh.lr_unit_shift += r.bit()?;
            }
        }
        if seq.subsampling_x && seq.subsampling_y && uses_chroma_lr {
            fh.lr_uv_shift = r.flag()?;
        }
    }
    Ok(())
}

/// `film_grain_params()` (§5.9.30) for an intra frame (`update_grain` is 1).
fn parse_film_grain_params(
    r: &mut BitReader<'_>,
    seq: &SequenceHeader,
    fh: &mut FrameHeader,
) -> PResult<()> {
    if !seq.film_grain_params_present || (!fh.show_frame && !fh.showable_frame) {
        return Ok(());
    }
    let g = &mut fh.film_grain;
    g.apply_grain = r.flag()?;
    if !g.apply_grain {
        return Ok(());
    }
    g.grain_seed = r.bits(16)?;
    // frame_type is KEY/INTRA_ONLY here, so update_grain == 1: no ref idx.
    g.num_y_points = r.bits(4)?;
    if g.num_y_points > 14 {
        return Err(ParseError("AV1 film grain num_y_points > 14"));
    }
    for i in 0..g.num_y_points as usize {
        g.point_y_value[i] = r.bits(8)?;
        g.point_y_scaling[i] = r.bits(8)?;
    }
    g.chroma_scaling_from_luma = if seq.mono_chrome { false } else { r.flag()? };
    if seq.mono_chrome
        || g.chroma_scaling_from_luma
        || (seq.subsampling_x && seq.subsampling_y && g.num_y_points == 0)
    {
        g.num_cb_points = 0;
        g.num_cr_points = 0;
    } else {
        g.num_cb_points = r.bits(4)?;
        if g.num_cb_points > 10 {
            return Err(ParseError("AV1 film grain num_cb_points > 10"));
        }
        for i in 0..g.num_cb_points as usize {
            g.point_cb_value[i] = r.bits(8)?;
            g.point_cb_scaling[i] = r.bits(8)?;
        }
        g.num_cr_points = r.bits(4)?;
        if g.num_cr_points > 10 {
            return Err(ParseError("AV1 film grain num_cr_points > 10"));
        }
        for i in 0..g.num_cr_points as usize {
            g.point_cr_value[i] = r.bits(8)?;
            g.point_cr_scaling[i] = r.bits(8)?;
        }
    }
    g.grain_scaling_minus_8 = r.bits(2)?;
    g.ar_coeff_lag = r.bits(2)?;
    let num_pos_luma = (2 * g.ar_coeff_lag * (g.ar_coeff_lag + 1)) as usize;
    let num_pos_chroma = if g.num_y_points > 0 {
        for i in 0..num_pos_luma {
            g.ar_coeffs_y[i] = r.bits(8)? as i32 - 128;
        }
        num_pos_luma + 1
    } else {
        num_pos_luma
    };
    if g.chroma_scaling_from_luma || g.num_cb_points > 0 {
        for i in 0..num_pos_chroma {
            g.ar_coeffs_cb[i] = r.bits(8)? as i32 - 128;
        }
    }
    if g.chroma_scaling_from_luma || g.num_cr_points > 0 {
        for i in 0..num_pos_chroma {
            g.ar_coeffs_cr[i] = r.bits(8)? as i32 - 128;
        }
    }
    g.ar_coeff_shift_minus_6 = r.bits(2)?;
    g.grain_scale_shift = r.bits(2)?;
    if g.num_cb_points > 0 {
        g.cb_mult = r.bits(8)?;
        g.cb_luma_mult = r.bits(8)?;
        g.cb_offset = r.bits(9)?;
    }
    if g.num_cr_points > 0 {
        g.cr_mult = r.bits(8)?;
        g.cr_luma_mult = r.bits(8)?;
        g.cr_offset = r.bits(9)?;
    }
    g.overlap_flag = r.flag()?;
    g.clip_to_restricted_range = r.flag()?;
    Ok(())
}

// ── Tile groups (§5.11.1) ───────────────────────────────────────────────────

/// One coded tile: its position and its bytes.
#[derive(Debug, Clone)]
pub struct Tile {
    pub row: u16,
    pub column: u16,
    pub data: Vec<u8>,
}

/// Parse one tile-group payload (an `OBU_TILE_GROUP` payload, or the bytes
/// of an `OBU_FRAME` after the frame header), appending tiles to `tiles`.
pub fn parse_tile_group(data: &[u8], fh: &FrameHeader, tiles: &mut Vec<Tile>) -> PResult<()> {
    let num_tiles = fh.num_tiles();
    let r = &mut BitReader::new(data);

    let tile_start_and_end_present = if num_tiles > 1 { r.flag()? } else { false };
    let (tg_start, tg_end) = if !tile_start_and_end_present {
        (0, num_tiles - 1)
    } else {
        let tile_bits = fh.tile_cols_log2 + fh.tile_rows_log2;
        (r.bits(tile_bits)?, r.bits(tile_bits)?)
    };
    if tg_end < tg_start || tg_end >= num_tiles {
        return Err(ParseError("AV1 tile group range out of bounds"));
    }
    r.align_to_byte()?;
    let mut pos = r.bit_pos() / 8;

    for tile_num in tg_start..=tg_end {
        let last = tile_num == tg_end;
        let size = if last {
            data.len()
                .checked_sub(pos)
                .ok_or(ParseError("AV1 tile group ran out of bytes"))?
        } else {
            let bytes = data
                .get(pos..pos + fh.tile_size_bytes as usize)
                .ok_or(ParseError("AV1 tile size field truncated"))?;
            pos += fh.tile_size_bytes as usize;
            bytes
                .iter()
                .enumerate()
                .fold(0usize, |a, (i, &b)| a | (usize::from(b) << (8 * i)))
                + 1
        };
        if size == 0 {
            return Err(ParseError("AV1 zero-size tile"));
        }
        let bytes = data
            .get(pos..pos + size)
            .ok_or(ParseError("AV1 tile data truncated"))?;
        pos += size;
        tiles.push(Tile {
            row: (tile_num / fh.tile_cols) as u16,
            column: (tile_num % fh.tile_cols) as u16,
            data: bytes.to_vec(),
        });
    }
    Ok(())
}

// ── Whole still-picture assembly ────────────────────────────────────────────

/// The fully parsed still picture: sequence header, frame header, tiles.
#[derive(Debug)]
pub struct StillPicture {
    pub seq: SequenceHeader,
    pub fh: FrameHeader,
    pub tiles: Vec<Tile>,
}

/// Parse an AVIF-style still picture: `config_obus` (from `av1C`) followed
/// by the item payload's OBU stream. The sequence header may live in either.
pub fn parse_still_picture(config_obus: &[u8], payload: &[u8]) -> PResult<StillPicture> {
    let mut seq: Option<SequenceHeader> = None;
    let mut fh: Option<FrameHeader> = None;
    let mut tiles: Vec<Tile> = Vec::new();

    let mut all_obus = split_obus(config_obus)?;
    all_obus.extend(split_obus(payload)?);

    for obu in all_obus {
        match obu.obu_type {
            OBU_SEQUENCE_HEADER => {
                if fh.is_none() {
                    seq = Some(parse_sequence_header(obu.payload)?);
                }
            }
            OBU_FRAME_HEADER => {
                if fh.is_none() {
                    let s = seq
                        .as_ref()
                        .ok_or(ParseError("AV1 frame header before sequence header"))?;
                    fh = Some(parse_frame_header(obu.payload, s)?);
                }
            }
            OBU_FRAME => {
                if fh.is_some() {
                    break; // second frame: a still image has exactly one
                }
                let s = seq
                    .as_ref()
                    .ok_or(ParseError("AV1 frame before sequence header"))?;
                let parsed = parse_frame_header(obu.payload, s)?;
                let tile_bytes = &obu.payload[parsed.header_bytes..];
                parse_tile_group(tile_bytes, &parsed, &mut tiles)?;
                fh = Some(parsed);
            }
            OBU_TILE_GROUP => {
                let f = fh
                    .as_ref()
                    .ok_or(ParseError("AV1 tile group before frame header"))?;
                if tiles.len() < f.num_tiles() as usize {
                    parse_tile_group(obu.payload, f, &mut tiles)?;
                }
            }
            OBU_TILE_LIST => {
                return Err(ParseError(
                    "AV1 large-scale tile lists are outside the still-picture scope",
                ));
            }
            OBU_TEMPORAL_DELIMITER | OBU_METADATA | OBU_REDUNDANT_FRAME_HEADER => {}
            _ => {}
        }
    }

    let seq = seq.ok_or(ParseError("AV1 stream carries no sequence header"))?;
    let fh = fh.ok_or(ParseError("AV1 stream carries no frame"))?;
    if tiles.len() != fh.num_tiles() as usize {
        return Err(ParseError("AV1 stream is missing coded tiles"));
    }
    Ok(StillPicture { seq, fh, tiles })
}

// ── VAAPI buffer construction ───────────────────────────────────────────────

/// Fill `VADecPictureParameterBufferAV1` for `pic` decoded into `surface`
/// (and, when film grain is applied, displayed into `display_surface`).
#[cfg(hwdec_backend = "vaapi")]
pub fn build_pic_param(
    pic: &StillPicture,
    surface: sys::VASurfaceID,
    display_surface: sys::VASurfaceID,
) -> PResult<sys::VADecPictureParameterBufferAV1> {
    let seq = &pic.seq;
    let fh = &pic.fh;

    let mut seq_info = 0u32;
    let mut set = |bit: u32, on: bool| {
        if on {
            seq_info |= 1 << bit;
        }
    };
    set(0, seq.still_picture);
    set(1, seq.use_128x128_superblock);
    set(2, seq.enable_filter_intra);
    set(3, seq.enable_intra_edge_filter);
    set(4, seq.enable_interintra_compound);
    set(5, seq.enable_masked_compound);
    set(6, seq.enable_dual_filter);
    set(7, seq.enable_order_hint);
    set(8, seq.enable_jnt_comp);
    set(9, seq.enable_cdef);
    set(10, seq.mono_chrome);
    set(11, seq.color_range_full);
    set(12, seq.subsampling_x);
    set(13, seq.subsampling_y);
    set(15, seq.film_grain_params_present);

    let mut pic_info = fh.frame_type & 0x3;
    let mut set = |bit: u32, on: bool| {
        if on {
            pic_info |= 1 << bit;
        }
    };
    set(2, fh.show_frame);
    set(3, fh.showable_frame);
    set(4, fh.error_resilient_mode);
    set(5, fh.disable_cdf_update);
    set(6, fh.allow_screen_content_tools);
    set(7, fh.force_integer_mv);
    set(8, fh.allow_intrabc);
    set(9, fh.use_superres);
    set(10, false); // allow_high_precision_mv: 0 for intra
    set(11, false); // is_motion_mode_switchable
    set(12, false); // use_ref_frame_mvs
    set(13, fh.disable_frame_end_update_cdf);
    set(14, fh.uniform_tile_spacing);
    set(15, fh.allow_warped_motion);
    set(16, false); // large_scale_tile

    let mut seg_fields = 0u32;
    if fh.segmentation_enabled {
        seg_fields |= 1;
    }
    if fh.segmentation_update_map {
        seg_fields |= 1 << 1;
    }
    if fh.segmentation_temporal_update {
        seg_fields |= 1 << 2;
    }
    if fh.segmentation_update_data {
        seg_fields |= 1 << 3;
    }
    let mut feature_data = [[0i16; 8]; 8];
    let mut feature_mask = [0u8; 8];
    for seg in 0..8 {
        for (feature, &enabled) in fh.feature_enabled[seg].iter().enumerate() {
            if enabled {
                feature_mask[seg] |= 1 << feature;
                feature_data[seg][feature] = fh.feature_data[seg][feature] as i16;
            }
        }
    }

    let g = &fh.film_grain;
    let mut grain_fields = 0u32;
    if g.apply_grain {
        grain_fields |= 1;
        if g.chroma_scaling_from_luma {
            grain_fields |= 1 << 1;
        }
        grain_fields |= (g.grain_scaling_minus_8 & 0x3) << 2;
        grain_fields |= (g.ar_coeff_lag & 0x3) << 4;
        grain_fields |= (g.ar_coeff_shift_minus_6 & 0x3) << 6;
        grain_fields |= (g.grain_scale_shift & 0x3) << 8;
        if g.overlap_flag {
            grain_fields |= 1 << 10;
        }
        if g.clip_to_restricted_range {
            grain_fields |= 1 << 11;
        }
    }
    let mut film_grain_info = sys::VAFilmGrainStructAV1 {
        film_grain_info_fields: grain_fields,
        grain_seed: g.grain_seed as u16,
        num_y_points: g.num_y_points as u8,
        point_y_value: [0; 14],
        point_y_scaling: [0; 14],
        num_cb_points: g.num_cb_points as u8,
        point_cb_value: [0; 10],
        point_cb_scaling: [0; 10],
        num_cr_points: g.num_cr_points as u8,
        point_cr_value: [0; 10],
        point_cr_scaling: [0; 10],
        ar_coeffs_y: [0; 24],
        ar_coeffs_cb: [0; 25],
        ar_coeffs_cr: [0; 25],
        cb_mult: g.cb_mult as u8,
        cb_luma_mult: g.cb_luma_mult as u8,
        cb_offset: g.cb_offset as u16,
        cr_mult: g.cr_mult as u8,
        cr_luma_mult: g.cr_luma_mult as u8,
        cr_offset: g.cr_offset as u16,
        va_reserved: [0; sys::VA_PADDING_LOW],
    };
    for i in 0..14 {
        film_grain_info.point_y_value[i] = g.point_y_value[i] as u8;
        film_grain_info.point_y_scaling[i] = g.point_y_scaling[i] as u8;
    }
    for i in 0..10 {
        film_grain_info.point_cb_value[i] = g.point_cb_value[i] as u8;
        film_grain_info.point_cb_scaling[i] = g.point_cb_scaling[i] as u8;
        film_grain_info.point_cr_value[i] = g.point_cr_value[i] as u8;
        film_grain_info.point_cr_scaling[i] = g.point_cr_scaling[i] as u8;
    }
    for i in 0..24 {
        film_grain_info.ar_coeffs_y[i] = g.ar_coeffs_y[i] as i8;
    }
    for i in 0..25 {
        film_grain_info.ar_coeffs_cb[i] = g.ar_coeffs_cb[i] as i8;
        film_grain_info.ar_coeffs_cr[i] = g.ar_coeffs_cr[i] as i8;
    }

    let mut width_in_sbs_minus_1 = [0u16; 63];
    for (i, w) in fh.width_in_sbs_minus_1.iter().take(63).enumerate() {
        width_in_sbs_minus_1[i] = *w as u16;
    }
    let mut height_in_sbs_minus_1 = [0u16; 63];
    for (i, h) in fh.height_in_sbs_minus_1.iter().take(63).enumerate() {
        height_in_sbs_minus_1[i] = *h as u16;
    }

    let mut cdef_y_strengths = [0u8; 8];
    let mut cdef_uv_strengths = [0u8; 8];
    for i in 0..8 {
        cdef_y_strengths[i] = ((fh.cdef_y_pri[i] << 2) | (fh.cdef_y_sec[i] & 0x3)) as u8;
        cdef_uv_strengths[i] = ((fh.cdef_uv_pri[i] << 2) | (fh.cdef_uv_sec[i] & 0x3)) as u8;
    }

    let mut loop_filter_info = 0u8;
    loop_filter_info |= (fh.loop_filter_sharpness & 0x7) as u8;
    if fh.loop_filter_delta_enabled {
        loop_filter_info |= 1 << 3;
    }
    if fh.loop_filter_delta_update {
        loop_filter_info |= 1 << 4;
    }
    let mut ref_deltas = [0i8; 8];
    for (out, &delta) in ref_deltas.iter_mut().zip(&fh.loop_filter_ref_deltas) {
        *out = delta as i8;
    }
    let mode_deltas = [
        fh.loop_filter_mode_deltas[0] as i8,
        fh.loop_filter_mode_deltas[1] as i8,
    ];

    let mut qmatrix = 0u16;
    if fh.using_qmatrix {
        qmatrix |= 1;
        qmatrix |= ((fh.qm_y & 0xf) as u16) << 1;
        qmatrix |= ((fh.qm_u & 0xf) as u16) << 5;
        qmatrix |= ((fh.qm_v & 0xf) as u16) << 9;
    }

    let mut mode_control = 0u32;
    if fh.delta_q_present {
        mode_control |= 1;
    }
    mode_control |= (fh.delta_q_res & 0x3) << 1;
    if fh.delta_lf_present {
        mode_control |= 1 << 3;
    }
    mode_control |= (fh.delta_lf_res & 0x3) << 4;
    if fh.delta_lf_multi {
        mode_control |= 1 << 6;
    }
    mode_control |= (fh.tx_mode & 0x3) << 7;
    // reference_select (bit 9) and skip_mode_present (bit 11): 0 for intra.
    if fh.reduced_tx_set {
        mode_control |= 1 << 10;
    }

    let mut loop_restoration = 0u16;
    loop_restoration |= u16::from(fh.lr_type[0] & 0x3);
    loop_restoration |= u16::from(fh.lr_type[1] & 0x3) << 2;
    loop_restoration |= u16::from(fh.lr_type[2] & 0x3) << 4;
    loop_restoration |= ((fh.lr_unit_shift & 0x3) as u16) << 6;
    if fh.lr_uv_shift {
        loop_restoration |= 1 << 8;
    }

    Ok(sys::VADecPictureParameterBufferAV1 {
        profile: seq.seq_profile,
        order_hint_bits_minus_1: seq.order_hint_bits.saturating_sub(1) as u8,
        bit_depth_idx: match seq.bit_depth {
            8 => 0,
            10 => 1,
            _ => return Err(ParseError("AV1 bit depth outside 8/10")),
        },
        matrix_coefficients: seq.matrix_coefficients,
        seq_info_fields: seq_info,
        current_frame: surface,
        current_display_picture: display_surface,
        anchor_frames_num: 0,
        anchor_frames_list: std::ptr::null_mut(),
        frame_width_minus1: (fh.upscaled_width - 1) as u16,
        frame_height_minus1: (fh.frame_height - 1) as u16,
        output_frame_width_in_tiles_minus_1: 0,
        output_frame_height_in_tiles_minus_1: 0,
        ref_frame_map: [sys::VA_INVALID_SURFACE; 8],
        ref_frame_idx: [0; 7],
        primary_ref_frame: PRIMARY_REF_NONE as u8,
        order_hint: fh.order_hint as u8,
        seg_info: sys::VASegmentationStructAV1 {
            segment_info_fields: seg_fields,
            feature_data,
            feature_mask,
            va_reserved: [0; sys::VA_PADDING_LOW],
        },
        film_grain_info,
        tile_cols: fh.tile_cols as u8,
        tile_rows: fh.tile_rows as u8,
        width_in_sbs_minus_1,
        height_in_sbs_minus_1,
        tile_count_minus_1: 0,
        context_update_tile_id: fh.context_update_tile_id as u16,
        pic_info_fields: pic_info,
        superres_scale_denominator: fh.superres_denom as u8,
        interp_filter: 0,
        filter_level: [fh.loop_filter_level[0] as u8, fh.loop_filter_level[1] as u8],
        filter_level_u: fh.loop_filter_level[2] as u8,
        filter_level_v: fh.loop_filter_level[3] as u8,
        loop_filter_info_fields: loop_filter_info,
        ref_deltas,
        mode_deltas,
        base_qindex: fh.base_q_idx as u8,
        y_dc_delta_q: fh.delta_q_y_dc as i8,
        u_dc_delta_q: fh.delta_q_u_dc as i8,
        u_ac_delta_q: fh.delta_q_u_ac as i8,
        v_dc_delta_q: fh.delta_q_v_dc as i8,
        v_ac_delta_q: fh.delta_q_v_ac as i8,
        qmatrix_fields: qmatrix,
        mode_control_fields: mode_control,
        cdef_damping_minus_3: fh.cdef_damping_minus_3 as u8,
        cdef_bits: fh.cdef_bits as u8,
        cdef_y_strengths,
        cdef_uv_strengths,
        loop_restoration_fields: loop_restoration,
        wm: [sys::VAWarpedMotionParamsAV1::identity(); 7],
        va_reserved: [0; sys::VA_PADDING_MEDIUM],
    })
}

/// Fill `VASliceParameterBufferAV1` for one tile placed at offset 0 of its
/// own data buffer.
#[cfg(hwdec_backend = "vaapi")]
pub fn build_tile_param(tile: &Tile) -> sys::VASliceParameterBufferAV1 {
    sys::VASliceParameterBufferAV1 {
        slice_data_size: tile.data.len() as u32,
        slice_data_offset: 0,
        slice_data_flag: sys::VA_SLICE_DATA_FLAG_ALL,
        tile_row: tile.row,
        tile_column: tile.column,
        tg_start: 0,
        tg_end: 0,
        anchor_frame_idx: 0,
        tile_idx_in_tile_list: 0,
        va_reserved: [0; sys::VA_PADDING_LOW],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real single-frame temporal unit produced by libaom (`ffmpeg -c:v
    /// libaom-av1 -still-picture 1`, 64x64 yuv420p): temporal delimiter +
    /// sequence header + one `OBU_FRAME`.
    const AV1_64X64_LIBAOM: &[u8] = &[
        0x12, 0x00, 0x0a, 0x06, 0x18, 0x15, 0x7f, 0xfd, 0xb0, 0x08, 0x32, 0xc3, 0x02, 0x47, 0x20,
        0x02, 0x4f, 0x31, 0x71, 0x62, 0xeb, 0x81, 0x00, 0xdd, 0xb8, 0xa2, 0x33, 0x13, 0x08, 0xe1,
        0x7f, 0xf0, 0xde, 0xe0, 0xd3, 0xf8, 0x48, 0x1d, 0x62, 0x95, 0xc7, 0xec, 0xec, 0x84, 0xce,
        0x24, 0x32, 0xbc, 0xa5, 0x9b, 0x8b, 0x75, 0x57, 0xf8, 0x01, 0x9e, 0x94, 0x08, 0x3d, 0xc6,
        0x3e, 0xad, 0xbb, 0x4d, 0x22, 0xce, 0xd5, 0x36, 0x64, 0x12, 0xa4, 0x30, 0x42, 0xe2, 0x8f,
        0x10, 0xf5, 0xf3, 0xd7, 0x62, 0x3a, 0x1e, 0x20, 0xaf, 0x96, 0xce, 0x10, 0xa4, 0xa8, 0xeb,
        0xa5, 0x03, 0xf5, 0x5f, 0x5a, 0x14, 0xbd, 0x80, 0xad, 0xad, 0x96, 0xd9, 0xbf, 0x95, 0x2d,
        0x38, 0x82, 0x22, 0x76, 0x3f, 0x99, 0x70, 0x66, 0xa9, 0x45, 0x75, 0xc3, 0xd7, 0x80, 0x36,
        0x64, 0x8c, 0x98, 0x9a, 0x71, 0xb7, 0x54, 0x02, 0xca, 0xe1, 0x6e, 0xb8, 0xcb, 0x25, 0x5d,
        0xa8, 0x3d, 0xc5, 0x28, 0x8a, 0xa7, 0x68, 0xe6, 0x43, 0x78, 0xe5, 0x80, 0xc3, 0xf9, 0x41,
        0x88, 0x99, 0x9c, 0xea, 0xac, 0xa4, 0x8a, 0x4e, 0x50, 0xdb, 0xb7, 0x7f, 0x38, 0xdc, 0x8f,
        0x15, 0xd3, 0xae, 0xfc, 0x44, 0x5e, 0x97, 0x08, 0x29, 0xb2, 0xb7, 0x8b, 0x45, 0xf3, 0xc2,
        0x79, 0x75, 0x0d, 0xd9, 0x0d, 0x57, 0x19, 0xeb, 0x03, 0x9c, 0x2a, 0x52, 0x3b, 0xaf, 0x75,
        0x1f, 0x2d, 0x2d, 0x7c, 0xf6, 0x7f, 0x1c, 0x8f, 0x07, 0x00, 0xee, 0xe0, 0x0f, 0x40, 0xbc,
        0x88, 0x76, 0x79, 0xdc, 0xe0, 0x85, 0xc1, 0xa9, 0x10, 0xd7, 0x74, 0x86, 0x4b, 0x39, 0x46,
        0x5f, 0x72, 0x91, 0xf1, 0x9b, 0xf5, 0xa8, 0x49, 0xd1, 0x78, 0xa6, 0x9d, 0x3f, 0x62, 0x4d,
        0xbc, 0xdc, 0xd6, 0xac, 0x6c, 0xc6, 0x43, 0xec, 0xfb, 0xba, 0x7a, 0xd3, 0x40, 0xd9, 0x53,
        0x3a, 0x30, 0x50, 0x1e, 0x92, 0xa5, 0x07, 0xb0, 0xe0, 0xd3, 0x4d, 0x5a, 0xa6, 0x8b, 0xff,
        0x19, 0x53, 0x0e, 0x34, 0x6c, 0x36, 0xaf, 0x91, 0x85, 0xaf, 0x78, 0x2d, 0x52, 0x9a, 0x06,
        0xf0, 0x8a, 0xe0, 0xee, 0xf3, 0x1c, 0x75, 0x57, 0x67, 0xa0, 0x2f, 0x50, 0xd8, 0x66, 0x34,
        0x2e, 0x90, 0x38, 0x34, 0x79, 0x17, 0xbc, 0x29, 0xe4, 0x18, 0x35, 0xfa, 0x9e, 0xb3, 0x2e,
        0xb0, 0xe2, 0xae, 0x75, 0xa8, 0x24, 0x4f, 0x8d, 0x1f, 0xb4, 0x6d, 0x9e, 0x46, 0xc2, 0x35,
        0xb5, 0x20, 0xab, 0x5c, 0x11, 0xe2,
    ];

    #[test]
    fn obu_stream_splits() {
        let obus = split_obus(AV1_64X64_LIBAOM).expect("valid OBU stream");
        let types: Vec<u8> = obus.iter().map(|o| o.obu_type).collect();
        assert_eq!(
            types,
            vec![OBU_TEMPORAL_DELIMITER, OBU_SEQUENCE_HEADER, OBU_FRAME]
        );
        assert_eq!(obus[1].payload.len(), 6);
        assert_eq!(obus[2].payload.len(), 323);
        assert!(split_obus(&[0x80]).is_err(), "forbidden bit");
        assert!(split_obus(&[0x0a, 0x09, 0x00]).is_err(), "truncated");
    }

    #[test]
    fn av1c_record_parses() {
        // marker/version 0x81, profile 0 level 0, flags, no config OBUs.
        let av1c = parse_av1c(&[0x81, 0x00, 0x0c, 0x00, 0x0a, 0x06]).unwrap();
        assert_eq!(av1c.seq_profile, 0);
        assert_eq!(av1c.config_obus, &[0x0a, 0x06]);
        assert!(parse_av1c(&[0x01, 0, 0, 0]).is_err(), "bad marker");
        assert!(parse_av1c(&[0x81, 0]).is_err(), "short");
    }

    #[test]
    fn libaom_sequence_header_parses() {
        let obus = split_obus(AV1_64X64_LIBAOM).unwrap();
        let seq = parse_sequence_header(obus[1].payload).expect("sequence header");
        assert_eq!(seq.seq_profile, 0);
        assert!(seq.still_picture, "-still-picture 1");
        assert_eq!(seq.max_frame_width, 64);
        assert_eq!(seq.max_frame_height, 64);
        assert_eq!(seq.bit_depth, 8);
        assert!(!seq.mono_chrome);
        assert!(seq.subsampling_x && seq.subsampling_y);
    }

    #[test]
    fn libaom_still_picture_parses_end_to_end() {
        let pic = parse_still_picture(&[], AV1_64X64_LIBAOM).expect("still picture");
        assert_eq!(pic.fh.frame_type, FRAME_KEY);
        assert!(pic.fh.show_frame);
        assert_eq!(pic.fh.upscaled_width, 64);
        assert_eq!(pic.fh.frame_height, 64);
        assert_eq!(pic.fh.num_tiles(), 1);
        assert_eq!(pic.tiles.len(), 1);
        assert_eq!(pic.tiles[0].row, 0);
        assert_eq!(pic.tiles[0].column, 0);
        assert!(!pic.tiles[0].data.is_empty());
        // The single tile is the tail of the frame OBU.
        let obus = split_obus(AV1_64X64_LIBAOM).unwrap();
        assert_eq!(
            pic.tiles[0].data.len() + pic.fh.header_bytes,
            obus[2].payload.len()
        );
    }

    #[test]
    fn pic_param_maps_seq_and_frame_fields() {
        let pic = parse_still_picture(&[], AV1_64X64_LIBAOM).unwrap();
        let p = build_pic_param(&pic, 5, sys::VA_INVALID_SURFACE).unwrap();
        assert_eq!(p.profile, 0);
        assert_eq!(p.bit_depth_idx, 0);
        assert_eq!(p.current_frame, 5);
        assert_eq!(p.frame_width_minus1, 63);
        assert_eq!(p.frame_height_minus1, 63);
        assert_eq!(p.tile_cols, 1);
        assert_eq!(p.tile_rows, 1);
        assert_eq!(p.primary_ref_frame, 7);
        // still_picture bit set; subsampling 4:2:0.
        assert_ne!(p.seq_info_fields & 1, 0);
        assert_ne!(p.seq_info_fields & (1 << 12), 0);
        assert_ne!(p.seq_info_fields & (1 << 13), 0);
        // KEY frame + show_frame + force_integer_mv.
        assert_eq!(p.pic_info_fields & 0x3, FRAME_KEY);
        assert_ne!(p.pic_info_fields & (1 << 2), 0);
        assert_ne!(p.pic_info_fields & (1 << 7), 0);
        // superres denominator 8 = disabled.
        assert_eq!(p.superres_scale_denominator, 8);
        assert!(
            p.ref_frame_map
                .iter()
                .all(|&s| s == sys::VA_INVALID_SURFACE)
        );

        let t = build_tile_param(&pic.tiles[0]);
        assert_eq!(t.slice_data_size, pic.tiles[0].data.len() as u32);
        assert_eq!((t.tile_row, t.tile_column), (0, 0));
    }

    #[test]
    fn inter_frames_and_tile_lists_are_rejected() {
        // Sequence header (reduced still picture) then a TILE_LIST OBU.
        let obus = split_obus(AV1_64X64_LIBAOM).unwrap();
        let mut stream = Vec::new();
        stream.push(0x0a); // OBU_SEQUENCE_HEADER, has_size
        stream.push(obus[1].payload.len() as u8);
        stream.extend_from_slice(obus[1].payload);
        stream.push(0x42); // OBU_TILE_LIST (8 << 3 | has_size)
        stream.push(0x00);
        let err = parse_still_picture(&[], &stream).unwrap_err();
        assert!(err.0.contains("large-scale tile"), "{}", err.0);
    }

    #[test]
    fn tile_log2_matches_spec() {
        assert_eq!(tile_log2(1, 1), 0);
        assert_eq!(tile_log2(1, 2), 1);
        assert_eq!(tile_log2(1, 3), 2);
        assert_eq!(tile_log2(1, 64), 6);
        assert_eq!(tile_log2(16, 100), 3);
    }
}
