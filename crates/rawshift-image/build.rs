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

    // jpegli encoder backend: build the vendored google/jpegli static lib (or
    // probe a system libjpegli), compile our C++ setjmp shim, and bindgen the
    // shim's C ABI. Only compiled when a `jpeg-encode-jpegli*` feature is on; its
    // build deps (bindgen / cc / cmake / pkg-config) are optional and pulled by
    // those features. All permissive (BSD-3 / MIT-Apache) — no GPL.
    #[cfg(feature = "jpeg-encode-jpegli")]
    jpegli::generate();

    // libaom AVIF encoder backend: link libaom and generate its C-API bindings.
    // Only compiled when an `avif-encode-libaom*` feature is on. The bindgen /
    // pkg-config build deps are shared with the libjxl backend; the vendored
    // build of libaom itself comes from the BSD-2 `libaom-sys` crate.
    #[cfg(feature = "avif-encode-libaom")]
    aom::generate();

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

/// Build/resolve jpegli, compile the C++ setjmp shim against it, and run
/// `bindgen` over the shim's self-contained C header into
/// `$OUT_DIR/jpegli_bindings.rs` (included by `src/codecs/jpegli.rs`).
///
/// jpegli's encode API is libjpeg-style (errors via `error_exit`/`setjmp`), so
/// unlike libjxl's return-code C API the whole compress sequence lives in
/// `src/codecs/jpegli_shim.cc`; here we just compile and link it.
#[cfg(feature = "jpeg-encode-jpegli")]
mod jpegli {
    use std::env;
    use std::path::PathBuf;

    /// Resolved native jpegli: header dirs for the shim + link directives to
    /// emit *after* the shim archive (so link order is shim → jpegli → hwy → c++).
    struct Native {
        include_dirs: Vec<PathBuf>,
        vendored: bool,
        link_search: Vec<PathBuf>,
        link_libs: Vec<String>,
    }

    pub fn generate() {
        let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
        let native = resolve();

        // Compile the shim FIRST: GNU ld resolves static archives left-to-right,
        // so the shim (which references `jpegli_*`) must precede `jpegli-static`.
        let mut build = cc::Build::new();
        build
            .cpp(true)
            .std("c++17")
            .file("src/codecs/jpegli_shim.cc")
            .include("src/codecs");
        if native.vendored {
            build.define("RAWSHIFT_JPEGLI_VENDORED", None);
        }
        for dir in &native.include_dirs {
            build.include(dir);
        }
        build.compile("rawshift_jpegli_shim");

        // Then jpegli + Highway + the C++ runtime.
        for dir in &native.link_search {
            println!("cargo:rustc-link-search=native={}", dir.display());
        }
        for lib in &native.link_libs {
            println!("cargo:rustc-link-lib={lib}");
        }
        link_cpp_runtime();

        // bindgen over the self-contained shim header (no jpegli includes needed).
        let bindings = bindgen::Builder::default()
            .rust_edition(bindgen::RustEdition::Edition2024)
            .header("src/codecs/jpegli_shim.h")
            .allowlist_function("rawshift_jpegli_.*")
            .allowlist_type("RawshiftJpegli.*")
            .allowlist_var("RAWSHIFT_JPEGLI_.*")
            .default_enum_style(bindgen::EnumVariation::Consts)
            .prepend_enum_name(false)
            .layout_tests(false)
            .generate_comments(false)
            .merge_extern_blocks(true)
            .generate()
            .expect("bindgen failed to generate jpegli shim bindings");
        bindings
            .write_to_file(out_dir.join("jpegli_bindings.rs"))
            .expect("failed to write jpegli_bindings.rs");

        println!("cargo:rerun-if-changed=src/codecs/jpegli_shim.cc");
        println!("cargo:rerun-if-changed=src/codecs/jpegli_shim.h");
    }

    /// Vendored: build only the static jpegli library (+ its Highway dep) from
    /// the `google/jpegli` submodule; everything else (tools, tests, CMS,
    /// png/zlib helpers, the libjpeg-compatible shared lib) is disabled.
    #[cfg(feature = "jpeg-encode-jpegli-vendored")]
    fn resolve() -> Native {
        let src = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
            .join("third_party/jpegli");
        assert!(
            src.join("lib/jpegli/encode.h").exists(),
            "jpegli submodule not checked out at {}; run \
             `git submodule update --init --recursive`",
            src.display()
        );

        let dst = cmake::Config::new(&src)
            .define("BUILD_SHARED_LIBS", "OFF")
            .define("BUILD_TESTING", "OFF")
            .define("JPEGLI_ENABLE_TOOLS", "OFF")
            .define("JPEGLI_ENABLE_DEVTOOLS", "OFF")
            .define("JPEGLI_ENABLE_FUZZERS", "OFF")
            .define("JPEGLI_ENABLE_DOXYGEN", "OFF")
            .define("JPEGLI_ENABLE_MANPAGES", "OFF")
            .define("JPEGLI_ENABLE_BENCHMARK", "OFF")
            .define("JPEGLI_ENABLE_JNI", "OFF")
            .define("JPEGLI_ENABLE_SJPEG", "OFF")
            .define("JPEGLI_ENABLE_OPENEXR", "OFF")
            .define("JPEGLI_ENABLE_JPEGLI_LIBJPEG", "OFF")
            .define("JPEGLI_FORCE_SYSTEM_HWY", "OFF")
            .build_target("jpegli-static")
            .build();

        // `build_target` skips install, so artifacts stay in the build tree.
        let build_dir = dst.join("build");
        let jpegli_lib = build_dir.join("lib");
        assert!(
            jpegli_lib.join("libjpegli-static.a").exists(),
            "libjpegli-static.a not found under {}",
            jpegli_lib.display()
        );
        Native {
            // Source root resolves `lib/...`-prefixed internal includes; the
            // build dir holds the generated libjpeg-compatible jpeglib.h/jconfig.h.
            include_dirs: vec![src.clone(), build_dir.join("lib/include/jpegli")],
            vendored: true,
            link_search: vec![jpegli_lib, build_dir.join("third_party/highway")],
            link_libs: vec!["static=jpegli-static".into(), "static=hwy".into()],
        }
    }

