//! Backend selection for the verified hardware-decode feature flags.
//!
//! The portable `hw` meta-flag selects the **native** backend for the compile
//! target (VideoToolbox on Apple, VAAPI on linux-gnu, MediaCodec on Android —
//! the permanent matrix in `docs/SUPPORT.md`). On a target with no hardware
//! decode API it emits a build warning and the crate compiles the no-backend
//! stub. The explicit `videotoolbox`/`vaapi`/`mediacodec` flags are validated
//! separately by hard `compile_error!` gates in `src/lib.rs`; here they only
//! contribute to the selected-backend cfg.
//!
//! The selected backend is exposed to the source as
//! `cfg(hwdec_backend = "...")`; the platform backend modules (separate
//! issues) compile behind it. This build script contains no platform code.

use std::env;

fn main() {
    // Declare the custom cfg so `-D warnings` builds accept it.
    println!(
        "cargo::rustc-check-cfg=cfg(hwdec_backend, values(\"videotoolbox\", \"vaapi\", \"mediacodec\"))"
    );

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    // The native backend for this target, per the permanent matrix in
    // docs/SUPPORT.md. Android must be matched before the generic linux arm:
    // its target_os is "android", not "linux", so the arms are disjoint.
    let native = match (target_os.as_str(), target_env.as_str()) {
        ("macos" | "ios", _) => Some("videotoolbox"),
        ("android", _) => Some("mediacodec"),
        ("linux", "gnu") => Some("vaapi"),
        _ => None,
    };

    let mut selected: Option<&str> = None;
    let mut select = |backend: &'static str| {
        // Explicit flags are already target-verified by the compile_error!
        // gates in lib.rs; the first selection wins and duplicates (e.g. `hw`
        // + the matching explicit flag) collapse to one cfg.
        if selected.is_none() {
            selected = Some(backend);
        }
    };

    if env::var("CARGO_FEATURE_VIDEOTOOLBOX").is_ok() {
        select("videotoolbox");
    }
    if env::var("CARGO_FEATURE_VAAPI").is_ok() {
        select("vaapi");
    }
    if env::var("CARGO_FEATURE_MEDIACODEC").is_ok() {
        select("mediacodec");
    }
    if env::var("CARGO_FEATURE_HW").is_ok() {
        match native {
            Some(backend) => select(backend),
            None => println!(
                "cargo::warning=rawshift-hwdec: the `hw` feature is enabled but the target \
                 ({target_os}/{target_env}) has no hardware decode API in docs/SUPPORT.md; \
                 compiling the no-backend stub — HEIC/AVIF pixel decode will report \
                 `HwDecodeError::Unavailable`"
            ),
        }
    }

    if let Some(backend) = selected {
        println!("cargo::rustc-cfg=hwdec_backend=\"{backend}\"");
    }
}
