//! VAAPI (libva) still-frame decode backend for linux-gnu.
//!
//! ## Runtime model
//!
//! libva is **dlopen'd at runtime** (`libva.so.2` + `libva-drm.so.2` via
//! [`sys::runtime`]); nothing links against it. When the libraries, a usable
//! `/dev/dri/renderD*` node, or the VLD entry points are missing, the crate
//! degrades to [`decoder`]`() == None` — never a link or startup failure, so
//! headless/CI machines are safe.
//!
//! ## Codec scope (see `hevc`/`av1` module docs for the precise contract)
//!
//! - **HEVC** Main / Main 10 still pictures (HEIC): IRAP intra frame(s),
//!   4:2:0 or monochrome, 8/10-bit → NV12 / P010.
//! - **AV1** Profile 0 (Main) still pictures (AVIF): one intra frame, any
//!   tiling, optional film grain, 8/10-bit 4:2:0 or monochrome → NV12 /
//!   P010.
//!
//! Streams outside this scope fail with a descriptive
//! [`HwDecodeError::Decode`]; hardware/driver rejections surface the libva
//! status string.
//!
//! ## GPU vendor coverage
//!
//! VAAPI covers Intel (media-driver/i965) and AMD (Mesa radeonsi) natively.
//! **NVIDIA GPUs are supported through the maintained
//! [`nvidia-vaapi-driver`](https://github.com/elFarto/nvidia-vaapi-driver)
//! translation layer over NVDEC** — install it and this backend picks it up
//! through the same dlopen path; no rawshift changes needed (see
//! `docs/SUPPORT.md`, "NVDEC" justification).
//!
//! ## Safety boundary
//!
//! All `unsafe` in this backend is FFI: the dlopen/symbol resolution in
//! [`sys`] and the libva call sites in this file, each with a documented
//! invariant. The bitstream parsers ([`bits`], [`hevc`], [`av1`]) are safe
//! Rust.

pub(crate) mod sys;

use std::ffi::c_int;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::sync::OnceLock;

use crate::{
    CodecConfig, ColorRange, DecodedFrame, HwBackend, HwCodec, HwDecodeError, HwStillDecoder,
    PixelFormat, Plane, StillDecodeRequest,
};
use crate::bitstream::{av1, bits, hevc};

// ── Public backend surface (called from lib.rs) ─────────────────────────────

/// Backend hook for [`crate::decoder`].
pub fn decoder(codec: HwCodec) -> Option<Box<dyn HwStillDecoder>> {
    let caps = probe();
    let supported = match codec {
        HwCodec::Hevc => caps.hevc_main || caps.hevc_main10,
        HwCodec::Av1 => caps.av1_profile0,
    };
    if !supported {
        return None;
    }
    let display = VaDisplay::open().ok()?;
    Some(Box::new(VaapiStillDecoder {
        codec,
        display,
        session: None,
    }))
}

/// Backend hook for [`crate::backend`].
pub fn backend() -> Option<HwBackend> {
    let caps = probe();
    (caps.hevc_main || caps.hevc_main10 || caps.av1_profile0).then_some(HwBackend::Vaapi)
}

/// Backend hook for [`crate::available_codecs`].
pub fn available_codecs() -> &'static [HwCodec] {
    const NONE: &[HwCodec] = &[];
    const HEVC_ONLY: &[HwCodec] = &[HwCodec::Hevc];
    const AV1_ONLY: &[HwCodec] = &[HwCodec::Av1];
    const BOTH: &[HwCodec] = &[HwCodec::Hevc, HwCodec::Av1];
    let caps = probe();
    match (caps.hevc_main || caps.hevc_main10, caps.av1_profile0) {
        (true, true) => BOTH,
        (true, false) => HEVC_ONLY,
        (false, true) => AV1_ONLY,
        (false, false) => NONE,
    }
}

// ── Runtime probe ───────────────────────────────────────────────────────────

/// What the driver actually decodes, per `vaQueryConfigProfiles` +
/// `vaQueryConfigEntrypoints` (VLD).
#[derive(Debug, Clone, Copy, Default)]
struct Caps {
    hevc_main: bool,
    hevc_main10: bool,
    av1_profile0: bool,
}

/// Probe once per process: dlopen, open a render node, init a display,
/// query profiles/entrypoints, terminate. Hardware does not change at
/// runtime, so the result is cached.
fn probe() -> Caps {
    static CAPS: OnceLock<Caps> = OnceLock::new();
    *CAPS.get_or_init(|| probe_uncached().unwrap_or_default())
}

