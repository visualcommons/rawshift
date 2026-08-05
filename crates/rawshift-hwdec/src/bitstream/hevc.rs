#![cfg_attr(not(hwdec_backend = "vaapi"), allow(dead_code))]
//! HEVC still-picture header parsing shared by hardware backends — **safe
//! Rust only** (no `unsafe`; platform FFI stays in backend modules).
//!
//! ## Scope
//!
//! This parser covers exactly what a HEIF/HEIC still image is (ISO/IEC
//! 23008-12 §7.2: an IRAP intra picture, Main / Main 10 / Main Still
//! Picture profile):
//!
//! - IRAP slices only (`nal_unit_type` 16..=23: BLA/IDR/CRA), I-slice type,
//!   4:2:0 or monochrome chroma, luma bit depth 8 or 10.
//! - Multiple slice segments (including dependent segments), tiles and
//!   wavefronts are supported; scaling lists, range/SCC extensions
//!   (`sps`/`pps_extension_present_flag`) and `separate_colour_plane_flag`
//!   are **not** — such streams fail with a clear `HwDecodeError::Decode`
//!   message rather than decoding incorrectly.
//!
//! Structures parsed: the `hvcC` HEVCDecoderConfigurationRecord (ISO/IEC
//! 14496-15 §8.3.3.1), SPS/PPS NAL units (ITU-T H.265 §7.3.2), and slice
//! segment headers (§7.3.6) precisely far enough to fill VAAPI's
//! `VAPictureParameterBufferHEVC` + `VASliceParameterBufferHEVC` long-format
//! buffers, including `slice_data_byte_offset` (RBSP byte offset of
//! `slice_data()`) and the emulation-prevention-byte count VAAPI wants.

use super::bits::{BitReader, PResult, ParseError, Rbsp, rbsp_from_nal_payload};
#[cfg(hwdec_backend = "vaapi")]
use crate::vaapi::sys;

// ── NAL classification ──────────────────────────────────────────────────────

pub const NAL_BLA_W_LP: u8 = 16;
pub const NAL_IDR_W_RADL: u8 = 19;
pub const NAL_IDR_N_LP: u8 = 20;
pub const NAL_RSV_IRAP_VCL23: u8 = 23;
pub const NAL_SPS: u8 = 33;
pub const NAL_PPS: u8 = 34;

/// Whether `nal_type` is an IRAP VCL type (16..=23) — the only coded-slice
/// types in still-picture scope.
pub fn is_irap(nal_type: u8) -> bool {
    (NAL_BLA_W_LP..=NAL_RSV_IRAP_VCL23).contains(&nal_type)
}

/// Whether `nal_type` is IDR (no POC / short-term RPS in the slice header).
pub fn is_idr(nal_type: u8) -> bool {
    nal_type == NAL_IDR_W_RADL || nal_type == NAL_IDR_N_LP
}

/// The six-bit `nal_unit_type` from the first NAL header byte.
pub fn nal_type(nal: &[u8]) -> PResult<u8> {
    let b0 = *nal.first().ok_or(ParseError("empty NAL unit"))?;
    if b0 & 0x80 != 0 {
        return Err(ParseError("HEVC NAL forbidden_zero_bit set"));
    }
    Ok((b0 >> 1) & 0x3f)
}

// ── hvcC record ─────────────────────────────────────────────────────────────

/// The parts of an `hvcC` HEVCDecoderConfigurationRecord the backend needs:
/// the NAL length-prefix width and the parameter-set NAL units.
#[derive(Debug)]
pub struct Hvcc {
    /// Length-prefix width of the item payload's NAL units (1, 2 or 4).
    pub nal_length_size: usize,
    /// All NAL units from the record's arrays, in record order.
    pub nal_units: Vec<Vec<u8>>,
}

/// Parse the raw `hvcC` record body (ISO/IEC 14496-15 §8.3.3.1).
pub fn parse_hvcc(data: &[u8]) -> PResult<Hvcc> {
    const HEADER_LEN: usize = 23;
    if data.len() < HEADER_LEN {
        return Err(ParseError("hvcC record shorter than its 23-byte header"));
    }
    if data[0] != 1 {
        return Err(ParseError("hvcC configurationVersion is not 1"));
    }
    let nal_length_size = usize::from(data[21] & 0x03) + 1;
    if nal_length_size == 3 {
        return Err(ParseError("hvcC lengthSizeMinusOne of 2 is not permitted"));
    }
    let num_arrays = usize::from(data[22]);

    let mut nal_units = Vec::new();
    let mut pos = HEADER_LEN;
    let take = |pos: &mut usize, n: usize| -> PResult<&[u8]> {
        let end = pos.checked_add(n).ok_or(ParseError("hvcC overflow"))?;
        let slice = data
            .get(*pos..end)
            .ok_or(ParseError("hvcC record truncated"))?;
        *pos = end;
        Ok(slice)
    };
    for _ in 0..num_arrays {
        let _array_header = take(&mut pos, 1)?[0];
        let num_nalus = u16::from_be_bytes(take(&mut pos, 2)?.try_into().unwrap());
        for _ in 0..num_nalus {
            let len = u16::from_be_bytes(take(&mut pos, 2)?.try_into().unwrap());
            nal_units.push(take(&mut pos, usize::from(len))?.to_vec());
        }
    }
    Ok(Hvcc {
        nal_length_size,
        nal_units,
    })
}

/// Split a length-prefixed item payload into NAL units (ISO/IEC 14496-15
/// §8.3.2 — every byte must be consumed exactly).
pub fn split_length_prefixed(payload: &[u8], length_size: usize) -> PResult<Vec<&[u8]>> {
    let mut nals = Vec::new();
    let mut pos = 0usize;
    while pos < payload.len() {
        let prefix = payload
            .get(pos..pos + length_size)
            .ok_or(ParseError("truncated NAL length prefix"))?;
        let len = prefix
            .iter()
            .fold(0usize, |a, &b| (a << 8) | usize::from(b));
        pos += length_size;
        if len == 0 {
            return Err(ParseError("zero-length NAL unit"));
        }
        let nal = payload
            .get(pos..pos + len)
            .ok_or(ParseError("truncated NAL unit body"))?;
        pos += len;
        nals.push(nal);
    }
    Ok(nals)
}

// ── SPS ─────────────────────────────────────────────────────────────────────

