//! End-to-end RAW development pipeline example.
//!
//! Decodes a RAW file, applies demosaicing, white balance, color matrix,
//! and saves the result as a JPEG.
//!
//! Usage: cargo run --example develop_image -- <input.arw> <output.jpg>

use clap::Parser;
use rawshift::formats::RawFile;
use rawshift::formats::export::EncodeOptions;
use rawshift::processing::{BayerAlgorithm, DemosaicMethod, ProcessingOptions};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Develop a RAW image through the full pipeline and save the result"
)]
struct Args {
    /// Input RAW file (ARW, CR2, CR3, CRW, DNG, NEF, RAF)
    input: PathBuf,

    /// Output image path (extension determines format: png, jpg, webp, dng)
    output: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Args { input, output } = Args::parse();

    println!("Opening {:?}", input);
    let file = File::open(&input)?;
    let reader = BufReader::new(file);
    let mut raw_file = RawFile::open(reader)?;

    let meta = raw_file.metadata();
    println!("Format detected. Bit depth: {}", meta.image.bit_depth);

    // Full pipeline: demosaic -> white balance -> color matrix -> gamma
    let options = ProcessingOptions::new()
        .demosaic(DemosaicMethod::Bayer(BayerAlgorithm::Bilinear))
        .white_balance(2.0, 1.0, 1.5)
        .color_matrix([1.6, -0.4, -0.2, -0.2, 1.4, -0.2, -0.1, -0.3, 1.4])
        .gamma(2.2);

    let encode_options = match output.extension().and_then(|e| e.to_str()) {
        Some("png") => EncodeOptions::png(),
        Some("jpg") | Some("jpeg") => EncodeOptions::jpeg(),
        Some("webp") => EncodeOptions::webp_lossy(),
        Some("dng") => EncodeOptions::dng(),
        _ => {
            eprintln!("Unsupported output format. Defaulting to JPEG.");
            EncodeOptions::jpeg()
        }
    };

    println!("Exporting to {:?}", output);
    raw_file.export(&output, &options, &encode_options)?;

    println!("Done!");
    Ok(())
}