fn probe_uncached() -> Option<Caps> {
    let display = VaDisplay::open().ok()?;
    let rt = display.rt;
    let dpy = display.dpy;

    // SAFETY: `dpy` is a valid, initialised display owned by `display`;
    // vaMaxNumProfiles only reads driver metadata.
    let max_profiles = unsafe { (rt.vaMaxNumProfiles)(dpy) };
    if max_profiles <= 0 {
        return None;
    }
    let mut profiles = vec![0 as sys::VAProfile; max_profiles as usize];
    let mut num_profiles: c_int = 0;
    // SAFETY: `profiles` has room for vaMaxNumProfiles entries as the API
    // requires; both out-pointers reference live stack/heap storage.
    let status =
        unsafe { (rt.vaQueryConfigProfiles)(dpy, profiles.as_mut_ptr(), &raw mut num_profiles) };
    if status != sys::VA_STATUS_SUCCESS {
        return None;
    }
    profiles.truncate(num_profiles.max(0) as usize);

    let mut caps = Caps::default();
    for &profile in &profiles {
        let interesting = matches!(
            profile,
            sys::VA_PROFILE_HEVC_MAIN | sys::VA_PROFILE_HEVC_MAIN10 | sys::VA_PROFILE_AV1_PROFILE0
        );
        if !interesting {
            continue;
        }
        if !display.has_vld_entrypoint(profile) {
            continue;
        }
        match profile {
            sys::VA_PROFILE_HEVC_MAIN => caps.hevc_main = true,
            sys::VA_PROFILE_HEVC_MAIN10 => caps.hevc_main10 = true,
            sys::VA_PROFILE_AV1_PROFILE0 => caps.av1_profile0 = true,
            _ => {}
        }
    }
    Some(caps)
}

// ── Display session ─────────────────────────────────────────────────────────

/// An initialised VA display over an open DRM render node. Terminated on
/// drop; the `File` (and thus the fd libva borrowed) outlives the display
/// because `Drop::drop` runs before the fields are dropped.
struct VaDisplay {
    rt: &'static sys::Runtime,
    dpy: sys::VADisplay,
    _node: File,
}

impl VaDisplay {
    /// dlopen libva, open the first usable DRM render node, and initialise
    /// a display on it.
    fn open() -> Result<Self, String> {
        let rt = sys::runtime().ok_or("libva.so.2 / libva-drm.so.2 not loadable")?;
        let mut last_err = String::from("no /dev/dri render node found");
        for minor in 128..136 {
            let path = format!("/dev/dri/renderD{minor}");
            let node = match File::options().read(true).write(true).open(&path) {
                Ok(file) => file,
                Err(e) => {
                    if std::path::Path::new(&path).exists() {
                        last_err = format!("{path}: {e}");
                    }
                    continue;
                }
            };
            // SAFETY: `node` is an open DRM fd that stays open for the
            // whole life of the returned display (owned by `VaDisplay`).
            // vaGetDisplayDRM only wraps the fd; it does not take ownership.
            let dpy = unsafe { (rt.vaGetDisplayDRM)(node.as_raw_fd()) };
            if dpy.is_null() {
                last_err = format!("{path}: vaGetDisplayDRM returned null");
                continue;
            }
            let (mut major, mut minor_ver): (c_int, c_int) = (0, 0);
            // SAFETY: `dpy` is a fresh display from vaGetDisplayDRM; the two
            // out-pointers reference live stack variables.
            let status = unsafe { (rt.vaInitialize)(dpy, &raw mut major, &raw mut minor_ver) };
            if status != sys::VA_STATUS_SUCCESS {
                last_err = format!("{path}: vaInitialize: {}", rt.error_str(status));
                continue;
            }
            return Ok(VaDisplay {
                rt,
                dpy,
                _node: node,
            });
        }
        Err(last_err)
    }

    /// Whether `profile` has the VLD (decode) entry point.
    fn has_vld_entrypoint(&self, profile: sys::VAProfile) -> bool {
        // SAFETY: valid display; reads driver metadata only.
        let max = unsafe { (self.rt.vaMaxNumEntrypoints)(self.dpy) };
        if max <= 0 {
            return false;
        }
        let mut entrypoints = vec![0 as sys::VAEntrypoint; max as usize];
        let mut num: c_int = 0;
        // SAFETY: `entrypoints` has room for vaMaxNumEntrypoints entries as
        // the API requires; out-pointers reference live storage.
        let status = unsafe {
            (self.rt.vaQueryConfigEntrypoints)(
                self.dpy,
                profile,
                entrypoints.as_mut_ptr(),
                &raw mut num,
            )
        };
        status == sys::VA_STATUS_SUCCESS
            && entrypoints[..num.max(0) as usize].contains(&sys::VA_ENTRYPOINT_VLD)
    }
}