/// The SPS fields the VAAPI buffers and the slice-header parser need
/// (H.265 §7.3.2.2.1).
#[derive(Debug, Clone, Default)]
pub struct Sps {
    pub sps_id: u32,
    pub chroma_format_idc: u32,
    pub pic_width_in_luma_samples: u32,
    pub pic_height_in_luma_samples: u32,
    /// Conformance window in luma samples (already scaled by SubWidthC /
    /// SubHeightC).
    pub crop_left: u32,
    pub crop_right: u32,
    pub crop_top: u32,
    pub crop_bottom: u32,
    pub bit_depth_luma: u8,
    pub bit_depth_chroma: u8,
    pub log2_max_pic_order_cnt_lsb: u32,
    pub sps_max_dec_pic_buffering_minus1: u8,
    pub log2_min_luma_coding_block_size_minus3: u8,
    pub log2_diff_max_min_luma_coding_block_size: u8,
    pub log2_min_transform_block_size_minus2: u8,
    pub log2_diff_max_min_transform_block_size: u8,
    pub max_transform_hierarchy_depth_inter: u8,
    pub max_transform_hierarchy_depth_intra: u8,
    pub scaling_list_enabled: bool,
    pub amp_enabled: bool,
    pub sample_adaptive_offset_enabled: bool,
    pub pcm_enabled: bool,
    pub pcm_sample_bit_depth_luma_minus1: u8,
    pub pcm_sample_bit_depth_chroma_minus1: u8,
    pub log2_min_pcm_luma_coding_block_size_minus3: u8,
    pub log2_diff_max_min_pcm_luma_coding_block_size: u8,
    pub pcm_loop_filter_disabled: bool,
    pub num_short_term_ref_pic_sets: u8,
    /// NumDeltaPocs of each SPS short-term RPS (needed to parse a slice
    /// header's explicit `st_ref_pic_set`).
    pub num_delta_pocs: Vec<u32>,
    pub long_term_ref_pics_present: bool,
    pub num_long_term_ref_pics_sps: u32,
    pub sps_temporal_mvp_enabled: bool,
    pub strong_intra_smoothing_enabled: bool,
    /// From VUI `video_full_range_flag`; `false` (limited) when absent.
    pub video_full_range: bool,
}

impl Sps {
    /// `CtbLog2SizeY` (H.265 §7.4.3.2.1).
    pub fn ctb_log2_size_y(&self) -> u32 {
        u32::from(self.log2_min_luma_coding_block_size_minus3)
            + 3
            + u32::from(self.log2_diff_max_min_luma_coding_block_size)
    }

    /// `PicWidthInCtbsY` / `PicHeightInCtbsY` / their product.
    pub fn pic_size_in_ctbs(&self) -> (u32, u32, u32) {
        let ctb = 1u32 << self.ctb_log2_size_y();
        let w = self.pic_width_in_luma_samples.div_ceil(ctb);
        let h = self.pic_height_in_luma_samples.div_ceil(ctb);
        (w, h, w * h)
    }

    /// Output (cropped) luma dimensions.
    pub fn cropped_size(&self) -> (u32, u32) {
        (
            self.pic_width_in_luma_samples
                .saturating_sub(self.crop_left + self.crop_right),
            self.pic_height_in_luma_samples
                .saturating_sub(self.crop_top + self.crop_bottom),
        )
    }
}

/// Skip `profile_tier_level(1, max_sub_layers_minus1)` (H.265 §7.3.3).
fn skip_profile_tier_level(r: &mut BitReader<'_>, max_sub_layers_minus1: u32) -> PResult<()> {
    // general: profile_space(2) tier(1) profile_idc(5) compat(32)
    // constraint+reserved(48) level_idc(8) = 96 bits.
    r.skip(96)?;
    let mut profile_present = [false; 7];
    let mut level_present = [false; 7];
    for i in 0..max_sub_layers_minus1 as usize {
        profile_present[i] = r.flag()?;
        level_present[i] = r.flag()?;
    }
    if max_sub_layers_minus1 > 0 {
        for _ in max_sub_layers_minus1..8 {
            r.skip(2)?; // reserved_zero_2bits
        }
    }
    for i in 0..max_sub_layers_minus1 as usize {
        if profile_present[i] {
            r.skip(88)?; // sub-layer profile block (96 - level byte)
        }
        if level_present[i] {
            r.skip(8)?;
        }
    }
    Ok(())
}

/// Parse one `st_ref_pic_set(stRpsIdx)` (H.265 §7.3.7), returning the
/// derived `NumDeltaPocs[stRpsIdx]`. `num_delta_pocs` holds the values for
/// all lower-indexed sets; `num_sets` is `num_short_term_ref_pic_sets`.
fn parse_st_ref_pic_set(
    r: &mut BitReader<'_>,
    st_rps_idx: u32,
    num_sets: u32,
    num_delta_pocs: &[u32],
) -> PResult<u32> {
    let inter_pred = if st_rps_idx != 0 { r.flag()? } else { false };
    if inter_pred {
        let delta_idx_minus1 = if st_rps_idx == num_sets { r.ue()? } else { 0 };
        let ref_idx = st_rps_idx
            .checked_sub(delta_idx_minus1 + 1)
            .ok_or(ParseError("st_ref_pic_set delta_idx out of range"))?;
        let _delta_rps_sign = r.bit()?;
        let _abs_delta_rps_minus1 = r.ue()?;
        let ref_num = *num_delta_pocs
            .get(ref_idx as usize)
            .ok_or(ParseError("st_ref_pic_set references an unparsed set"))?;
        let mut derived = 0u32;
        for _ in 0..=ref_num {
            let used_by_curr = r.flag()?;
            let use_delta = if used_by_curr { true } else { r.flag()? };
            if used_by_curr || use_delta {
                derived += 1;
            }
        }
        Ok(derived)
    } else {
        let num_negative = r.ue_max(16, "st_ref_pic_set num_negative_pics too large")?;
        let num_positive = r.ue_max(16, "st_ref_pic_set num_positive_pics too large")?;
        for _ in 0..num_negative + num_positive {
            let _delta_poc_minus1 = r.ue()?;
            let _used_by_curr = r.bit()?;
        }
        Ok(num_negative + num_positive)
    }
}

