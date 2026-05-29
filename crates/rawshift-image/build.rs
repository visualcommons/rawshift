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

    // libjxl encoder backend: link libjxl and generate its C-API bindings. Only
    // compiled when a `jxl-encode-libjxl*` feature is on; the build deps that make
    // it work (bindgen / pkg-config / jpegxl-src) are optional and pulled by those
    // features. All permissive (BSD-3) — no GPL `jpegxl-sys`/`jpegxl-rs`.
    #[cfg(feature = "jxl-encode-libjxl")]
    libjxl::generate();

    // TODO: Generate large static tables from constant files
}

/// Resolve libjxl, emit its `cargo:rustc-link-*` directives, and run `bindgen`
/// over its C API into `$OUT_DIR/jxl_bindings.rs` (included by
/// `src/codecs/jxl_libjxl.rs`).
#[cfg(feature = "jxl-encode-libjxl")]
mod libjxl {
    use std::env;
    use std::path::{Path, PathBuf};

    pub fn generate() {
        let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
        let include_dirs = link_and_include_dirs(&out_dir);

        // One translation unit pulling in the encoder API and the thread runner we
        // attach. Everything reachable is generated; we keep it scoped with allowlists.
        let mut builder = bindgen::Builder::default()
            // This crate is edition 2024, where `extern` blocks must be
            // `unsafe extern` — make bindgen emit edition-2024-correct code.
            .rust_edition(bindgen::RustEdition::Edition2024)
            .header_contents(
                "rawshift_jxl_wrapper.h",
                "#include <jxl/encode.h>\n#include <jxl/resizable_parallel_runner.h>\n",
            )
            .allowlist_function("Jxl(Encoder|ColorEncoding|ResizableParallelRunner).*")
            .allowlist_type("Jxl.*")
            .allowlist_var("JXL_.*")
            // C enums as integer type-aliases + consts: simplest to compare against
            // and lets the config's raw escape-hatch pass arbitrary frame-setting ids.
            // `prepend_enum_name(false)` keeps the C names (`JXL_ENC_SUCCESS`) rather
            // than prefixing with the enum type — libjxl's constants are already unique.
            .default_enum_style(bindgen::EnumVariation::Consts)
            .prepend_enum_name(false)
            .layout_tests(false)
            .generate_comments(false)
            .merge_extern_blocks(true);
        for dir in &include_dirs {
            builder = builder.clang_arg(format!("-I{}", dir.display()));
        }

        let bindings = builder
            .generate()
            .expect("bindgen failed to generate libjxl bindings");
        bindings
            .write_to_file(out_dir.join("jxl_bindings.rs"))
            .expect("failed to write jxl_bindings.rs");

        println!("cargo:rerun-if-changed=build.rs");
    }

    /// Emit the link directives for libjxl and return the header include dirs to
    /// hand to bindgen. Vendored builds libjxl from source (BSD-3 `jpegxl-src`,
    /// which also emits every static link directive); otherwise a system libjxl is
    /// resolved via pkg-config.
    #[cfg(feature = "jxl-encode-libjxl-vendored")]
    fn link_and_include_dirs(out_dir: &Path) -> Vec<PathBuf> {
        // `jpegxl_src::build()` runs cmake, installs headers into `$OUT_DIR/include`,
        // and prints `cargo:rustc-link-{search,lib}` for jxl + jxl_threads + its
        // bundled deps (hwy, brotli*, jxl_cms) and the C++ runtime.
        jpegxl_src::build();
        vec![out_dir.join("include")]
    }

    #[cfg(not(feature = "jxl-encode-libjxl-vendored"))]
    fn link_and_include_dirs(_out_dir: &Path) -> Vec<PathBuf> {
        let probe = |lib: &str| {
            pkg_config::Config::new()
                .atleast_version("0.10")
                .probe(lib)
                .unwrap_or_else(|e| {
                    panic!(
                        "could not find system `{lib}` via pkg-config ({e}); install \
                         libjxl development files or enable the \
                         `jxl-encode-libjxl-vendored` feature to build it from source"
                    )
                })
        };
        let mut dirs = probe("libjxl").include_paths;
        dirs.extend(probe("libjxl_threads").include_paths);
        dirs
    }
}
