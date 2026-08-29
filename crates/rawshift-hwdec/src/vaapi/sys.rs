//! Minimal hand-written VA-API FFI surface, dlopen'd at runtime.
//!
//! This is the **only** place rawshift declares libva types and function
//! signatures. Everything here was transcribed from and verified against the
//! reference headers `va/va.h`, `va/va_drm.h`, `va/va_dec_hevc.h`, and
//! `va/va_dec_av1.h` (VA-API 1.x — the layouts of the decode parameter
//! buffers are frozen ABI). Only the entry points and struct fields this
//! crate actually calls/fills are declared; nothing is linked at build time.
//!
//! Loading uses [`libloading`]: `libva.so.2` + `libva-drm.so.2` are opened
//! once per process and leaked (see [`runtime`]), so every resolved function
//! pointer stays valid for the process lifetime. Absence of either library
//! degrades to `None` — never a link or startup failure.
//!
//! C bit-field unions (e.g. `pic_fields`) are represented as plain `u32`/
//! `u16`/`u8` words; helper constants/builders in the codec modules pack the
//! bits explicitly (first-declared C bit-field occupies the least significant
//! bits on all Linux ABIs this crate targets, per the System V psABI).

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_int, c_uint, c_void};
use std::sync::OnceLock;

// ── Scalar typedefs (va.h) ──────────────────────────────────────────────────

pub type VADisplay = *mut c_void;
pub type VAStatus = c_int;
pub type VAGenericID = c_uint;
pub type VAConfigID = VAGenericID;
pub type VAContextID = VAGenericID;
pub type VASurfaceID = VAGenericID;
pub type VABufferID = VAGenericID;
pub type VAImageID = VAGenericID;
/// C enums are `int` on every supported target.
pub type VAProfile = c_int;
pub type VAEntrypoint = c_int;
pub type VABufferType = c_int;

pub const VA_STATUS_SUCCESS: VAStatus = 0;
pub const VA_INVALID_ID: u32 = 0xffff_ffff;
pub const VA_INVALID_SURFACE: VASurfaceID = VA_INVALID_ID;

// VAProfile values (va.h).
/// `VAProfileH264ConstrainedBaseline` (va.h).
pub const VA_PROFILE_H264_CONSTRAINED_BASELINE: VAProfile = 13;
/// `VAProfileH264Main` (va.h).
pub const VA_PROFILE_H264_MAIN: VAProfile = 6;
/// `VAProfileH264High` (va.h).
pub const VA_PROFILE_H264_HIGH: VAProfile = 7;
pub const VA_PROFILE_HEVC_MAIN: VAProfile = 17;
pub const VA_PROFILE_HEVC_MAIN10: VAProfile = 18;
pub const VA_PROFILE_AV1_PROFILE0: VAProfile = 32;

// VAEntrypoint values (va.h).
pub const VA_ENTRYPOINT_VLD: VAEntrypoint = 1;

// VABufferType values (va.h).
pub const VA_PICTURE_PARAMETER_BUFFER_TYPE: VABufferType = 0;
pub const VA_IQ_MATRIX_BUFFER_TYPE: VABufferType = 1;
pub const VA_SLICE_PARAMETER_BUFFER_TYPE: VABufferType = 4;
pub const VA_SLICE_DATA_BUFFER_TYPE: VABufferType = 5;

// VAConfigAttribType values (va.h).
pub const VA_CONFIG_ATTRIB_RT_FORMAT: c_int = 0;

// VA_RT_FORMAT_* (va.h).
pub const VA_RT_FORMAT_YUV420: u32 = 0x0000_0001;
pub const VA_RT_FORMAT_YUV420_10: u32 = 0x0000_0100;

// Pixel FOURCCs (va.h).
pub const VA_FOURCC_NV12: u32 = 0x3231_564E;
pub const VA_FOURCC_P010: u32 = 0x3031_3050;

// VAGenericValueType (va.h).
pub const VA_GENERIC_VALUE_TYPE_INTEGER: c_int = 1;

