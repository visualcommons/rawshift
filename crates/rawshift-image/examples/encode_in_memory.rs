//! Demonstrates the in-memory encode API, codec enumeration and the header
//! probe — all without touching the filesystem.
//!
//! Run with: `cargo run -p rawshift-image --example encode_in_memory`

use rawshift_image::prelude::{
    EncodeOptions, ImageMetadata, RgbImage, available_encoders, encode_rgb_image_to_vec,
    probe_standard_image,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build a small synthetic gradient (16-bit RGB).
    let (width, height) = (64u32, 64u32);
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let r = (x * 65535 / width) as u16;
            let g = (y * 65535 / height) as u16;
            data.extend_from_slice(&[r, g, 32768]);
        }
    }
    let image = RgbImage::new(width, height, data).expect("valid RGB buffer");
    let metadata = ImageMetadata::default();

    println!("Encoders compiled into this build:");
    for codec in available_encoders() {
        println!("  {:<20} v{}", codec.id, codec.version);
    }
    println!();

    // Encode straight to a `Vec<u8>` with each default encoder, then probe the
    // result's header — no file path is ever needed.
    // `mut` is used only when an opt-in backend is compiled in.
    #[allow(unused_mut)]
    let mut options = vec![
        EncodeOptions::png(),
        EncodeOptions::jpeg(),
        EncodeOptions::webp_lossy(),
    ];
    // The gamut-jxl (libjxl) backend encodes 16-bit by default (unlike the
    // others, which are 8-bit). Only present when built with `jxl-encode`.
    #[cfg(feature = "jxl-encode")]
    options.push(EncodeOptions::jxl());

    for opts in options {
        let bytes = encode_rgb_image_to_vec(&image, &metadata, &opts)?;
        let probe = probe_standard_image(&bytes)?;
        println!(
            "{:<8} {:>8} bytes  via {:<18}  probe -> {:?} {}x{}",
            opts.format().name(),
            bytes.len(),
            opts.codec_id().to_string(),
            probe.format,
            probe.size.width,
            probe.size.height,
        );
    }

    Ok(())
}
