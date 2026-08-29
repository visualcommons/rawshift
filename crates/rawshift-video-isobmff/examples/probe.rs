//! Print what the ISOBMFF demuxer sees in a file.
//!
//! `cargo run -p rawshift-video-isobmff --example probe -- FILE`

use std::fs::File;

use rawshift_video_core::{Demuxer, Track};
use rawshift_video_isobmff::IsoBmffDemuxer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: probe FILE")?;
    let mut demuxer = IsoBmffDemuxer::open(File::open(&path)?)?;

    println!("container: {}", demuxer.container());
    let md = demuxer.metadata();
    println!("brands:    {:?}", md.container.brands);
    println!("timescale: {:?}", md.container.timescale);
    println!("duration:  {:?}", md.container.duration);
    println!("created:   {:?}", md.container.creation_time);

    for track in demuxer.tracks() {
        match track {
            Track::Video(v) => println!(
                "video  #{} {} {}x{} rot={} tb={}/{} frames={:?} dur={:?} cfg={}B",
                v.id,
                v.codec,
                v.dimensions.width,
                v.dimensions.height,
                v.rotation,
                v.time_base.numerator,
                v.time_base.denominator,
                v.frame_count,
                v.duration,
                v.codec_config.as_bytes().len(),
            ),
            Track::Audio(a) => println!("audio  #{} {}", a.id, a.codec),
            Track::Other(o) => println!("other  #{} {:?}", o.id, o.kind),
            other => println!("other  #{}", other.id()),
        }
    }

    let (mut packets, mut keyframes, mut first_pts) = (0usize, 0usize, None);
    while let Some(p) = demuxer.next_packet()? {
        if first_pts.is_none() {
            first_pts = p.pts;
        }
        packets += 1;
        if p.is_keyframe {
            keyframes += 1;
        }
    }
    println!("packets:   {packets} ({keyframes} random access points), first pts {first_pts:?}");
    Ok(())
}
