//! # rawshift gallery
//!
//! A cross-platform GUI that demonstrates rawshift's decode and encode paths
//! end to end, including hardware-accelerated HEIC/AVIF decode.
//!
//! Pick images from the filesystem; each one is decoded, re-encoded through
//! every selected output format with every parameter exposed, written to the
//! system temp directory, and decoded back. The results are shown side by side
//! per source, with a numeric comparison against the original — or, where a
//! stage failed, the error text inside an empty placeholder.
//!
//! Usage:
//!
//! ```text
//! cargo run --release                       # the window
//! cargo run --release -- --headless a.jpg   # the same matrix, no window
//! cargo run --release -- --headless --all-formats a.jpg
//! ```
//!
//! Run from `examples/gallery`: this is a standalone package, outside the
//! rawshift workspace. See its `Cargo.toml` for why.
//!
//! ## Layout
//!
//! - [`pipeline`] — decode, encode, round trip, compare. No `iced` types.
//! - [`color`] — container colour resolution and the display transform.
//! - [`settings`] — the encode-settings model.
//! - [`render`] — decoded image to texture.
//! - [`headless`] — the same matrix with no window.
//! - [`app`] — the window.

mod app;
mod color;
mod headless;
mod pipeline;
mod render;
mod settings;

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Images to load at startup. Required with `--headless`.
    inputs: Vec<PathBuf>,

    /// Run the round-trip matrix and print a table instead of opening a
    /// window. Exits non-zero if any round trip failed.
    #[arg(long)]
    headless: bool,

    /// Enable every output format rather than the default PNG + JPEG pair.
    #[arg(long)]
    all_formats: bool,
}

/// One line describing what this build can decode on this machine.
///
/// Shown in the window header and at the top of the headless report, because
/// "the AVIF column is empty" means something entirely different depending on
/// whether a hardware decoder exists.
pub fn backend_banner() -> String {
    let backend = rawshift_hwdec::backend()
        .map_or_else(|| "none".to_owned(), |backend| backend.name().to_owned());
    let codecs = rawshift_hwdec::available_codecs();
    let codecs = if codecs.is_empty() {
        "no codecs".to_owned()
    } else {
        codecs
            .iter()
            .map(|c| c.name())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let decoders = rawshift_image::formats::available_decoders().len();
    let encoders = rawshift_image::formats::available_encoders().len();
    format!(
        "hardware backend: {backend} ({codecs})  ·  {decoders} decoders, {encoders} encoders \
         compiled in"
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    if args.headless {
        if args.inputs.is_empty() {
            eprintln!("--headless needs at least one input path");
            std::process::exit(2);
        }
        let mut encode = settings::EncodeSettings::default();
        if args.all_formats {
            for format in &mut encode.formats {
                format.enabled = true;
            }
        }
        let failures = headless::run(&args.inputs, &encode, pipeline::RawSettings::default());
        if failures > 0 {
            eprintln!("{failures} round trip(s) failed");
            std::process::exit(1);
        }
        return Ok(());
    }

    app::run(args.inputs)?;
    Ok(())
}