// VASurfaceAttribType (va.h).
pub const VA_SURFACE_ATTRIB_PIXEL_FORMAT: c_int = 1;
pub const VA_SURFACE_ATTRIB_SETTABLE: u32 = 0x0000_0002;

// VA_SLICE_DATA_FLAG_* (va.h).
pub const VA_SLICE_DATA_FLAG_ALL: u32 = 0x00;

// VAPictureHEVC flags (va.h).
pub const VA_PICTURE_HEVC_INVALID: u32 = 0x0000_0001;

/// `VA_PADDING_LOW` (va.h).
pub const VA_PADDING_LOW: usize = 4;
/// `VA_PADDING_MEDIUM` (va.h).
pub const VA_PADDING_MEDIUM: usize = 8;

// ── Core structs (va.h) ─────────────────────────────────────────────────────

/// `VAConfigAttrib` (va.h).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VAConfigAttrib {
    pub type_: c_int,
    pub value: u32,
}

/// The value union of `VAGenericValue` (va.h): `int32_t` / `float` /
/// pointer / function pointer. Pointer-sized and pointer-aligned.
#[repr(C)]
#[derive(Clone, Copy)]
pub union VAGenericValueUnion {
    pub i: i32,
    pub f: f32,
    pub p: *mut c_void,
}

/// `VAGenericValue` (va.h).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VAGenericValue {
    pub type_: c_int,
    pub value: VAGenericValueUnion,
}

/// `VASurfaceAttrib` (va.h).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VASurfaceAttrib {
    pub type_: c_int,
    pub flags: u32,
    pub value: VAGenericValue,
}

/// `VAImageFormat` (va.h).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VAImageFormat {
    pub fourcc: u32,
    pub byte_order: u32,
    pub bits_per_pixel: u32,
    pub depth: u32,
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub alpha_mask: u32,
    pub va_reserved: [u32; VA_PADDING_LOW],
}

/// `VAImage` (va.h).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VAImage {
    pub image_id: VAImageID,
    pub format: VAImageFormat,
    pub buf: VABufferID,
    pub width: u16,
    pub height: u16,
    pub data_size: u32,
    pub num_planes: u32,
    pub pitches: [u32; 3],
    pub offsets: [u32; 3],
    pub num_palette_entries: i32,
    pub entry_bytes: i32,
    pub component_order: [i8; 4],
    pub va_reserved: [u32; VA_PADDING_LOW],
}

impl VAImage {
    /// An all-invalid image value for out-parameter initialisation.
    pub fn zeroed() -> Self {
        VAImage {
            image_id: VA_INVALID_ID,
            format: VAImageFormat::default(),
            buf: VA_INVALID_ID,
            width: 0,
            height: 0,
            data_size: 0,
            num_planes: 0,
            pitches: [0; 3],
            offsets: [0; 3],
            num_palette_entries: 0,
            entry_bytes: 0,
            component_order: [0; 4],
            va_reserved: [0; VA_PADDING_LOW],
        }
    }
}

// ── HEVC decode parameter buffers (va_dec_hevc.h) ───────────────────────────

/// `VAPictureHEVC` (va.h).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VAPictureHEVC {
    pub picture_id: VASurfaceID,
    pub pic_order_cnt: i32,
    pub flags: u32,
    pub va_reserved: [u32; VA_PADDING_LOW],
}

impl VAPictureHEVC {
    /// The canonical "no picture" DPB entry.
    pub fn invalid() -> Self {
        VAPictureHEVC {
            picture_id: VA_INVALID_SURFACE,
            pic_order_cnt: 0,
            flags: VA_PICTURE_HEVC_INVALID,
            va_reserved: [0; VA_PADDING_LOW],
        }
    }
}

