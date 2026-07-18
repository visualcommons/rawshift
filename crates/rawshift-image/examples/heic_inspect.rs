//! HEIC inspection example.
//!
//! Enumerates auxiliary images (thumbnails, depth maps, HDR gain maps,
//! alpha/auxiliary) and dumps all extracted metadata — all of which works
//! with **no hardware decoder** — then attempts pixel decode, which needs a
//! hardware HEVC backend (`hw` feature + a usable platform decoder) and
//! otherwise reports `HwDecoderUnavailable`.
//!
//! Usage:
//!   cargo run --example heic_inspect --features heic -- <input.heic>

use rawshift_image::formats::{HeicFile, heic_hw_decode_available};

fn human_bytes(blob: Option<&Vec<u8>>) -> String {
    match blob {
        Some(b) => format!("{} bytes", b.len()),
        None => "none".to_string(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .expect("Usage: heic_inspect <input.heic>");
    let data = std::fs::read(&path)?;

    let file = HeicFile::open(data)?;

    // ── Auxiliary images (backend-less enumeration) ──────────────────────────
    let aux = file.aux_images();
    println!("Auxiliary images: {}", aux.len());
    for (i, a) in aux.iter().enumerate() {
        print!("  [{i}] {:?}  {}x{}", a.kind, a.width, a.height);
        if let Some(t) = &a.aux_type {
            print!("  type={t}");
        }
        println!();
    }

    // ── Metadata (backend-less) ──────────────────────────────────────────────
    let md = file.metadata();
    println!("\nMetadata:");
    println!("  camera:        {} {}", md.camera.make, md.camera.model);
    println!("  ISO:           {:?}", md.exif.iso);
    println!("  datetime:      {:?}", md.datetime.datetime_original);
    println!("  orientation:   {:?}", md.image.orientation);
    println!("  bit depth:     {}", md.image.bit_depth);
    println!("  ICC profile:   {}", human_bytes(md.icc_profile.as_ref()));
    println!("  XMP:           {}", human_bytes(md.xmp.as_ref()));
    println!("  EXIF (raw):    {}", human_bytes(md.exif_raw.as_ref()));

    println!("\n  generic tag table ({} entries):", md.extra.len());
    for entry in &md.extra {
        println!(
            "    {:?}:{} = {:?}",
            entry.key.namespace, entry.key.tag, entry.value
        );
    }

    // ── Pixel decode (needs a hardware HEVC decoder) ─────────────────────────
    println!(
        "\nHardware HEVC decode available: {}",
        heic_hw_decode_available()
    );
    match file.decode_primary() {
        Ok(primary) => println!("Primary image: {}x{}", primary.width(), primary.height()),
        Err(e) => println!("Primary decode unavailable: {e}"),
    }
    for (i, a) in aux.iter().enumerate() {
        match file.decode_aux(a) {
            Ok(img) => println!("  aux [{i}] decoded {}x{}", img.width(), img.height()),
            Err(e) => println!("  aux [{i}] decode unavailable: {e}"),
        }
    }

    Ok(())
}