    /// System: resolve libjpegli via pkg-config (rarely packaged). Metadata
    /// emission is suppressed so link directives can be ordered after the shim.
    #[cfg(not(feature = "jpeg-encode-jpegli-vendored"))]
    fn resolve() -> Native {
        let lib = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe("libjpegli")
            .unwrap_or_else(|e| {
                panic!(
                    "could not find system `libjpegli` via pkg-config ({e}); install \
                     libjpegli development files or enable the \
                     `jpeg-encode-jpegli-vendored` feature to build it from source"
                )
            });
        Native {
            include_dirs: lib.include_paths,
            vendored: false,
            link_search: lib.link_paths,
            link_libs: lib.libs,
        }
    }

    fn link_cpp_runtime() {
        if cfg!(any(target_vendor = "apple", target_os = "freebsd")) {
            println!("cargo:rustc-link-lib=c++");
        } else if cfg!(target_os = "linux") {
            println!("cargo:rustc-link-lib=stdc++");
        }
    }
}

/// Resolve libaom, ensure its link directives are emitted, and run `bindgen` over
/// its C encoder API into `$OUT_DIR/aom_bindings.rs` (included by
/// `src/codecs/avif_libaom.rs`).
///
/// libaom only emits a raw AV1 bitstream; the `avif-serialize` crate muxes that
/// into the AVIF container. We bind libaom ourselves (no GPL wrapper crates).
#[cfg(feature = "avif-encode-libaom")]
mod aom {
    use std::env;
    use std::path::PathBuf;

    pub fn generate() {
        let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
        let include_dirs = link_and_include_dirs();

        // `aomcx.h` is the AV1 encoder control interface; it pulls in `aom.h`,
        // `aom_encoder.h`, and `aom_image.h`. Allowlists keep the output scoped.
        let mut builder = bindgen::Builder::default()
            // This crate is edition 2024, where `extern` blocks must be
            // `unsafe extern` — make bindgen emit edition-2024-correct code.
            .rust_edition(bindgen::RustEdition::Edition2024)
            .header_contents(
                "rawshift_aom_wrapper.h",
                "#include <aom/aomcx.h>\n#include <aom/aom_encoder.h>\n#include <aom/aom_image.h>\n",
            )
            .allowlist_function("aom_codec_.*")
            .allowlist_function("aom_img_.*")
            .allowlist_type("aom_.*")
            .allowlist_var("AOM_.*")
            .allowlist_var("AV1E_.*")
            .allowlist_var("AOME_.*")
            // C enums as integer type-aliases + consts: simplest to compare against
            // and to pass to the variadic `aom_codec_control`. `prepend_enum_name(false)`
            // keeps the original C names (`AOM_CODEC_OK`, `AV1E_SET_CQ_LEVEL`).
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
            .expect("bindgen failed to generate libaom bindings");
        bindings
            .write_to_file(out_dir.join("aom_bindings.rs"))
            .expect("failed to write aom_bindings.rs");

        println!("cargo:rerun-if-changed=build.rs");
    }

    /// Return the header include dirs for bindgen. The vendored build is performed
    /// by `libaom-sys` (BSD-2), which cmake-builds the bundled source, links it
    /// statically, and exposes the installed headers via `DEP_AOM_INCLUDE`;
    /// otherwise a system libaom is resolved via pkg-config.
    #[cfg(feature = "avif-encode-libaom-vendored")]
    fn link_and_include_dirs() -> Vec<PathBuf> {
        // `libaom-sys`'s build script (it declares `links = "aom"`) already emitted
        // the `cargo:rustc-link-{search,lib}` directives and a `cargo:include` that
        // cargo forwards to us as `DEP_AOM_INCLUDE`. The headers live under
        // `<include>/aom/`, so bindgen's `<aom/aomcx.h>` resolves with `-I<include>`.
        let include = env::var("DEP_AOM_INCLUDE")
            .expect("DEP_AOM_INCLUDE is set by the libaom-sys build script");
        vec![PathBuf::from(include)]
    }

    #[cfg(not(feature = "avif-encode-libaom-vendored"))]
    fn link_and_include_dirs() -> Vec<PathBuf> {
        pkg_config::Config::new()
            .probe("aom")
            .unwrap_or_else(|e| {
                panic!(
                    "could not find system `aom` via pkg-config ({e}); install \
                     libaom development files (e.g. `libaom-dev` / `libaom-devel`) \
                     or enable the `avif-encode-libaom-vendored` feature to build it \
                     from source"
                )
            })
            .include_paths
    }
}