impl Drop for VaDisplay {
    fn drop(&mut self) {
        // SAFETY: `dpy` was successfully initialised and is terminated
        // exactly once here; the DRM fd is still open (`_node` drops after
        // this function returns).
        unsafe { (self.rt.vaTerminate)(self.dpy) };
    }
}

// ── Decoder ─────────────────────────────────────────────────────────────────

/// Key of a reusable decode session (config + context + surfaces).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionKey {
    profile: sys::VAProfile,
    width: u32,
    height: u32,
    rt_format: u32,
    fourcc: u32,
    num_surfaces: u32,
}

/// A live VA config/context/surface set for one picture geometry. Grid HEICs
/// decode hundreds of identically-sized tiles, so reusing the session across
/// [`decode_still`](HwStillDecoder::decode_still) calls is a large win.
struct Session {
    key: SessionKey,
    config: sys::VAConfigID,
    context: sys::VAContextID,
    surfaces: Vec<sys::VASurfaceID>,
}

/// The VAAPI [`HwStillDecoder`]: one display per decoder (moved freely
/// across threads; all calls take `&mut self`, so the display is never used
/// concurrently).
struct VaapiStillDecoder {
    codec: HwCodec,
    display: VaDisplay,
    session: Option<Session>,
}

// SAFETY: the decoder owns its display/context exclusively (`&mut self` on
// the only entry point) and libva objects are not thread-affine — they may
// be used from any thread as long as calls are externally synchronised,
// which exclusive ownership guarantees. `Send` is required by the
// `HwStillDecoder` contract.
unsafe impl Send for VaapiStillDecoder {}

impl Drop for VaapiStillDecoder {
    fn drop(&mut self) {
        self.destroy_session();
    }
}

impl HwStillDecoder for VaapiStillDecoder {
    fn decode_still(
        &mut self,
        request: &StillDecodeRequest<'_>,
    ) -> Result<DecodedFrame, HwDecodeError> {
        if request.codec() != self.codec {
            return Err(HwDecodeError::Decode {
                codec: request.codec(),
                message: format!("decoder was opened for {}", self.codec),
            });
        }
        match request.config {
            CodecConfig::Hvcc(hvcc) => self.decode_hevc(hvcc, request),
            CodecConfig::Av1c(av1c) => self.decode_av1(av1c, request),
        }
    }
}

/// Shorthand for a decode-stage failure.
fn decode_err(codec: HwCodec, message: impl Into<String>) -> HwDecodeError {
    HwDecodeError::Decode {
        codec,
        message: message.into(),
    }
}

impl VaapiStillDecoder {
    // ── session management ──────────────────────────────────────────────────

    fn destroy_session(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        let rt = self.display.rt;
        let dpy = self.display.dpy;
        // SAFETY: the context/config/surfaces were created on `dpy` and are
        // destroyed exactly once, context before the surfaces it renders to,
        // as libva requires. Errors on teardown are unrecoverable and
        // ignored.
        unsafe {
            (rt.vaDestroyContext)(dpy, session.context);
            let mut surfaces = session.surfaces;
            (rt.vaDestroySurfaces)(dpy, surfaces.as_mut_ptr(), surfaces.len() as c_int);
            (rt.vaDestroyConfig)(dpy, session.config);
        }
    }