/// Parse an SPS NAL unit (full NAL including the two-byte header).
pub fn parse_sps(nal: &[u8]) -> PResult<Sps> {
    if nal.len() < 2 {
        return Err(ParseError("SPS NAL too short"));
    }
    let rbsp = rbsp_from_nal_payload(&nal[2..]);
    let r = &mut BitReader::new(&rbsp.data);
    let mut sps = Sps::default();

    let _vps_id = r.bits(4)?;
    let max_sub_layers_minus1 = r.bits(3)?;
    if max_sub_layers_minus1 > 6 {
        return Err(ParseError("SPS sps_max_sub_layers_minus1 out of range"));
    }
    let _temporal_id_nesting = r.bit()?;
    skip_profile_tier_level(r, max_sub_layers_minus1)?;

    sps.sps_id = r.ue_max(15, "SPS id out of range")?;
    sps.chroma_format_idc = r.ue_max(3, "SPS chroma_format_idc out of range")?;
    if sps.chroma_format_idc == 3 {
        let separate = r.flag()?;
        if separate {
            return Err(ParseError(
                "HEVC separate_colour_plane_flag is outside the still-picture scope",
            ));
        }
    }
    sps.pic_width_in_luma_samples = r.ue_max(16888, "SPS picture width out of range")?;
    sps.pic_height_in_luma_samples = r.ue_max(16888, "SPS picture height out of range")?;
    if sps.pic_width_in_luma_samples == 0 || sps.pic_height_in_luma_samples == 0 {
        return Err(ParseError("SPS picture dimensions must be non-zero"));
    }
    if r.flag()? {
        // conformance_window_flag: offsets are in chroma units; scale to
        // luma samples with SubWidthC/SubHeightC (2 for 4:2:0, 1 otherwise).
        let (sub_w, sub_h) = match sps.chroma_format_idc {
            1 => (2, 2),
            2 => (2, 1),
            _ => (1, 1),
        };
        sps.crop_left = r.ue()? * sub_w;
        sps.crop_right = r.ue()? * sub_w;
        sps.crop_top = r.ue()? * sub_h;
        sps.crop_bottom = r.ue()? * sub_h;
    }
    sps.bit_depth_luma = 8 + r.ue_max(8, "SPS bit_depth_luma out of range")? as u8;
    sps.bit_depth_chroma = 8 + r.ue_max(8, "SPS bit_depth_chroma out of range")? as u8;
    sps.log2_max_pic_order_cnt_lsb = 4 + r.ue_max(12, "SPS log2_max_poc_lsb out of range")?;

    let ordering_info_present = r.flag()?;
    let first = if ordering_info_present {
        0
    } else {
        max_sub_layers_minus1
    };
    for i in first..=max_sub_layers_minus1 {
        let max_dec_pic_buffering_minus1 = r.ue_max(15, "SPS dec_pic_buffering out of range")?;
        let _max_num_reorder = r.ue()?;
        let _max_latency = r.ue()?;
        if i == max_sub_layers_minus1 {
            sps.sps_max_dec_pic_buffering_minus1 = max_dec_pic_buffering_minus1 as u8;
        }
    }

    sps.log2_min_luma_coding_block_size_minus3 = r.ue_max(3, "SPS log2_min_cb out of range")? as u8;
    sps.log2_diff_max_min_luma_coding_block_size =
        r.ue_max(3, "SPS log2_diff_cb out of range")? as u8;
    sps.log2_min_transform_block_size_minus2 = r.ue_max(3, "SPS log2_min_tb out of range")? as u8;
    sps.log2_diff_max_min_transform_block_size =
        r.ue_max(3, "SPS log2_diff_tb out of range")? as u8;
    sps.max_transform_hierarchy_depth_inter = r.ue_max(4, "SPS tx depth out of range")? as u8;
    sps.max_transform_hierarchy_depth_intra = r.ue_max(4, "SPS tx depth out of range")? as u8;

    sps.scaling_list_enabled = r.flag()?;
    if sps.scaling_list_enabled {
        // Sending the IQ-matrix buffer (default or explicit lists) is not
        // implemented; still-picture HEIC streams do not use scaling lists.
        return Err(ParseError(
            "HEVC scaling lists are outside the still-picture scope",
        ));
    }
    sps.amp_enabled = r.flag()?;
    sps.sample_adaptive_offset_enabled = r.flag()?;
    sps.pcm_enabled = r.flag()?;
    if sps.pcm_enabled {
        sps.pcm_sample_bit_depth_luma_minus1 = r.bits(4)? as u8;
        sps.pcm_sample_bit_depth_chroma_minus1 = r.bits(4)? as u8;
        sps.log2_min_pcm_luma_coding_block_size_minus3 =
            r.ue_max(2, "SPS pcm cb size out of range")? as u8;
        sps.log2_diff_max_min_pcm_luma_coding_block_size =
            r.ue_max(2, "SPS pcm cb diff out of range")? as u8;
        sps.pcm_loop_filter_disabled = r.flag()?;
    }

    let num_sets = r.ue_max(64, "SPS num_short_term_ref_pic_sets out of range")?;
    sps.num_short_term_ref_pic_sets = num_sets as u8;
    for i in 0..num_sets {
        let n = parse_st_ref_pic_set(r, i, num_sets, &sps.num_delta_pocs)?;
        sps.num_delta_pocs.push(n);
    }

    sps.long_term_ref_pics_present = r.flag()?;
    if sps.long_term_ref_pics_present {
        sps.num_long_term_ref_pics_sps = r.ue_max(32, "SPS num_long_term out of range")?;
        for _ in 0..sps.num_long_term_ref_pics_sps {
            r.skip(sps.log2_max_pic_order_cnt_lsb as usize)?; // lt_ref_pic_poc_lsb_sps
            let _used_by_curr = r.bit()?;
        }
    }
    sps.sps_temporal_mvp_enabled = r.flag()?;
    sps.strong_intra_smoothing_enabled = r.flag()?;

    // VUI: parsed only far enough to reach video_full_range_flag.
    if r.flag()? {
        sps.video_full_range = parse_vui_full_range(r)?;
    }
    // sps_extension_present_flag and beyond are irrelevant to decoding here.

    Ok(sps)
}

/// Parse `vui_parameters()` (H.265 §E.2.1) up to `video_full_range_flag`.
fn parse_vui_full_range(r: &mut BitReader<'_>) -> PResult<bool> {
    if r.flag()? {
        // aspect_ratio_info_present_flag
        let idc = r.bits(8)?;
        if idc == 255 {
            r.skip(32)?; // sar_width + sar_height
        }
    }
    if r.flag()? {
        // overscan_info_present_flag
        let _overscan_appropriate = r.bit()?;
    }
    if r.flag()? {
        // video_signal_type_present_flag
        let _video_format = r.bits(3)?;
        return r.flag(); // video_full_range_flag
    }
    Ok(false)
}

// ── PPS ─────────────────────────────────────────────────────────────────────