/// `VAPictureParameterBufferHEVC` (va_dec_hevc.h). Bit-field unions are the
/// plain words `pic_fields` / `slice_parsing_fields`; the HEVC module packs
/// them.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VAPictureParameterBufferHEVC {
    pub CurrPic: VAPictureHEVC,
    pub ReferenceFrames: [VAPictureHEVC; 15],
    pub pic_width_in_luma_samples: u16,
    pub pic_height_in_luma_samples: u16,
    pub pic_fields: u32,
    pub sps_max_dec_pic_buffering_minus1: u8,
    pub bit_depth_luma_minus8: u8,
    pub bit_depth_chroma_minus8: u8,
    pub pcm_sample_bit_depth_luma_minus1: u8,
    pub pcm_sample_bit_depth_chroma_minus1: u8,
    pub log2_min_luma_coding_block_size_minus3: u8,
    pub log2_diff_max_min_luma_coding_block_size: u8,
    pub log2_min_transform_block_size_minus2: u8,
    pub log2_diff_max_min_transform_block_size: u8,
    pub log2_min_pcm_luma_coding_block_size_minus3: u8,
    pub log2_diff_max_min_pcm_luma_coding_block_size: u8,
    pub max_transform_hierarchy_depth_intra: u8,
    pub max_transform_hierarchy_depth_inter: u8,
    pub init_qp_minus26: i8,
    pub diff_cu_qp_delta_depth: u8,
    pub pps_cb_qp_offset: i8,
    pub pps_cr_qp_offset: i8,
    pub log2_parallel_merge_level_minus2: u8,
    pub num_tile_columns_minus1: u8,
    pub num_tile_rows_minus1: u8,
    pub column_width_minus1: [u16; 19],
    pub row_height_minus1: [u16; 21],
    pub slice_parsing_fields: u32,
    pub log2_max_pic_order_cnt_lsb_minus4: u8,
    pub num_short_term_ref_pic_sets: u8,
    pub num_long_term_ref_pic_sps: u8,
    pub num_ref_idx_l0_default_active_minus1: u8,
    pub num_ref_idx_l1_default_active_minus1: u8,
    pub pps_beta_offset_div2: i8,
    pub pps_tc_offset_div2: i8,
    pub num_extra_slice_header_bits: u8,
    pub st_rps_bits: u32,
    pub va_reserved: [u32; VA_PADDING_MEDIUM],
}

/// `VASliceParameterBufferHEVC` (va_dec_hevc.h). `LongSliceFlags` is the
/// plain word form of the C bit-field union.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VASliceParameterBufferHEVC {
    pub slice_data_size: u32,
    pub slice_data_offset: u32,
    pub slice_data_flag: u32,
    pub slice_data_byte_offset: u32,
    pub slice_segment_address: u32,
    pub RefPicList: [[u8; 15]; 2],
    pub LongSliceFlags: u32,
    pub collocated_ref_idx: u8,
    pub num_ref_idx_l0_active_minus1: u8,
    pub num_ref_idx_l1_active_minus1: u8,
    pub slice_qp_delta: i8,
    pub slice_cb_qp_offset: i8,
    pub slice_cr_qp_offset: i8,
    pub slice_beta_offset_div2: i8,
    pub slice_tc_offset_div2: i8,
    pub luma_log2_weight_denom: u8,
    pub delta_chroma_log2_weight_denom: i8,
    pub delta_luma_weight_l0: [i8; 15],
    pub luma_offset_l0: [i8; 15],
    pub delta_chroma_weight_l0: [[i8; 2]; 15],
    pub ChromaOffsetL0: [[i8; 2]; 15],
    pub delta_luma_weight_l1: [i8; 15],
    pub luma_offset_l1: [i8; 15],
    pub delta_chroma_weight_l1: [[i8; 2]; 15],
    pub ChromaOffsetL1: [[i8; 2]; 15],
    pub five_minus_max_num_merge_cand: u8,
    pub num_entry_point_offsets: u16,
    pub entry_offset_to_subset_array: u16,
    pub slice_data_num_emu_prevn_bytes: u16,
    pub va_reserved: [u32; VA_PADDING_LOW - 2],
}

// ── AV1 decode parameter buffers (va_dec_av1.h) ─────────────────────────────

/// `VASegmentationStructAV1` (va_dec_av1.h).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VASegmentationStructAV1 {
    pub segment_info_fields: u32,
    pub feature_data: [[i16; 8]; 8],
    pub feature_mask: [u8; 8],
    pub va_reserved: [u32; VA_PADDING_LOW],
}

