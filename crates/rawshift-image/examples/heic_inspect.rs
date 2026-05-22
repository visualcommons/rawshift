//! HEIC inspection example.
//!
//! Decodes the primary image, enumerates auxiliary images (thumbnails, depth
//! maps, HDR gain maps, alpha/auxiliary), and dumps all extracted metadata —
//! demonstrating the full [`HeicFile`](rawshift_image::formats::HeicFile) API.
//!
//! Usage:
//!   cargo run --example heic_inspect --features heic -- <input.heic>

use rawshift_image::formats::HeicFile;

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

    // ── Primary image ────────────────────────────────────────────────────────
    let primary = file.decode_primary()?;
    println!("Primary image: {}x{}", primary.width(), primary.height());

    // ── Auxiliary images ─────────────────────────────────────────────────────
    let aux = file.aux_images();
    println!("\nAuxiliary images: {}", aux.len());
    for (i, a) in aux.iter().enumerate() {
        print!("  [{i}] {:?}  {}x{}", a.kind, a.width, a.height);
        if let Some(t) = &a.aux_type {
            print!("  type={t}");
        }
        match file.decode_aux(a) {
            Ok(img) => println!("  -> decoded {}x{}", img.width(), img.height()),
            Err(e) => println!("  -> decode failed: {e}"),
        }
    }

    // ── Metadata ─────────────────────────────────────────────────────────────
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

    Ok(())
}