    /// Get or (re)create the session for `key`.
    fn session(&mut self, key: SessionKey) -> Result<&Session, String> {
        if self.session.as_ref().is_some_and(|s| s.key == key) {
            return Ok(self.session.as_ref().unwrap());
        }
        self.destroy_session();

        let rt = self.display.rt;
        let dpy = self.display.dpy;

        let mut attrib = sys::VAConfigAttrib {
            type_: sys::VA_CONFIG_ATTRIB_RT_FORMAT,
            value: key.rt_format,
        };
        let mut config: sys::VAConfigID = sys::VA_INVALID_ID;
        // SAFETY: valid display; `attrib` and `config` reference live stack
        // storage; profile/entrypoint values come from the probe.
        let status = unsafe {
            (rt.vaCreateConfig)(
                dpy,
                key.profile,
                sys::VA_ENTRYPOINT_VLD,
                &raw mut attrib,
                1,
                &raw mut config,
            )
        };
        if status != sys::VA_STATUS_SUCCESS {
            return Err(format!("vaCreateConfig: {}", rt.error_str(status)));
        }
        // From here on, clean up on failure.
        let destroy_config = |config| {
            // SAFETY: `config` was created above on `dpy`; destroyed once.
            unsafe { (rt.vaDestroyConfig)(dpy, config) };
        };

        let mut surfaces = vec![sys::VA_INVALID_SURFACE; key.num_surfaces as usize];
        let mut pixel_format_attrib = sys::VASurfaceAttrib {
            type_: sys::VA_SURFACE_ATTRIB_PIXEL_FORMAT,
            flags: sys::VA_SURFACE_ATTRIB_SETTABLE,
            value: sys::VAGenericValue {
                type_: sys::VA_GENERIC_VALUE_TYPE_INTEGER,
                value: sys::VAGenericValueUnion {
                    i: key.fourcc as i32,
                },
            },
        };
        // SAFETY: valid display; `surfaces` has exactly `num_surfaces`
        // slots; the attribute array holds one initialised element.
        let status = unsafe {
            (rt.vaCreateSurfaces)(
                dpy,
                key.rt_format,
                key.width,
                key.height,
                surfaces.as_mut_ptr(),
                key.num_surfaces,
                &raw mut pixel_format_attrib,
                1,
            )
        };
        if status != sys::VA_STATUS_SUCCESS {
            destroy_config(config);
            return Err(format!("vaCreateSurfaces: {}", rt.error_str(status)));
        }

        let mut context: sys::VAContextID = sys::VA_INVALID_ID;
        // SAFETY: valid display/config; `surfaces` are the render targets
        // just created; `context` references live stack storage.
        let status = unsafe {
            (rt.vaCreateContext)(
                dpy,
                config,
                key.width as c_int,
                key.height as c_int,
                0x1, // VA_PROGRESSIVE
                surfaces.as_mut_ptr(),
                surfaces.len() as c_int,
                &raw mut context,
            )
        };
        if status != sys::VA_STATUS_SUCCESS {
            // SAFETY: surfaces were created above on `dpy`; destroyed once.
            unsafe { (rt.vaDestroySurfaces)(dpy, surfaces.as_mut_ptr(), surfaces.len() as c_int) };
            destroy_config(config);
            return Err(format!("vaCreateContext: {}", rt.error_str(status)));
        }

        self.session = Some(Session {
            key,
            config,
            context,
            surfaces,
        });
        Ok(self.session.as_ref().unwrap())
    }

    /// Create one parameter/data buffer of `type_` from `bytes`.
    fn create_buffer(
        &self,
        context: sys::VAContextID,
        type_: sys::VABufferType,
        bytes: &[u8],
    ) -> Result<sys::VABufferID, String> {
        let rt = self.display.rt;
        let mut buf: sys::VABufferID = sys::VA_INVALID_ID;
        // SAFETY: valid display/context; `bytes` is a live, initialised
        // slice of exactly `size` bytes which libva copies before returning
        // (data is passed non-null, so no later aliasing); `buf` references
        // live stack storage.
        let status = unsafe {
            (rt.vaCreateBuffer)(
                self.display.dpy,
                context,
                type_,
                bytes.len() as u32,
                1,
                bytes.as_ptr() as *mut std::ffi::c_void,
                &raw mut buf,
            )
        };
        if status != sys::VA_STATUS_SUCCESS {
            return Err(format!("vaCreateBuffer: {}", rt.error_str(status)));
        }
        Ok(buf)
    }

    /// Run one begin/render/end/sync cycle over `buffers` targeting
    /// `surface`, destroying the buffers afterwards.
    fn submit(
        &self,
        context: sys::VAContextID,
        surface: sys::VASurfaceID,
        buffers: &mut Vec<sys::VABufferID>,
    ) -> Result<(), String> {
        let rt = self.display.rt;
        let dpy = self.display.dpy;
        let destroy_buffers = |buffers: &mut Vec<sys::VABufferID>| {
            for &buf in buffers.iter() {
                // SAFETY: each id was created on `dpy` by create_buffer and
                // is destroyed exactly once here.
                unsafe { (rt.vaDestroyBuffer)(dpy, buf) };
            }
            buffers.clear();
        };

        // SAFETY: valid display/context; `surface` is one of the context's
        // render targets.
        let status = unsafe { (rt.vaBeginPicture)(dpy, context, surface) };
        if status != sys::VA_STATUS_SUCCESS {
            destroy_buffers(buffers);
            return Err(format!("vaBeginPicture: {}", rt.error_str(status)));
        }
        // SAFETY: `buffers` holds ids created on this context; the slice
        // pointer/length pair is valid for the call.
        let status = unsafe {
            (rt.vaRenderPicture)(dpy, context, buffers.as_mut_ptr(), buffers.len() as c_int)
        };
        if status != sys::VA_STATUS_SUCCESS {
            destroy_buffers(buffers);
            return Err(format!("vaRenderPicture: {}", rt.error_str(status)));
        }
        // SAFETY: begin/render succeeded on this context; vaEndPicture
        // consumes the queued picture.
        let status = unsafe { (rt.vaEndPicture)(dpy, context) };
        destroy_buffers(buffers);
        if status != sys::VA_STATUS_SUCCESS {
            return Err(format!("vaEndPicture: {}", rt.error_str(status)));
        }
        // SAFETY: valid display; `surface` has a pending decode to wait on.
        let status = unsafe { (rt.vaSyncSurface)(dpy, surface) };
        if status != sys::VA_STATUS_SUCCESS {
            return Err(format!("vaSyncSurface: {}", rt.error_str(status)));
        }
        Ok(())
    }