/// `VAFilmGrainStructAV1` (va_dec_av1.h).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VAFilmGrainStructAV1 {
    pub film_grain_info_fields: u32,
    pub grain_seed: u16,
    pub num_y_points: u8,
    pub point_y_value: [u8; 14],
    pub point_y_scaling: [u8; 14],
    pub num_cb_points: u8,
    pub point_cb_value: [u8; 10],
    pub point_cb_scaling: [u8; 10],
    pub num_cr_points: u8,
    pub point_cr_value: [u8; 10],
    pub point_cr_scaling: [u8; 10],
    pub ar_coeffs_y: [i8; 24],
    pub ar_coeffs_cb: [i8; 25],
    pub ar_coeffs_cr: [i8; 25],
    pub cb_mult: u8,
    pub cb_luma_mult: u8,
    pub cb_offset: u16,
    pub cr_mult: u8,
    pub cr_luma_mult: u8,
    pub cr_offset: u16,
    pub va_reserved: [u32; VA_PADDING_LOW],
}

/// `VAWarpedMotionParamsAV1` (va_dec_av1.h). `wmtype` is the
/// `VAAV1TransformationType` C enum (an `int`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VAWarpedMotionParamsAV1 {
    pub wmtype: c_int,
    pub wmmat: [i32; 8],
    pub invalid: u8,
    pub va_reserved: [u32; VA_PADDING_LOW],
}

impl VAWarpedMotionParamsAV1 {
    /// The identity global-motion transform (`wmtype` 0, unit diagonal in
    /// the 1<<16 fixed-point convention).
    pub fn identity() -> Self {
        let mut wmmat = [0i32; 8];
        wmmat[2] = 1 << 16;
        wmmat[5] = 1 << 16;
        VAWarpedMotionParamsAV1 {
            wmtype: 0,
            wmmat,
            invalid: 0,
            va_reserved: [0; VA_PADDING_LOW],
        }
    }
}

/// `VADecPictureParameterBufferAV1` (va_dec_av1.h). Bit-field unions are
/// plain words; `anchor_frames_list` is null for non-large-scale-tile
/// decode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VADecPictureParameterBufferAV1 {
    pub profile: u8,
    pub order_hint_bits_minus_1: u8,
    pub bit_depth_idx: u8,
    pub matrix_coefficients: u8,
    pub seq_info_fields: u32,
    pub current_frame: VASurfaceID,
    pub current_display_picture: VASurfaceID,
    pub anchor_frames_num: u8,
    pub anchor_frames_list: *mut VASurfaceID,
    pub frame_width_minus1: u16,
    pub frame_height_minus1: u16,
    pub output_frame_width_in_tiles_minus_1: u16,
    pub output_frame_height_in_tiles_minus_1: u16,
    pub ref_frame_map: [VASurfaceID; 8],
    pub ref_frame_idx: [u8; 7],
    pub primary_ref_frame: u8,
    pub order_hint: u8,
    pub seg_info: VASegmentationStructAV1,
    pub film_grain_info: VAFilmGrainStructAV1,
    pub tile_cols: u8,
    pub tile_rows: u8,
    pub width_in_sbs_minus_1: [u16; 63],
    pub height_in_sbs_minus_1: [u16; 63],
    pub tile_count_minus_1: u16,
    pub context_update_tile_id: u16,
    pub pic_info_fields: u32,
    pub superres_scale_denominator: u8,
    pub interp_filter: u8,
    pub filter_level: [u8; 2],
    pub filter_level_u: u8,
    pub filter_level_v: u8,
    pub loop_filter_info_fields: u8,
    pub ref_deltas: [i8; 8],
    pub mode_deltas: [i8; 2],
    pub base_qindex: u8,
    pub y_dc_delta_q: i8,
    pub u_dc_delta_q: i8,
    pub u_ac_delta_q: i8,
    pub v_dc_delta_q: i8,
    pub v_ac_delta_q: i8,
    pub qmatrix_fields: u16,
    pub mode_control_fields: u32,
    pub cdef_damping_minus_3: u8,
    pub cdef_bits: u8,
    pub cdef_y_strengths: [u8; 8],
    pub cdef_uv_strengths: [u8; 8],
    pub loop_restoration_fields: u16,
    pub wm: [VAWarpedMotionParamsAV1; 7],
    pub va_reserved: [u32; VA_PADDING_MEDIUM],
}