/// The PPS fields the VAAPI buffers and the slice-header parser need
/// (H.265 §7.3.2.3.1).
#[derive(Debug, Clone, Default)]
pub struct Pps {
    pub pps_id: u32,
    pub sps_id: u32,
    pub dependent_slice_segments_enabled: bool,
    pub output_flag_present: bool,
    pub num_extra_slice_header_bits: u8,
    pub sign_data_hiding_enabled: bool,
    pub cabac_init_present: bool,
    pub num_ref_idx_l0_default_active_minus1: u8,
    pub num_ref_idx_l1_default_active_minus1: u8,
    pub init_qp_minus26: i8,
    pub constrained_intra_pred: bool,
    pub transform_skip_enabled: bool,
    pub cu_qp_delta_enabled: bool,
    pub diff_cu_qp_delta_depth: u8,
    pub pps_cb_qp_offset: i8,
    pub pps_cr_qp_offset: i8,
    pub pps_slice_chroma_qp_offsets_present: bool,
    pub weighted_pred: bool,
    pub weighted_bipred: bool,
    pub transquant_bypass_enabled: bool,
    pub tiles_enabled: bool,
    pub entropy_coding_sync_enabled: bool,
    pub num_tile_columns_minus1: u8,
    pub num_tile_rows_minus1: u8,
    pub uniform_spacing: bool,
    /// Explicit tile column widths / row heights in CTBs (non-uniform only,
    /// without the derived last entry).
    pub column_widths_minus1: Vec<u32>,
    pub row_heights_minus1: Vec<u32>,
    pub loop_filter_across_tiles_enabled: bool,
    pub pps_loop_filter_across_slices_enabled: bool,
    pub deblocking_filter_control_present: bool,
    pub deblocking_filter_override_enabled: bool,
    pub pps_deblocking_filter_disabled: bool,
    pub pps_beta_offset_div2: i8,
    pub pps_tc_offset_div2: i8,
    pub lists_modification_present: bool,
    pub log2_parallel_merge_level_minus2: u8,
    pub slice_segment_header_extension_present: bool,
}

/// Parse a PPS NAL unit (full NAL including the two-byte header).
// Fields are assigned in the spec's exact syntax order — clearer for a
// parser than one giant struct literal.
#[allow(clippy::field_reassign_with_default)]
pub fn parse_pps(nal: &[u8]) -> PResult<Pps> {
    if nal.len() < 2 {
        return Err(ParseError("PPS NAL too short"));
    }
    let rbsp = rbsp_from_nal_payload(&nal[2..]);
    let r = &mut BitReader::new(&rbsp.data);
    let mut pps = Pps::default();

    pps.pps_id = r.ue_max(63, "PPS id out of range")?;
    pps.sps_id = r.ue_max(15, "PPS sps id out of range")?;
    pps.dependent_slice_segments_enabled = r.flag()?;
    pps.output_flag_present = r.flag()?;
    pps.num_extra_slice_header_bits = r.bits(3)? as u8;
    pps.sign_data_hiding_enabled = r.flag()?;
    pps.cabac_init_present = r.flag()?;
    pps.num_ref_idx_l0_default_active_minus1 =
        r.ue_max(14, "PPS num_ref_idx_l0 out of range")? as u8;
    pps.num_ref_idx_l1_default_active_minus1 =
        r.ue_max(14, "PPS num_ref_idx_l1 out of range")? as u8;
    pps.init_qp_minus26 = clip_i8(r.se()?, "PPS init_qp_minus26")?;
    pps.constrained_intra_pred = r.flag()?;
    pps.transform_skip_enabled = r.flag()?;
    pps.cu_qp_delta_enabled = r.flag()?;
    if pps.cu_qp_delta_enabled {
        pps.diff_cu_qp_delta_depth = r.ue_max(3, "PPS diff_cu_qp_delta_depth")? as u8;
    }
    pps.pps_cb_qp_offset = clip_i8(r.se()?, "PPS cb_qp_offset")?;
    pps.pps_cr_qp_offset = clip_i8(r.se()?, "PPS cr_qp_offset")?;
    pps.pps_slice_chroma_qp_offsets_present = r.flag()?;
    pps.weighted_pred = r.flag()?;
    pps.weighted_bipred = r.flag()?;
    pps.transquant_bypass_enabled = r.flag()?;
    pps.tiles_enabled = r.flag()?;
    pps.entropy_coding_sync_enabled = r.flag()?;
    if pps.tiles_enabled {
        pps.num_tile_columns_minus1 = r.ue_max(18, "PPS too many tile columns for VAAPI")? as u8;
        pps.num_tile_rows_minus1 = r.ue_max(20, "PPS too many tile rows for VAAPI")? as u8;
        pps.uniform_spacing = r.flag()?;
        if !pps.uniform_spacing {
            for _ in 0..pps.num_tile_columns_minus1 {
                pps.column_widths_minus1.push(r.ue()?);
            }
            for _ in 0..pps.num_tile_rows_minus1 {
                pps.row_heights_minus1.push(r.ue()?);
            }
        }
        pps.loop_filter_across_tiles_enabled = r.flag()?;
    } else {
        pps.uniform_spacing = true;
        pps.loop_filter_across_tiles_enabled = true;
    }
    pps.pps_loop_filter_across_slices_enabled = r.flag()?;
    pps.deblocking_filter_control_present = r.flag()?;
    if pps.deblocking_filter_control_present {
        pps.deblocking_filter_override_enabled = r.flag()?;
        pps.pps_deblocking_filter_disabled = r.flag()?;
        if !pps.pps_deblocking_filter_disabled {
            pps.pps_beta_offset_div2 = clip_i8(r.se()?, "PPS beta_offset")?;
            pps.pps_tc_offset_div2 = clip_i8(r.se()?, "PPS tc_offset")?;
        }
    }
    if r.flag()? {
        // pps_scaling_list_data_present_flag
        return Err(ParseError(
            "HEVC scaling lists are outside the still-picture scope",
        ));
    }
    pps.lists_modification_present = r.flag()?;
    pps.log2_parallel_merge_level_minus2 = r.ue_max(4, "PPS parallel merge level")? as u8;
    pps.slice_segment_header_extension_present = r.flag()?;
    if r.flag()? {
        // pps_extension_present_flag: range/SCC extensions change the slice
        // header syntax this parser relies on.
        return Err(ParseError(
            "HEVC range/SCC PPS extensions are outside the still-picture scope",
        ));
    }
    Ok(pps)
}

fn clip_i8(v: i32, what: &'static str) -> PResult<i8> {
    i8::try_from(v).map_err(|_| ParseError(what))
}

// ── Slice segment header ────────────────────────────────────────────────────