    // ── surface readback ────────────────────────────────────────────────────

    /// Read `surface` back to an owned [`DecodedFrame`], cropping to
    /// `crop = (x0, y0, width, height)` in luma samples.
    #[allow(clippy::too_many_arguments)]
    fn read_surface(
        &self,
        surface: sys::VASurfaceID,
        surface_size: (u32, u32),
        crop: (u32, u32, u32, u32),
        expect_fourcc: u32,
        bit_depth: u8,
        range: ColorRange,
    ) -> Result<DecodedFrame, HwDecodeError> {
        let codec = self.codec;
        let rt = self.display.rt;
        let dpy = self.display.dpy;

        let mut image = sys::VAImage::zeroed();
        // SAFETY: valid display; `surface` is synced; `image` is a live
        // out-struct the driver fills on success.
        let mut status = unsafe { (rt.vaDeriveImage)(dpy, surface, &raw mut image) };
        let mut derived = status == sys::VA_STATUS_SUCCESS;
        if derived && image.format.fourcc != expect_fourcc {
            // Unexpected derived layout — fall back to vaGetImage below.
            // SAFETY: `image` was successfully derived and is destroyed once.
            unsafe { (rt.vaDestroyImage)(dpy, image.image_id) };
            derived = false;
        }
        if !derived {
            let mut format = sys::VAImageFormat {
                fourcc: expect_fourcc,
                byte_order: 1, // VA_LSB_FIRST
                bits_per_pixel: if bit_depth > 8 { 24 } else { 12 },
                ..Default::default()
            };
            image = sys::VAImage::zeroed();
            // SAFETY: valid display; `format`/`image` are live structs; on
            // success the library allocates the image buffer it describes.
            status = unsafe {
                (rt.vaCreateImage)(
                    dpy,
                    &raw mut format,
                    surface_size.0 as c_int,
                    surface_size.1 as c_int,
                    &raw mut image,
                )
            };
            if status != sys::VA_STATUS_SUCCESS {
                return Err(decode_err(
                    codec,
                    format!("vaCreateImage: {}", rt.error_str(status)),
                ));
            }
            // SAFETY: valid display/surface/image; the region is within the
            // surface bounds by construction.
            status = unsafe {
                (rt.vaGetImage)(
                    dpy,
                    surface,
                    0,
                    0,
                    surface_size.0,
                    surface_size.1,
                    image.image_id,
                )
            };
            if status != sys::VA_STATUS_SUCCESS {
                // SAFETY: image was created above; destroyed once.
                unsafe { (rt.vaDestroyImage)(dpy, image.image_id) };
                return Err(decode_err(
                    codec,
                    format!("vaGetImage: {}", rt.error_str(status)),
                ));
            }
        }

        let result = self.copy_image_planes(&image, crop, bit_depth, range);

        // SAFETY: `image.buf` was mapped only inside copy_image_planes
        // (which unmaps before returning); the image itself is destroyed
        // exactly once here.
        unsafe { (rt.vaDestroyImage)(dpy, image.image_id) };
        result
    }