/// `VASliceParameterBufferAV1` (va_dec_av1.h) — per-tile parameters.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VASliceParameterBufferAV1 {
    pub slice_data_size: u32,
    pub slice_data_offset: u32,
    pub slice_data_flag: u32,
    pub tile_row: u16,
    pub tile_column: u16,
    pub tg_start: u16,
    pub tg_end: u16,
    pub anchor_frame_idx: u8,
    pub tile_idx_in_tile_list: u16,
    pub va_reserved: [u32; VA_PADDING_LOW],
}

// ── Function-pointer table ──────────────────────────────────────────────────

/// The resolved VA entry points this crate calls, plus the two open library
/// handles that keep them valid.
///
/// Held only as `&'static Runtime` (leaked once per process by [`runtime`]),
/// so the libraries are never unloaded and the function pointers never
/// dangle.
pub struct Runtime {
    _libva: libloading::Library,
    _libva_drm: libloading::Library,

    pub vaGetDisplayDRM: unsafe extern "C" fn(fd: c_int) -> VADisplay,
    pub vaInitialize:
        unsafe extern "C" fn(dpy: VADisplay, major: *mut c_int, minor: *mut c_int) -> VAStatus,
    pub vaTerminate: unsafe extern "C" fn(dpy: VADisplay) -> VAStatus,
    pub vaErrorStr: unsafe extern "C" fn(status: VAStatus) -> *const c_char,
    pub vaMaxNumProfiles: unsafe extern "C" fn(dpy: VADisplay) -> c_int,
    pub vaQueryConfigProfiles: unsafe extern "C" fn(
        dpy: VADisplay,
        profile_list: *mut VAProfile,
        num_profiles: *mut c_int,
    ) -> VAStatus,
    pub vaMaxNumEntrypoints: unsafe extern "C" fn(dpy: VADisplay) -> c_int,
    pub vaQueryConfigEntrypoints: unsafe extern "C" fn(
        dpy: VADisplay,
        profile: VAProfile,
        entrypoint_list: *mut VAEntrypoint,
        num_entrypoints: *mut c_int,
    ) -> VAStatus,
    pub vaCreateConfig: unsafe extern "C" fn(
        dpy: VADisplay,
        profile: VAProfile,
        entrypoint: VAEntrypoint,
        attrib_list: *mut VAConfigAttrib,
        num_attribs: c_int,
        config_id: *mut VAConfigID,
    ) -> VAStatus,
    pub vaDestroyConfig: unsafe extern "C" fn(dpy: VADisplay, config_id: VAConfigID) -> VAStatus,
    pub vaCreateSurfaces: unsafe extern "C" fn(
        dpy: VADisplay,
        format: c_uint,
        width: c_uint,
        height: c_uint,
        surfaces: *mut VASurfaceID,
        num_surfaces: c_uint,
        attrib_list: *mut VASurfaceAttrib,
        num_attribs: c_uint,
    ) -> VAStatus,
    pub vaDestroySurfaces:
        unsafe extern "C" fn(dpy: VADisplay, surfaces: *mut VASurfaceID, num: c_int) -> VAStatus,
    pub vaCreateContext: unsafe extern "C" fn(
        dpy: VADisplay,
        config_id: VAConfigID,
        picture_width: c_int,
        picture_height: c_int,
        flag: c_int,
        render_targets: *mut VASurfaceID,
        num_render_targets: c_int,
        context: *mut VAContextID,
    ) -> VAStatus,
    pub vaDestroyContext: unsafe extern "C" fn(dpy: VADisplay, context: VAContextID) -> VAStatus,
    pub vaCreateBuffer: unsafe extern "C" fn(
        dpy: VADisplay,
        context: VAContextID,
        type_: VABufferType,
        size: c_uint,
        num_elements: c_uint,
        data: *mut c_void,
        buf_id: *mut VABufferID,
    ) -> VAStatus,
    pub vaDestroyBuffer: unsafe extern "C" fn(dpy: VADisplay, buffer_id: VABufferID) -> VAStatus,
    pub vaBeginPicture: unsafe extern "C" fn(
        dpy: VADisplay,
        context: VAContextID,
        render_target: VASurfaceID,
    ) -> VAStatus,
    pub vaRenderPicture: unsafe extern "C" fn(
        dpy: VADisplay,
        context: VAContextID,
        buffers: *mut VABufferID,
        num_buffers: c_int,
    ) -> VAStatus,
    pub vaEndPicture: unsafe extern "C" fn(dpy: VADisplay, context: VAContextID) -> VAStatus,
    pub vaSyncSurface: unsafe extern "C" fn(dpy: VADisplay, render_target: VASurfaceID) -> VAStatus,
    pub vaDeriveImage:
        unsafe extern "C" fn(dpy: VADisplay, surface: VASurfaceID, image: *mut VAImage) -> VAStatus,
    pub vaCreateImage: unsafe extern "C" fn(
        dpy: VADisplay,
        format: *mut VAImageFormat,
        width: c_int,
        height: c_int,
        image: *mut VAImage,
    ) -> VAStatus,
    pub vaGetImage: unsafe extern "C" fn(
        dpy: VADisplay,
        surface: VASurfaceID,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        image: VAImageID,
    ) -> VAStatus,
    pub vaDestroyImage: unsafe extern "C" fn(dpy: VADisplay, image: VAImageID) -> VAStatus,
    pub vaMapBuffer: unsafe extern "C" fn(
        dpy: VADisplay,
        buf_id: VABufferID,
        pbuf: *mut *mut c_void,
    ) -> VAStatus,
    pub vaUnmapBuffer: unsafe extern "C" fn(dpy: VADisplay, buf_id: VABufferID) -> VAStatus,
}

