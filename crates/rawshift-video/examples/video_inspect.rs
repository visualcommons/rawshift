//! Inspect a video file and decode its first frame, through the public API.
//!
//! ```text
//! cargo run -p rawshift-video --features full --example video_inspect -- FILE
//! ```
//!
//! Everything except the final decode works on every supported target with no
//! hardware decoder at all; the decode step reports why when none is
//! available rather than failing the run.

use std::fs::File;

use rawshift_video::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: video_inspect FILE")?;

    let mut video = VideoFile::open(File::open(&path)?)?;

    println!("container : {}", video.container());
    let md = video.metadata();
    if !md.container.brands.is_empty() {
        println!("brands    : {}", md.container.brands.join(", "));
    }
    println!("duration  : {:?}", md.container.duration);
    if let Some(created) = &md.container.creation_time {
        println!("created   : {created}");
    }

    println!("\ntracks");
    for track in video.tracks() {
        match track {
            Track::Video(v) => println!(
                "  #{} video  {} {}x{} rotation {} {:?} frames, config {} bytes",
                v.id,
                v.codec,
                v.dimensions.width,
                v.dimensions.height,
                v.rotation,
                v.frame_count,
                v.codec_config.as_bytes().len(),
            ),
            Track::Audio(a) => println!(
                "  #{} audio  {} {:?} Hz, {:?} channels",
                a.id, a.codec, a.sample_rate, a.channels
            ),
            other => println!("  #{} {:?}", other.id(), other.kind()),
        }
    }

    println!("\ndecoders compiled in");
    for codec in available_decoders() {
        println!("  {} ({})", codec.id, codec.version);
    }

    let Some(track) = video.tracks().iter().find_map(Track::as_video) else {
        println!("\nno video track to decode");
        return Ok(());
    };
    println!(
        "\n{} decode available here: {}",
        track.codec,
        hw_decode_available(track.codec)
    );

    match video.thumbnail() {
        Ok(Some(frame)) => println!(
            "first frame: {}x{} {:?} {}-bit, pts {:?}",
            frame.width(),
            frame.height(),
            frame.format(),
            frame.bit_depth(),
            frame.pts(),
        ),
        Ok(None) => println!("first frame: none decodable"),
        // A missing backend is the expected outcome on most targets, and is
        // reported rather than treated as a failure of the file.
        Err(e) if e.is_capability_gap() => println!("first frame: unavailable — {e}"),
        Err(e) => return Err(e.into()),
    }

    Ok(())
}
