fn main() {
    // Emit `any_raw` cfg alias when at least one RAW format feature is enabled.
    // This avoids copy-pasting the 7-format `cfg(any(...))` block everywhere.
    if [
        "arw-decode",
        "cr2-decode",
        "cr3-decode",
        "crw-decode",
        "dng-decode",
        "nef-decode",
        "raf-decode",
    ]
    .iter()
    .any(|f| {
        std::env::var(format!(
            "CARGO_FEATURE_{}",
            f.to_uppercase().replace('-', "_")
        ))
        .is_ok()
    }) {
        println!("cargo::rustc-cfg=any_raw");
    }
    // TODO: Generate large static tables from constant files
}