/// The parsed slice-segment-header values VAAPI's long-format slice
/// parameter buffer needs (H.265 §7.3.6.1, I-slices only).
#[derive(Debug, Clone, Default)]
pub struct SliceHeader {
    pub first_slice_in_pic: bool,
    pub dependent_slice_segment: bool,
    pub slice_segment_address: u32,
    pub pps_id: u32,
    /// Always 2 (I) in still-picture scope.
    pub slice_type: u32,
    pub slice_sao_luma: bool,
    pub slice_sao_chroma: bool,
    pub slice_temporal_mvp_enabled: bool,
    pub slice_qp_delta: i8,
    pub slice_cb_qp_offset: i8,
    pub slice_cr_qp_offset: i8,
    pub slice_deblocking_filter_disabled: bool,
    pub slice_beta_offset_div2: i8,
    pub slice_tc_offset_div2: i8,
    pub slice_loop_filter_across_slices_enabled: bool,
    pub num_entry_point_offsets: u32,
    /// Bits of an explicit `st_ref_pic_set` carried by this (CRA) slice
    /// header — VAAPI's `st_rps_bits`.
    pub st_rps_bits: u32,
    /// VAAPI `slice_data_byte_offset`: bytes from the start of the NAL unit
    /// (header included) to `slice_data()`, counted on the de-escaped RBSP.
    pub slice_data_byte_offset: u32,
    /// Emulation-prevention bytes within the slice header span.
    pub num_emu_prevn_bytes: u32,
}

/// Parse the slice segment header of an IRAP I-slice NAL.
// Fields are assigned in the spec's exact syntax order (see parse_pps).
#[allow(clippy::field_reassign_with_default)]
pub fn parse_slice_header(nal: &[u8], sps: &Sps, pps: &Pps) -> PResult<SliceHeader> {
    let nut = nal_type(nal)?;
    if !is_irap(nut) {
        return Err(ParseError(
            "HEVC non-IRAP coded slice is outside the still-picture scope",
        ));
    }
    if nal.len() < 3 {
        return Err(ParseError("slice NAL too short"));
    }
    let rbsp: Rbsp = rbsp_from_nal_payload(&nal[2..]);
    let r = &mut BitReader::new(&rbsp.data);
    let mut sh = SliceHeader::default();

    sh.first_slice_in_pic = r.flag()?;
    // IRAP (16..=23): no_output_of_prior_pics_flag.
    let _no_output_of_prior_pics = r.bit()?;
    sh.pps_id = r.ue_max(63, "slice pps id out of range")?;
    if sh.pps_id != pps.pps_id {
        return Err(ParseError("slice references an unknown PPS"));
    }

    if !sh.first_slice_in_pic {
        if pps.dependent_slice_segments_enabled {
            sh.dependent_slice_segment = r.flag()?;
        }
        let (_, _, pic_size_in_ctbs) = sps.pic_size_in_ctbs();
        let addr_bits = ceil_log2(pic_size_in_ctbs);
        sh.slice_segment_address = r.bits(addr_bits)?;
    }

    if !sh.dependent_slice_segment {
        r.skip(usize::from(pps.num_extra_slice_header_bits))?;
        sh.slice_type = r.ue_max(2, "slice_type out of range")?;
        if sh.slice_type != 2 {
            return Err(ParseError(
                "HEVC non-I slice is outside the still-picture scope",
            ));
        }
        if pps.output_flag_present {
            let _pic_output_flag = r.bit()?;
        }
        // separate_colour_plane_flag was rejected at SPS parse, so no
        // colour_plane_id here.
        if !is_idr(nut) {
            // CRA/BLA: POC lsb + short-term RPS.
            r.skip(sps.log2_max_pic_order_cnt_lsb as usize)?;
            let sps_rps = r.flag()?;
            if !sps_rps {
                let start = r.bit_pos();
                parse_st_ref_pic_set(
                    r,
                    u32::from(sps.num_short_term_ref_pic_sets),
                    u32::from(sps.num_short_term_ref_pic_sets),
                    &sps.num_delta_pocs,
                )?;
                sh.st_rps_bits = (r.bit_pos() - start) as u32;
            } else if sps.num_short_term_ref_pic_sets > 1 {
                r.skip(ceil_log2(u32::from(sps.num_short_term_ref_pic_sets)) as usize)?;
            }
            if sps.long_term_ref_pics_present {
                let num_long_term_sps = if sps.num_long_term_ref_pics_sps > 0 {
                    r.ue_max(sps.num_long_term_ref_pics_sps, "num_long_term_sps")?
                } else {
                    0
                };
                let num_long_term_pics = r.ue_max(32, "num_long_term_pics")?;
                for i in 0..num_long_term_sps + num_long_term_pics {
                    if i < num_long_term_sps {
                        if sps.num_long_term_ref_pics_sps > 1 {
                            r.skip(ceil_log2(sps.num_long_term_ref_pics_sps) as usize)?;
                        }
                    } else {
                        r.skip(sps.log2_max_pic_order_cnt_lsb as usize)?; // poc_lsb_lt
                        let _used_by_curr_pic_lt = r.bit()?;
                    }
                    if r.flag()? {
                        // delta_poc_msb_present_flag
                        let _delta_poc_msb_cycle_lt = r.ue()?;
                    }
                }
            }
            if sps.sps_temporal_mvp_enabled {
                sh.slice_temporal_mvp_enabled = r.flag()?;
            }
        }
        if sps.sample_adaptive_offset_enabled {
            sh.slice_sao_luma = r.flag()?;
            if sps.chroma_format_idc != 0 {
                sh.slice_sao_chroma = r.flag()?;
            }
        }
        // slice_type == I: no reference-list / prediction-weight syntax.
        sh.slice_qp_delta = clip_i8(r.se()?, "slice_qp_delta out of range")?;
        if pps.pps_slice_chroma_qp_offsets_present {
            sh.slice_cb_qp_offset = clip_i8(r.se()?, "slice_cb_qp_offset")?;
            sh.slice_cr_qp_offset = clip_i8(r.se()?, "slice_cr_qp_offset")?;
        }
        let mut deblocking_override = false;
        if pps.deblocking_filter_control_present {
            if pps.deblocking_filter_override_enabled {
                deblocking_override = r.flag()?;
            }
            if deblocking_override {
                sh.slice_deblocking_filter_disabled = r.flag()?;
                if !sh.slice_deblocking_filter_disabled {
                    sh.slice_beta_offset_div2 = clip_i8(r.se()?, "slice_beta_offset")?;
                    sh.slice_tc_offset_div2 = clip_i8(r.se()?, "slice_tc_offset")?;
                }
            } else {
                sh.slice_deblocking_filter_disabled = pps.pps_deblocking_filter_disabled;
            }
        }
        sh.slice_loop_filter_across_slices_enabled = pps.pps_loop_filter_across_slices_enabled;
        if pps.pps_loop_filter_across_slices_enabled
            && (sh.slice_sao_luma || sh.slice_sao_chroma || !sh.slice_deblocking_filter_disabled)
        {
            sh.slice_loop_filter_across_slices_enabled = r.flag()?;
        }
    }

    if pps.tiles_enabled || pps.entropy_coding_sync_enabled {
        sh.num_entry_point_offsets = r.ue_max(440, "too many entry point offsets")?;
        if sh.num_entry_point_offsets > 0 {
            let offset_len = r.ue_max(31, "entry point offset length")? + 1;
            for _ in 0..sh.num_entry_point_offsets {
                r.skip(offset_len as usize)?;
            }
        }
    }
    if pps.slice_segment_header_extension_present {
        let len = r.ue_max(256, "slice header extension length")?;
        r.skip(len as usize * 8)?;
    }
    // byte_alignment(): alignment_bit_equal_to_one + zero bits.
    if r.bit()? != 1 {
        return Err(ParseError("slice header alignment bit is not 1"));
    }
    r.align_to_byte()?;

    let rbsp_bytes = r.bit_pos() / 8;
    sh.slice_data_byte_offset = 2 + rbsp_bytes as u32;
    sh.num_emu_prevn_bytes = rbsp.epb_count_within(rbsp_bytes) as u32;
    Ok(sh)
}