    /// Map the image buffer and copy out NV12/P010 planes, cropped.
    fn copy_image_planes(
        &self,
        image: &sys::VAImage,
        crop: (u32, u32, u32, u32),
        bit_depth: u8,
        range: ColorRange,
    ) -> Result<DecodedFrame, HwDecodeError> {
        let codec = self.codec;
        let rt = self.display.rt;
        let dpy = self.display.dpy;
        let (x0, y0, out_w, out_h) = crop;

        if image.num_planes < 2 {
            return Err(decode_err(codec, "driver image has fewer than 2 planes"));
        }
        let format = match image.format.fourcc {
            sys::VA_FOURCC_NV12 => PixelFormat::Nv12,
            sys::VA_FOURCC_P010 => PixelFormat::P010,
            other => {
                return Err(decode_err(
                    codec,
                    format!("driver image has unsupported fourcc {other:#010x}"),
                ));
            }
        };

        let mut mapped: *mut std::ffi::c_void = std::ptr::null_mut();
        // SAFETY: `image.buf` is the image's data buffer created by the
        // library; on success `mapped` points at `data_size` readable bytes
        // which stay valid until vaUnmapBuffer below.
        let status = unsafe { (rt.vaMapBuffer)(dpy, image.buf, &raw mut mapped) };
        if status != sys::VA_STATUS_SUCCESS || mapped.is_null() {
            return Err(decode_err(
                codec,
                format!("vaMapBuffer: {}", rt.error_str(status)),
            ));
        }
        // SAFETY: `mapped` points at `image.data_size` initialised bytes
        // per the vaMapBuffer contract; the slice lives only until the
        // unmap below and is not aliased by other Rust references.
        let data =
            unsafe { std::slice::from_raw_parts(mapped as *const u8, image.data_size as usize) };

        let bps = format.bytes_per_sample();
        let frame = (|| -> Result<DecodedFrame, HwDecodeError> {
            // Luma plane, cropped.
            let luma = copy_plane(
                data,
                image.offsets[0] as usize,
                image.pitches[0] as usize,
                x0 as usize * bps,
                y0 as usize,
                out_w as usize * bps,
                out_h as usize,
            )
            .ok_or_else(|| decode_err(codec, "driver image luma plane is too small"))?;
            // Interleaved CbCr plane: chroma rows are half height; the byte
            // offset of luma column x is x*bps for the CbCr pair.
            let (cw, ch) = (out_w.div_ceil(2) as usize, out_h.div_ceil(2) as usize);
            let chroma = copy_plane(
                data,
                image.offsets[1] as usize,
                image.pitches[1] as usize,
                x0 as usize * bps,
                y0 as usize / 2,
                cw * 2 * bps,
                ch,
            )
            .ok_or_else(|| decode_err(codec, "driver image chroma plane is too small"))?;
            DecodedFrame::new(format, out_w, out_h, bit_depth, range, vec![luma, chroma])
        })();

        // SAFETY: the buffer was mapped above and is unmapped exactly once;
        // the `data` slice is dead after this call (not returned).
        unsafe { (rt.vaUnmapBuffer)(dpy, image.buf) };
        frame
    }

    // ── HEVC ────────────────────────────────────────────────────────────────

