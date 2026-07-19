/// Emit a `cargo::rustc-cfg` alias when at least one of `features` is enabled.
///
/// Lets the source use one `#[cfg(alias)]` instead of repeating a long
/// `cfg(any(feature = "…", …))` block. Every alias emitted here must also be
/// listed in the `[lints.rust] unexpected_cfgs` `check-cfg` entry in Cargo.toml.
fn emit_cfg_alias(alias: &str, features: &[&str]) {
    let any_enabled = features.iter().any(|f| {
        let var = format!("CARGO_FEATURE_{}", f.to_uppercase().replace('-', "_"));
        std::env::var(var).is_ok()
    });
    if any_enabled {
        println!("cargo::rustc-cfg={alias}");
    }
}

fn main() {
    // `any_raw` — at least one of the 7 RAW format decoders is compiled in.
    emit_cfg_alias(
        "any_raw",
        &[
            "arw-decode",
            "cr2-decode",
            "cr3-decode",
            "crw-decode",
            "dng-decode",
            "nef-decode",
            "raf-decode",
        ],
    );
    // `any_standard_decode` — at least one standard (non-RAW) decoder is on.
    emit_cfg_alias(
        "any_standard_decode",
        &[
            "gif-decode",
            "jpeg-decode",
            "png-decode",
            "webp-decode",
            "jxl-decode",
            "tiff-decode",
            "avif-decode",
            "heic-decode",
            "svg-decode",
            "ppm-decode",
        ],
    );
    // `any_standard_encode` — at least one standard (non-RAW) encoder is on.
    emit_cfg_alias(
        "any_standard_encode",
        &[
            "png-encode",
            "jpeg-encode",
            "webp-encode",
            "avif-encode",
            "jxl-encode",
            "dng-encode",
        ],
    );

    // TODO: Generate large static tables from constant files
}