/// `Ceil(Log2(x))` for `x >= 1`.
fn ceil_log2(x: u32) -> u32 {
    if x <= 1 {
        0
    } else {
        32 - (x - 1).leading_zeros()
    }
}

// ── VAAPI buffer construction ───────────────────────────────────────────────

/// Uniform tile partition of `total` CTBs into `count` parts, as the
/// `minus1` sizes VAAPI wants (H.265 §6.5.1 derivation).
#[cfg(hwdec_backend = "vaapi")]
fn uniform_partition_minus1(total: u32, count: u32) -> Vec<u16> {
    (0..count)
        .map(|i| (((i + 1) * total) / count - (i * total) / count - 1) as u16)
        .collect()
}

/// Fill `VAPictureParameterBufferHEVC` for a still picture decoded into
/// `surface`. `nut` is the slice NAL type; `st_rps_bits` comes from the
/// first independent slice header.
#[cfg(hwdec_backend = "vaapi")]
pub fn build_pic_param(
    sps: &Sps,
    pps: &Pps,
    nut: u8,
    st_rps_bits: u32,
    surface: sys::VASurfaceID,
) -> PResult<sys::VAPictureParameterBufferHEVC> {
    let (ctbs_w, ctbs_h, _) = sps.pic_size_in_ctbs();

    let mut pic_fields = 0u32;
    let mut set = |bit: u32, on: bool| {
        if on {
            pic_fields |= 1 << bit;
        }
    };
    set(0, sps.chroma_format_idc & 1 != 0); // chroma_format_idc: 2 bits at 0
    set(1, sps.chroma_format_idc & 2 != 0);
    set(2, false); // separate_colour_plane_flag (rejected earlier)
    set(3, sps.pcm_enabled);
    set(4, sps.scaling_list_enabled);
    set(5, pps.transform_skip_enabled);
    set(6, sps.amp_enabled);
    set(7, sps.strong_intra_smoothing_enabled);
    set(8, pps.sign_data_hiding_enabled);
    set(9, pps.constrained_intra_pred);
    set(10, pps.cu_qp_delta_enabled);
    set(11, pps.weighted_pred);
    set(12, pps.weighted_bipred);
    set(13, pps.transquant_bypass_enabled);
    set(14, pps.tiles_enabled);
    set(15, pps.entropy_coding_sync_enabled);
    set(16, pps.pps_loop_filter_across_slices_enabled);
    set(17, pps.loop_filter_across_tiles_enabled);
    set(18, sps.pcm_loop_filter_disabled);
    set(19, true); // NoPicReorderingFlag: single still picture
    set(20, true); // NoBiPredFlag: intra only

    let mut slice_parsing = 0u32;
    let mut set = |bit: u32, on: bool| {
        if on {
            slice_parsing |= 1 << bit;
        }
    };
    set(0, pps.lists_modification_present);
    set(1, sps.long_term_ref_pics_present);
    set(2, sps.sps_temporal_mvp_enabled);
    set(3, pps.cabac_init_present);
    set(4, pps.output_flag_present);
    set(5, pps.dependent_slice_segments_enabled);
    set(6, pps.pps_slice_chroma_qp_offsets_present);
    set(7, sps.sample_adaptive_offset_enabled);
    set(8, pps.deblocking_filter_override_enabled);
    set(9, pps.pps_deblocking_filter_disabled);
    set(10, pps.slice_segment_header_extension_present);
    set(11, (16..=21).contains(&nut)); // RapPicFlag
    set(12, is_idr(nut)); // IdrPicFlag
    set(13, true); // IntraPicFlag

    let mut column_width_minus1 = [0u16; 19];
    let mut row_height_minus1 = [0u16; 21];
    if pps.tiles_enabled {
        let cols = u32::from(pps.num_tile_columns_minus1) + 1;
        let rows = u32::from(pps.num_tile_rows_minus1) + 1;
        if pps.uniform_spacing {
            for (i, w) in uniform_partition_minus1(ctbs_w, cols).iter().enumerate() {
                column_width_minus1[i] = *w;
            }
            for (i, h) in uniform_partition_minus1(ctbs_h, rows).iter().enumerate() {
                row_height_minus1[i] = *h;
            }
        } else {
            let mut used_w = 0u32;
            for (i, w) in pps.column_widths_minus1.iter().enumerate() {
                column_width_minus1[i] = *w as u16;
                used_w += w + 1;
            }
            column_width_minus1[pps.column_widths_minus1.len()] = ctbs_w
                .checked_sub(used_w + 1)
                .ok_or(ParseError("tile column widths exceed the picture"))?
                as u16;
            let mut used_h = 0u32;
            for (i, h) in pps.row_heights_minus1.iter().enumerate() {
                row_height_minus1[i] = *h as u16;
                used_h += h + 1;
            }
            row_height_minus1[pps.row_heights_minus1.len()] = ctbs_h
                .checked_sub(used_h + 1)
                .ok_or(ParseError("tile row heights exceed the picture"))?
                as u16;
        }
    }

    Ok(sys::VAPictureParameterBufferHEVC {
        CurrPic: sys::VAPictureHEVC {
            picture_id: surface,
            pic_order_cnt: 0,
            flags: 0,
            va_reserved: [0; sys::VA_PADDING_LOW],
        },
        ReferenceFrames: [sys::VAPictureHEVC::invalid(); 15],
        pic_width_in_luma_samples: sps.pic_width_in_luma_samples as u16,
        pic_height_in_luma_samples: sps.pic_height_in_luma_samples as u16,
        pic_fields,
        sps_max_dec_pic_buffering_minus1: sps.sps_max_dec_pic_buffering_minus1,
        bit_depth_luma_minus8: sps.bit_depth_luma - 8,
        bit_depth_chroma_minus8: sps.bit_depth_chroma - 8,
        pcm_sample_bit_depth_luma_minus1: sps.pcm_sample_bit_depth_luma_minus1,
        pcm_sample_bit_depth_chroma_minus1: sps.pcm_sample_bit_depth_chroma_minus1,
        log2_min_luma_coding_block_size_minus3: sps.log2_min_luma_coding_block_size_minus3,
        log2_diff_max_min_luma_coding_block_size: sps.log2_diff_max_min_luma_coding_block_size,
        log2_min_transform_block_size_minus2: sps.log2_min_transform_block_size_minus2,
        log2_diff_max_min_transform_block_size: sps.log2_diff_max_min_transform_block_size,
        log2_min_pcm_luma_coding_block_size_minus3: sps.log2_min_pcm_luma_coding_block_size_minus3,
        log2_diff_max_min_pcm_luma_coding_block_size: sps
            .log2_diff_max_min_pcm_luma_coding_block_size,
        max_transform_hierarchy_depth_intra: sps.max_transform_hierarchy_depth_intra,
        max_transform_hierarchy_depth_inter: sps.max_transform_hierarchy_depth_inter,
        init_qp_minus26: pps.init_qp_minus26,
        diff_cu_qp_delta_depth: pps.diff_cu_qp_delta_depth,
        pps_cb_qp_offset: pps.pps_cb_qp_offset,
        pps_cr_qp_offset: pps.pps_cr_qp_offset,
        log2_parallel_merge_level_minus2: pps.log2_parallel_merge_level_minus2,
        num_tile_columns_minus1: pps.num_tile_columns_minus1,
        num_tile_rows_minus1: pps.num_tile_rows_minus1,
        column_width_minus1,
        row_height_minus1,
        slice_parsing_fields: slice_parsing,
        log2_max_pic_order_cnt_lsb_minus4: (sps.log2_max_pic_order_cnt_lsb - 4) as u8,
        num_short_term_ref_pic_sets: sps.num_short_term_ref_pic_sets,
        num_long_term_ref_pic_sps: sps.num_long_term_ref_pics_sps as u8,
        num_ref_idx_l0_default_active_minus1: pps.num_ref_idx_l0_default_active_minus1,
        num_ref_idx_l1_default_active_minus1: pps.num_ref_idx_l1_default_active_minus1,
        pps_beta_offset_div2: pps.pps_beta_offset_div2,
        pps_tc_offset_div2: pps.pps_tc_offset_div2,
        num_extra_slice_header_bits: pps.num_extra_slice_header_bits,
        st_rps_bits,
        va_reserved: [0; sys::VA_PADDING_MEDIUM],
    })
}