    fn decode_hevc(
        &mut self,
        hvcc_bytes: &[u8],
        request: &StillDecodeRequest<'_>,
    ) -> Result<DecodedFrame, HwDecodeError> {
        let codec = HwCodec::Hevc;
        let err = |e: bits::ParseError| decode_err(codec, e.0);

        let hvcc = hevc::parse_hvcc(hvcc_bytes).map_err(err)?;
        let payload_nals =
            hevc::split_length_prefixed(request.payload, hvcc.nal_length_size).map_err(err)?;

        // Parameter sets may sit in the hvcC arrays, the payload, or both;
        // later ones win (in-band overrides, as in a conforming stream).
        let mut sps: Option<hevc::Sps> = None;
        let mut pps: Option<hevc::Pps> = None;
        let mut slices: Vec<&[u8]> = Vec::new();
        for nal in hvcc.nal_units.iter().map(Vec::as_slice).chain(payload_nals) {
            match hevc::nal_type(nal).map_err(err)? {
                hevc::NAL_SPS => sps = Some(hevc::parse_sps(nal).map_err(err)?),
                hevc::NAL_PPS => pps = Some(hevc::parse_pps(nal).map_err(err)?),
                t if hevc::is_irap(t) => slices.push(nal),
                t if t < 32 => {
                    return Err(decode_err(
                        codec,
                        "HEVC non-IRAP coded slice is outside the still-picture scope",
                    ));
                }
                _ => {} // VPS, SEI, …: not needed for decode
            }
        }
        let sps = sps.ok_or_else(|| decode_err(codec, "stream carries no SPS"))?;
        let pps = pps.ok_or_else(|| decode_err(codec, "stream carries no PPS"))?;
        if slices.is_empty() {
            return Err(decode_err(codec, "stream carries no coded slice"));
        }

        if sps.chroma_format_idc > 1 {
            return Err(decode_err(
                codec,
                "HEVC 4:2:2/4:4:4 chroma is outside the still-picture scope (Main/Main10)",
            ));
        }
        let (profile, rt_format, fourcc) = match sps.bit_depth_luma.max(sps.bit_depth_chroma) {
            8 => (
                sys::VA_PROFILE_HEVC_MAIN,
                sys::VA_RT_FORMAT_YUV420,
                sys::VA_FOURCC_NV12,
            ),
            9 | 10 => (
                sys::VA_PROFILE_HEVC_MAIN10,
                sys::VA_RT_FORMAT_YUV420_10,
                sys::VA_FOURCC_P010,
            ),
            _ => {
                return Err(decode_err(
                    codec,
                    "HEVC bit depth above 10 is outside the Main/Main10 scope",
                ));
            }
        };
        let caps = probe();
        let profile_supported = match profile {
            sys::VA_PROFILE_HEVC_MAIN => caps.hevc_main,
            _ => caps.hevc_main10,
        };
        if !profile_supported {
            return Err(HwDecodeError::Unavailable {
                codec,
                reason: "the driver lacks the required HEVC profile (Main/Main10) at VLD"
                    .to_string(),
            });
        }

        // Parse every slice header up front (fail before touching the GPU).
        let mut headers = Vec::with_capacity(slices.len());
        for nal in &slices {
            headers.push(hevc::parse_slice_header(nal, &sps, &pps).map_err(err)?);
        }
        let st_rps_bits = headers
            .iter()
            .find(|h| !h.dependent_slice_segment)
            .map_or(0, |h| h.st_rps_bits);
        let nut = hevc::nal_type(slices[0]).map_err(err)?;

        let key = SessionKey {
            profile,
            width: sps.pic_width_in_luma_samples,
            height: sps.pic_height_in_luma_samples,
            rt_format,
            fourcc,
            num_surfaces: 1,
        };
        let (context, surface) = {
            let session = self.session(key).map_err(|m| decode_err(codec, m))?;
            (session.context, session.surfaces[0])
        };

        let pic_param =
            hevc::build_pic_param(&sps, &pps, nut, st_rps_bits, surface).map_err(err)?;
        let mut buffers = Vec::with_capacity(1 + slices.len() * 2);
        let mut push = |this: &Self, type_, bytes: &[u8]| -> Result<(), HwDecodeError> {
            match this.create_buffer(context, type_, bytes) {
                Ok(id) => {
                    buffers.push(id);
                    Ok(())
                }
                Err(m) => Err(decode_err(codec, m)),
            }
        };
        push(
            self,
            sys::VA_PICTURE_PARAMETER_BUFFER_TYPE,
            as_bytes(&pic_param),
        )?;
        let last = slices.len() - 1;
        for (i, (nal, header)) in slices.iter().zip(&headers).enumerate() {
            let slice_param = hevc::build_slice_param(header, &pps, nal.len() as u32, i == last);
            push(
                self,
                sys::VA_SLICE_PARAMETER_BUFFER_TYPE,
                as_bytes(&slice_param),
            )?;
            push(self, sys::VA_SLICE_DATA_BUFFER_TYPE, nal)?;
        }

        self.submit(context, surface, &mut buffers)
            .map_err(|m| decode_err(codec, m))?;

        let (out_w, out_h) = sps.cropped_size();
        if out_w == 0 || out_h == 0 {
            return Err(decode_err(codec, "conformance window crops away the frame"));
        }
        let range = if sps.video_full_range {
            ColorRange::Full
        } else {
            ColorRange::Limited
        };
        self.read_surface(
            surface,
            (
                sps.pic_width_in_luma_samples,
                sps.pic_height_in_luma_samples,
            ),
            (sps.crop_left, sps.crop_top, out_w, out_h),
            fourcc,
            sps.bit_depth_luma.max(sps.bit_depth_chroma),
            range,
        )
    }

    // ── AV1 ─────────────────────────────────────────────────────────────────

