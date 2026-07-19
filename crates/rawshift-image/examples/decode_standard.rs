//! Standard image format decode example.
//!
//! Demonstrates [`detect_standard_format`] and [`decode_standard_image`] for
//! JPEG, PNG, WebP, JXL, GIF, TIFF, and other supported formats.
//!
//! Usage:
//!   cargo run --example decode_standard -- <input.jpg>
//!   cargo run --example decode_standard -- <input.png> --save-raw output.raw

use clap::Parser;
use rawshift_image::formats::{
    DecodeOptions, decode_standard_image, decode_standard_image_with, detect_standard_format,
};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Detect and decode a standard image format, printing its dimensions"
)]
struct Args {
    /// Input image file (JPEG, PNG, WebP, JXL, GIF, TIFF, AVIF, ...)
    input: PathBuf,

    /// Optional: save decoded raw pixel data (interleaved RGB u8) to this path
    #[arg(long)]
    save_raw: Option<PathBuf>,

    /// Decode through the explicit-backend API (`decode_standard_image_with`)
    /// using the default implementation for the detected format, instead of
    /// the convenience `decode_standard_image` entry point.
    #[arg(long)]
    explicit_backend: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Args {
        input,
        save_raw,
        explicit_backend,
    } = Args::parse();

    let data = std::fs::read(&input)?;

    // Detect format from magic bytes
    let format = match detect_standard_format(&data) {
        Some(f) => f,
        None => {
            eprintln!(
                "Could not detect a supported standard image format in {:?}",
                input
            );
            std::process::exit(1);
        }
    };
    println!("Detected format: {}", format);

    // Decode to RGB. With --explicit-backend, pin the decoder implementation
    // explicitly via DecodeOptions (here: each format's default backend).
    let image = if explicit_backend {
        let backend = DecodeOptions::default_for(format)
            .ok_or_else(|| format!("no decoder backend compiled in for {format}"))?;
        println!("Backend: {backend:?}");
        decode_standard_image_with(&data, &backend)?
    } else {
        decode_standard_image(&data, format)?
    };
    println!(
        "Dimensions: {}x{} ({} pixels)",
        image.width(),
        image.height(),
        image.width() as u64 * image.height() as u64
    );
    println!("Pixel data length: {} u16 values", image.data().len());

    if let Some(out_path) = save_raw {
        // Convert u16 to u8 bytes (little-endian) and write
        let bytes: Vec<u8> = image.data().iter().flat_map(|&v| v.to_le_bytes()).collect();
        std::fs::write(&out_path, &bytes)?;
        println!(
            "Saved {} bytes of raw pixel data to {:?}",
            bytes.len(),
            out_path
        );
    }

    Ok(())
}