// SAFETY: `Runtime` holds library handles and plain function pointers; none
// of them are thread-affine (libva entry points may be called from any
// thread as long as each display is used with external synchronisation,
// which the decoder guarantees by owning its display exclusively).
unsafe impl Send for Runtime {}
// SAFETY: sharing `&Runtime` across threads only shares immutable function
// pointers; all mutation happens through VA objects owned by one decoder.
unsafe impl Sync for Runtime {}

/// Resolve one symbol from `lib`, converting to a plain function pointer.
///
/// The macro keeps every resolution site uniform so the SAFETY argument is
/// auditable in one place.
macro_rules! sym {
    ($lib:expr, $name:literal) => {{
        // SAFETY: the requested symbol is a C function exported by libva
        // with exactly the signature declared in the `Runtime` field this
        // value is assigned to (transcribed from the reference headers) —
        // that field type is what `Symbol<T>` is instantiated at, so the
        // deref below copies out a correctly-typed fn pointer. The pointer
        // is only stored next to the owning `Library`, which lives for the
        // whole process (the `Runtime` is leaked), so it never outlives the
        // mapping.
        let symbol = unsafe { $lib.get($name) }.ok()?;
        *symbol
    }};
}

impl Runtime {
    /// dlopen `libva.so.2` + `libva-drm.so.2` and resolve every entry point.
    ///
    /// Returns `None` when either library or any symbol is missing — the
    /// caller treats that as "no VAAPI on this machine".
    fn load() -> Option<Self> {
        // SAFETY: dlopen of the system libva libraries. libva's constructors
        // are safe to run at any time; we never dlclose (the Runtime is
        // leaked), so no unloading hazards exist.
        let libva = unsafe { libloading::Library::new("libva.so.2") }.ok()?;
        // SAFETY: as above, for the DRM display glue library.
        let libva_drm = unsafe { libloading::Library::new("libva-drm.so.2") }.ok()?;

        Some(Runtime {
            vaGetDisplayDRM: sym!(libva_drm, b"vaGetDisplayDRM\0"),
            vaInitialize: sym!(libva, b"vaInitialize\0"),
            vaTerminate: sym!(libva, b"vaTerminate\0"),
            vaErrorStr: sym!(libva, b"vaErrorStr\0"),
            vaMaxNumProfiles: sym!(libva, b"vaMaxNumProfiles\0"),
            vaQueryConfigProfiles: sym!(libva, b"vaQueryConfigProfiles\0"),
            vaMaxNumEntrypoints: sym!(libva, b"vaMaxNumEntrypoints\0"),
            vaQueryConfigEntrypoints: sym!(libva, b"vaQueryConfigEntrypoints\0"),
            vaCreateConfig: sym!(libva, b"vaCreateConfig\0"),
            vaDestroyConfig: sym!(libva, b"vaDestroyConfig\0"),
            vaCreateSurfaces: sym!(libva, b"vaCreateSurfaces\0"),
            vaDestroySurfaces: sym!(libva, b"vaDestroySurfaces\0"),
            vaCreateContext: sym!(libva, b"vaCreateContext\0"),
            vaDestroyContext: sym!(libva, b"vaDestroyContext\0"),
            vaCreateBuffer: sym!(libva, b"vaCreateBuffer\0"),
            vaDestroyBuffer: sym!(libva, b"vaDestroyBuffer\0"),
            vaBeginPicture: sym!(libva, b"vaBeginPicture\0"),
            vaRenderPicture: sym!(libva, b"vaRenderPicture\0"),
            vaEndPicture: sym!(libva, b"vaEndPicture\0"),
            vaSyncSurface: sym!(libva, b"vaSyncSurface\0"),
            vaDeriveImage: sym!(libva, b"vaDeriveImage\0"),
            vaCreateImage: sym!(libva, b"vaCreateImage\0"),
            vaGetImage: sym!(libva, b"vaGetImage\0"),
            vaDestroyImage: sym!(libva, b"vaDestroyImage\0"),
            vaMapBuffer: sym!(libva, b"vaMapBuffer\0"),
            vaUnmapBuffer: sym!(libva, b"vaUnmapBuffer\0"),
            _libva: libva,
            _libva_drm: libva_drm,
        })
    }