    fn decode_av1(
        &mut self,
        av1c_bytes: &[u8],
        request: &StillDecodeRequest<'_>,
    ) -> Result<DecodedFrame, HwDecodeError> {
        let codec = HwCodec::Av1;
        let err = |e: bits::ParseError| decode_err(codec, e.0);

        let av1c = av1::parse_av1c(av1c_bytes).map_err(err)?;
        if av1c.seq_profile != 0 {
            return Err(decode_err(
                codec,
                "AV1 profile 1/2 (av1C) is outside the VAAPI backend scope (Profile 0 only)",
            ));
        }
        let pic = av1::parse_still_picture(av1c.config_obus, request.payload).map_err(err)?;

        let (rt_format, fourcc) = match pic.seq.bit_depth {
            8 => (sys::VA_RT_FORMAT_YUV420, sys::VA_FOURCC_NV12),
            10 => (sys::VA_RT_FORMAT_YUV420_10, sys::VA_FOURCC_P010),
            _ => return Err(decode_err(codec, "AV1 bit depth outside 8/10")),
        };
        if !probe().av1_profile0 {
            return Err(HwDecodeError::Unavailable {
                codec,
                reason: "the driver lacks AV1 Profile 0 at VLD".to_string(),
            });
        }

        let width = pic.fh.upscaled_width;
        let height = pic.fh.frame_height;
        let apply_grain = pic.fh.film_grain.apply_grain;
        let key = SessionKey {
            profile: sys::VA_PROFILE_AV1_PROFILE0,
            width,
            height,
            rt_format,
            fourcc,
            // Film grain decodes into one surface and composites the grained
            // picture into a second display surface.
            num_surfaces: if apply_grain { 2 } else { 1 },
        };
        let (context, decode_surface, display_surface) = {
            let session = self.session(key).map_err(|m| decode_err(codec, m))?;
            let decode = session.surfaces[0];
            let display = if apply_grain {
                session.surfaces[1]
            } else {
                sys::VA_INVALID_SURFACE
            };
            (session.context, decode, display)
        };

        let pic_param = av1::build_pic_param(&pic, decode_surface, display_surface).map_err(err)?;
        let mut buffers = Vec::with_capacity(1 + pic.tiles.len() * 2);
        let mut push = |this: &Self, type_, bytes: &[u8]| -> Result<(), HwDecodeError> {
            match this.create_buffer(context, type_, bytes) {
                Ok(id) => {
                    buffers.push(id);
                    Ok(())
                }
                Err(m) => Err(decode_err(codec, m)),
            }
        };
        push(
            self,
            sys::VA_PICTURE_PARAMETER_BUFFER_TYPE,
            as_bytes(&pic_param),
        )?;
        for tile in &pic.tiles {
            let tile_param = av1::build_tile_param(tile);
            push(
                self,
                sys::VA_SLICE_PARAMETER_BUFFER_TYPE,
                as_bytes(&tile_param),
            )?;
            push(self, sys::VA_SLICE_DATA_BUFFER_TYPE, &tile.data)?;
        }

        self.submit(context, decode_surface, &mut buffers)
            .map_err(|m| decode_err(codec, m))?;

        let read_from = if apply_grain {
            display_surface
        } else {
            decode_surface
        };
        let range = if pic.seq.color_range_full {
            ColorRange::Full
        } else {
            ColorRange::Limited
        };
        self.read_surface(
            read_from,
            (width, height),
            (0, 0, width, height),
            fourcc,
            pic.seq.bit_depth,
            range,
        )
    }
}

// ── Small safe helpers ──────────────────────────────────────────────────────

/// View a `#[repr(C)]` parameter struct as its raw bytes for vaCreateBuffer.
fn as_bytes<T>(value: &T) -> &[u8] {
    // SAFETY: `T` is one of the `#[repr(C)]` VA parameter structs (plain
    // integers/arrays, fully initialised); reading `size_of::<T>()` bytes at
    // its address is valid, and the borrow keeps the value alive for the
    // returned slice's lifetime.
    unsafe { std::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()) }
}

/// Copy a cropped window of one plane out of a mapped VAImage into a tight
/// owned [`Plane`]. Returns `None` when the source is too small.
fn copy_plane(
    data: &[u8],
    plane_offset: usize,
    pitch: usize,
    x_bytes: usize,
    y_rows: usize,
    row_bytes: usize,
    rows: usize,
) -> Option<Plane> {
    let mut out = Vec::with_capacity(row_bytes * rows);
    for row in 0..rows {
        let start = plane_offset
            .checked_add(pitch.checked_mul(y_rows + row)?)?
            .checked_add(x_bytes)?;
        out.extend_from_slice(data.get(start..start + row_bytes)?);
    }
    Some(Plane {
        data: out,
        stride: row_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_plane_crops_with_pitch() {
        // 4x2 plane with pitch 6, offset 1; crop the 2x2 window at (1, 0).
        let data = [
            0xEE, 0x10, 0x11, 0x12, 0x13, 0xAA, //
            0xEE, 0x20, 0x21, 0x22, 0x23, 0xAA,
        ];
        let plane = copy_plane(&data, 1, 6, 1, 0, 2, 2).expect("in bounds");
        assert_eq!(plane.data, vec![0x11, 0x12, 0x21, 0x22]);
        assert_eq!(plane.stride, 2);
        assert!(copy_plane(&data, 1, 6, 1, 1, 2, 2).is_none(), "row overrun");
    }

    #[test]
    fn as_bytes_matches_struct_size() {
        let attrib = sys::VAConfigAttrib { type_: 0, value: 7 };
        let bytes = as_bytes(&attrib);
        assert_eq!(bytes.len(), 8);
        assert_eq!(&bytes[4..8], &7u32.to_ne_bytes());
    }

    /// The probe must never panic — on machines without libva or a render
    /// node it reports empty capabilities.
    #[test]
    fn probe_is_infallible() {
        let caps = probe();
        let listed = available_codecs();
        assert_eq!(
            listed.contains(&HwCodec::Hevc),
            caps.hevc_main || caps.hevc_main10
        );
        assert_eq!(listed.contains(&HwCodec::Av1), caps.av1_profile0);
        assert_eq!(backend().is_some(), !listed.is_empty());
    }
}