/// Fill `VASliceParameterBufferHEVC` for one coded slice NAL of
/// `slice_data_size` bytes at offset 0 of its own data buffer.
#[cfg(hwdec_backend = "vaapi")]
pub fn build_slice_param(
    sh: &SliceHeader,
    pps: &Pps,
    slice_data_size: u32,
    last_slice_of_pic: bool,
) -> sys::VASliceParameterBufferHEVC {
    let mut flags = 0u32;
    // slice_type (2 bits at 2): I = 2; color_plane_id (2 bits at 4): 0.
    flags |= (sh.slice_type & 0x3) << 2;
    let mut set = |bit: u32, on: bool| {
        if on {
            flags |= 1 << bit;
        }
    };
    set(0, last_slice_of_pic);
    set(1, sh.dependent_slice_segment);
    set(6, sh.slice_sao_luma);
    set(7, sh.slice_sao_chroma);
    set(8, false); // mvd_l1_zero_flag: N/A for I slices
    set(9, false); // cabac_init_flag: not present in I slices
    set(10, sh.slice_temporal_mvp_enabled);
    set(11, sh.slice_deblocking_filter_disabled);
    set(12, false); // collocated_from_l0_flag
    set(13, sh.slice_loop_filter_across_slices_enabled);

    sys::VASliceParameterBufferHEVC {
        slice_data_size,
        slice_data_offset: 0,
        slice_data_flag: sys::VA_SLICE_DATA_FLAG_ALL,
        slice_data_byte_offset: sh.slice_data_byte_offset,
        slice_segment_address: sh.slice_segment_address,
        RefPicList: [[0xff; 15]; 2],
        LongSliceFlags: flags,
        collocated_ref_idx: 0xff,
        num_ref_idx_l0_active_minus1: pps.num_ref_idx_l0_default_active_minus1,
        num_ref_idx_l1_active_minus1: pps.num_ref_idx_l1_default_active_minus1,
        slice_qp_delta: sh.slice_qp_delta,
        slice_cb_qp_offset: sh.slice_cb_qp_offset,
        slice_cr_qp_offset: sh.slice_cr_qp_offset,
        slice_beta_offset_div2: sh.slice_beta_offset_div2,
        slice_tc_offset_div2: sh.slice_tc_offset_div2,
        luma_log2_weight_denom: 0,
        delta_chroma_log2_weight_denom: 0,
        delta_luma_weight_l0: [0; 15],
        luma_offset_l0: [0; 15],
        delta_chroma_weight_l0: [[0; 2]; 15],
        ChromaOffsetL0: [[0; 2]; 15],
        delta_luma_weight_l1: [0; 15],
        luma_offset_l1: [0; 15],
        delta_chroma_weight_l1: [[0; 2]; 15],
        ChromaOffsetL1: [[0; 2]; 15],
        five_minus_max_num_merge_cand: 0,
        num_entry_point_offsets: sh.num_entry_point_offsets as u16,
        entry_offset_to_subset_array: 0,
        slice_data_num_emu_prevn_bytes: sh.num_emu_prevn_bytes as u16,
        va_reserved: [0; sys::VA_PADDING_LOW - 2],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real SPS/PPS pair produced by x265 (via heif-enc) for a 64x64 8-bit
    // 4:2:0 full-range HEIC still picture — byte-identical to the parameter
    // sets in `test_data/standard/heic/test_64x64.heic`'s hvcC record
    // (extracted with `ffmpeg -i test_64x64.heic -c copy -f hevc`).
    const SPS_64X64_X265: &[u8] = &[
        0x42, 0x01, 0x01, 0x03, 0x70, 0x00, 0x00, 0x03, 0x00, 0x90, 0x00, 0x00, 0x03, 0x00, 0x00,
        0x03, 0x00, 0x1e, 0xa0, 0x20, 0x81, 0x05, 0x96, 0xea, 0x49, 0x29, 0xae, 0x6e, 0x02, 0x1a,
        0x0c, 0x08, 0x00, 0x00, 0x03, 0x00, 0xc8, 0x00, 0x00, 0x03, 0x00, 0x08, 0x40,
    ];
    const PPS_64X64_X265: &[u8] = &[0x44, 0x01, 0xc1, 0x72, 0xb0, 0x22, 0x40];

    #[test]
    fn hvcc_parses_arrays_and_length_size() {
        // 23-byte header + one array with one 3-byte NAL.
        let mut hvcc = vec![0u8; 23];
        hvcc[0] = 1;
        hvcc[16] = 0x01; // chroma 4:2:0
        hvcc[21] = 0x03; // lengthSizeMinusOne = 3
        hvcc[22] = 1; // numOfArrays
        hvcc.extend_from_slice(&[0x80 | 33, 0x00, 0x01, 0x00, 0x03, 0x42, 0x01, 0xAA]);
        let parsed = parse_hvcc(&hvcc).expect("valid hvcC");
        assert_eq!(parsed.nal_length_size, 4);
        assert_eq!(parsed.nal_units, vec![vec![0x42, 0x01, 0xAA]]);

        assert!(parse_hvcc(&hvcc[..20]).is_err(), "truncated header");
        let mut bad = hvcc.clone();
        bad[21] = 0x02; // lengthSizeMinusOne = 2 (3-byte prefixes) is illegal
        assert!(parse_hvcc(&bad).is_err());
    }

    #[test]
    fn length_prefixed_payload_splits_exactly() {
        let payload = [0, 0, 0, 2, 0x26, 0x01, 0, 0, 0, 1, 0x40];
        let nals = split_length_prefixed(&payload, 4).unwrap();
        assert_eq!(nals, vec![&[0x26u8, 0x01][..], &[0x40u8][..]]);
        assert!(split_length_prefixed(&payload[..9], 4).is_err());
        assert!(split_length_prefixed(&[0, 0, 0, 0], 4).is_err(), "zero len");
    }

    #[test]
    fn x265_sps_parses() {
        let sps = parse_sps(SPS_64X64_X265).expect("x265 SPS");
        assert_eq!(sps.pic_width_in_luma_samples, 64);
        assert_eq!(sps.pic_height_in_luma_samples, 64);
        assert_eq!(sps.chroma_format_idc, 1);
        assert_eq!(sps.bit_depth_luma, 8);
        assert_eq!(sps.bit_depth_chroma, 8);
        assert!(sps.video_full_range, "heif-enc marks full range");
        assert_eq!(sps.cropped_size(), (64, 64));
        assert_eq!(sps.num_short_term_ref_pic_sets, 0);
        assert!(!sps.scaling_list_enabled);
    }

    #[test]
    fn x265_pps_parses() {
        let pps = parse_pps(PPS_64X64_X265).expect("x265 PPS");
        assert_eq!(pps.pps_id, 0);
        assert_eq!(pps.sps_id, 0);
        assert!(!pps.tiles_enabled);
        assert!(!pps.slice_segment_header_extension_present);
    }

    #[test]
    #[cfg(hwdec_backend = "vaapi")]
    fn pic_param_packs_bitfields() {
        let sps = parse_sps(SPS_64X64_X265).unwrap();
        let pps = parse_pps(PPS_64X64_X265).unwrap();
        let pic = build_pic_param(&sps, &pps, NAL_IDR_N_LP, 0, 7).unwrap();
        assert_eq!(pic.CurrPic.picture_id, 7);
        assert_eq!(pic.pic_width_in_luma_samples, 64);
        // chroma_format_idc = 1 in bits 0..2.
        assert_eq!(pic.pic_fields & 0b11, 1);
        // NoPicReorderingFlag + NoBiPredFlag set.
        assert_ne!(pic.pic_fields & (1 << 19), 0);
        assert_ne!(pic.pic_fields & (1 << 20), 0);
        // IdrPicFlag + RapPicFlag + IntraPicFlag.
        assert_ne!(pic.slice_parsing_fields & (1 << 11), 0);
        assert_ne!(pic.slice_parsing_fields & (1 << 12), 0);
        assert_ne!(pic.slice_parsing_fields & (1 << 13), 0);
        // All reference frames invalid.
        assert!(
            pic.ReferenceFrames
                .iter()
                .all(|f| f.picture_id == sys::VA_INVALID_SURFACE
                    && f.flags == sys::VA_PICTURE_HEVC_INVALID)
        );
    }

    #[test]
    #[cfg(hwdec_backend = "vaapi")]
    fn uniform_tile_partition_covers_exactly() {
        // 10 CTBs into 3 columns: 3+3+4 (spec derivation gives 3,3,4).
        let parts = uniform_partition_minus1(10, 3);
        assert_eq!(parts.iter().map(|&p| u32::from(p) + 1).sum::<u32>(), 10);
        assert_eq!(parts.len(), 3);
    }

    #[test]
    #[cfg(hwdec_backend = "vaapi")]
    fn slice_param_marks_last_slice_and_i_type() {
        let sh = SliceHeader {
            slice_type: 2,
            slice_data_byte_offset: 7,
            ..SliceHeader::default()
        };
        let pps = Pps::default();
        let param = build_slice_param(&sh, &pps, 123, true);
        assert_eq!(param.slice_data_size, 123);
        assert_eq!(param.LongSliceFlags & 1, 1, "LastSliceOfPic");
        assert_eq!((param.LongSliceFlags >> 2) & 0x3, 2, "slice_type I");
        assert_eq!(param.slice_data_byte_offset, 7);
        assert!(param.RefPicList.iter().flatten().all(|&i| i == 0xff));
    }

    #[test]
    fn nal_classification() {
        assert!(is_irap(NAL_IDR_W_RADL));
        assert!(is_irap(21));
        assert!(!is_irap(NAL_SPS));
        assert!(is_idr(NAL_IDR_N_LP));
        assert!(!is_idr(21));
        assert_eq!(nal_type(&[0x42, 0x01]).unwrap(), 33);
        assert!(nal_type(&[0x80, 0x00]).is_err(), "forbidden bit");
    }
}