    /// A human-readable message for a VA status code.
    pub fn error_str(&self, status: VAStatus) -> String {
        // SAFETY: vaErrorStr returns a pointer to a static, NUL-terminated
        // string table entry for any status value (libva guarantees a
        // fallback string for unknown codes); it is never freed.
        let ptr = unsafe { (self.vaErrorStr)(status) };
        if ptr.is_null() {
            return format!("VA status {status}");
        }
        // SAFETY: `ptr` is a valid NUL-terminated C string per the above.
        let cstr = unsafe { std::ffi::CStr::from_ptr(ptr) };
        format!("{} (VA status {status})", cstr.to_string_lossy())
    }
}

/// The per-process VA runtime: dlopen'd on first use, `None` when libva is
/// not present. Never unloaded.
pub fn runtime() -> Option<&'static Runtime> {
    static RUNTIME: OnceLock<Option<Runtime>> = OnceLock::new();
    RUNTIME.get_or_init(Runtime::load).as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    // Layout constants verified against the reference C headers with
    // `gcc -m64` on x86_64 (sizeof/offsetof of each struct as declared in
    // va.h / va_dec_hevc.h / va_dec_av1.h). They pin the repr(C) transcription
    // above to the frozen libva ABI.

    #[test]
    fn core_struct_layouts_match_va_abi() {
        assert_eq!(size_of::<VAConfigAttrib>(), 8);
        assert_eq!(size_of::<VAGenericValue>(), 16);
        assert_eq!(align_of::<VAGenericValue>(), 8);
        assert_eq!(size_of::<VASurfaceAttrib>(), 24);
        assert_eq!(size_of::<VAImageFormat>(), 48);
        assert_eq!(size_of::<VAImage>(), 120);
        assert_eq!(offset_of!(VAImage, buf), 52);
        assert_eq!(offset_of!(VAImage, pitches), 68);
        assert_eq!(offset_of!(VAImage, offsets), 80);
    }

    #[test]
    fn hevc_struct_layouts_match_va_abi() {
        assert_eq!(size_of::<VAPictureHEVC>(), 28);
        assert_eq!(size_of::<VAPictureParameterBufferHEVC>(), 604);
        assert_eq!(
            offset_of!(VAPictureParameterBufferHEVC, pic_width_in_luma_samples),
            448
        );
        assert_eq!(offset_of!(VAPictureParameterBufferHEVC, pic_fields), 452);
        assert_eq!(
            offset_of!(VAPictureParameterBufferHEVC, column_width_minus1),
            476
        );
        assert_eq!(
            offset_of!(VAPictureParameterBufferHEVC, row_height_minus1),
            514
        );
        assert_eq!(
            offset_of!(VAPictureParameterBufferHEVC, slice_parsing_fields),
            556
        );
        assert_eq!(offset_of!(VAPictureParameterBufferHEVC, st_rps_bits), 568);

        assert_eq!(size_of::<VASliceParameterBufferHEVC>(), 264);
        assert_eq!(offset_of!(VASliceParameterBufferHEVC, RefPicList), 20);
        assert_eq!(offset_of!(VASliceParameterBufferHEVC, LongSliceFlags), 52);
        assert_eq!(
            offset_of!(VASliceParameterBufferHEVC, delta_luma_weight_l0),
            66
        );
        assert_eq!(
            offset_of!(VASliceParameterBufferHEVC, five_minus_max_num_merge_cand),
            246
        );
        assert_eq!(
            offset_of!(VASliceParameterBufferHEVC, num_entry_point_offsets),
            248
        );
        assert_eq!(
            offset_of!(VASliceParameterBufferHEVC, slice_data_num_emu_prevn_bytes),
            252
        );
    }

    #[test]
    fn av1_struct_layouts_match_va_abi() {
        assert_eq!(size_of::<VASegmentationStructAV1>(), 156);
        assert_eq!(size_of::<VAFilmGrainStructAV1>(), 176);
        assert_eq!(offset_of!(VAFilmGrainStructAV1, ar_coeffs_y), 77);
        assert_eq!(offset_of!(VAFilmGrainStructAV1, cb_mult), 151);
        assert_eq!(size_of::<VAWarpedMotionParamsAV1>(), 56);
        assert_eq!(offset_of!(VAWarpedMotionParamsAV1, invalid), 36);

        assert_eq!(size_of::<VADecPictureParameterBufferAV1>(), 1160);
        assert_eq!(
            offset_of!(VADecPictureParameterBufferAV1, anchor_frames_list),
            24
        );
        assert_eq!(
            offset_of!(VADecPictureParameterBufferAV1, ref_frame_map),
            40
        );
        assert_eq!(offset_of!(VADecPictureParameterBufferAV1, seg_info), 84);
        assert_eq!(
            offset_of!(VADecPictureParameterBufferAV1, film_grain_info),
            240
        );
        assert_eq!(offset_of!(VADecPictureParameterBufferAV1, tile_cols), 416);
        assert_eq!(
            offset_of!(VADecPictureParameterBufferAV1, tile_count_minus_1),
            670
        );
        assert_eq!(
            offset_of!(VADecPictureParameterBufferAV1, pic_info_fields),
            676
        );
        assert_eq!(offset_of!(VADecPictureParameterBufferAV1, base_qindex), 697);
        assert_eq!(
            offset_of!(VADecPictureParameterBufferAV1, mode_control_fields),
            708
        );
        assert_eq!(
            offset_of!(VADecPictureParameterBufferAV1, loop_restoration_fields),
            730
        );
        assert_eq!(offset_of!(VADecPictureParameterBufferAV1, wm), 732);

        assert_eq!(size_of::<VASliceParameterBufferAV1>(), 40);
        assert_eq!(offset_of!(VASliceParameterBufferAV1, tile_row), 12);
        assert_eq!(offset_of!(VASliceParameterBufferAV1, anchor_frame_idx), 20);
        assert_eq!(
            offset_of!(VASliceParameterBufferAV1, tile_idx_in_tile_list),
            22
        );
    }
}
