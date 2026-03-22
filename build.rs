fn main() {
    // Emit `any_raw` cfg alias when at least one RAW format feature is enabled.
    // This avoids copy-pasting the 7-format `cfg(any(...))` block everywhere.
    if ["arw", "cr2", "cr3", "crw", "dng", "nef", "raf"]
        .iter()
        .any(|f| std::env::var(format!("CARGO_FEATURE_{}", f.to_uppercase())).is_ok())
    {
        println!("cargo::rustc-cfg=any_raw");
    }
    // TODO: Generate large static tables from constant files
}
