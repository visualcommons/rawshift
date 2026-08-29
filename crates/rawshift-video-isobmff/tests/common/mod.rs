//! Shared fixture generation for the container integration tests.
//!
//! Fixtures are generated with `ffmpeg` at test time rather than committed,
//! following `TEST_FIXTURES.md`: generated inputs stay out of the repository,
//! and **tests skip gracefully when the tooling is absent**, so `cargo test`
//! passes on a machine with no ffmpeg.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Whether ffmpeg is available to generate fixtures.
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Where generated fixtures live: a temp directory, never the repository.
fn fixture_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("rawshift-video-fixtures");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Generate a test clip, returning its path.
///
/// Cached across runs: regenerating clips for every test would dominate the
/// suite's runtime. Returns `None` when ffmpeg is missing or lacks the
/// encoder, which callers treat as a skip — the tests assert rawshift's
/// behaviour, not the machine's codec inventory.
pub fn clip(name: &str, args: &[&str]) -> Option<PathBuf> {
    if !ffmpeg_available() {
        return None;
    }
    let path = fixture_dir().join(name);
    if path.exists() {
        return Some(path);
    }

    // Generate to a unique name and rename into place. Tests run in parallel
    // and share this cache; writing directly to `path` would make it exist —
    // and so look ready — while ffmpeg is still filling it, and the other
    // tests would open a half-written file.
    let staging = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));

    let ok = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error"])
        .args(args)
        .arg(&staging)
        .status()
        .is_ok_and(|s| s.success());

    if !ok || !staging.exists() {
        let _ = std::fs::remove_file(&staging);
        return None;
    }
    // Rename is atomic within a filesystem, so `path` goes from absent to
    // complete with nothing observable in between. A concurrent generator
    // renaming its own copy over ours is harmless: both are complete.
    if std::fs::rename(&staging, &path).is_err() {
        let _ = std::fs::remove_file(&staging);
        return None;
    }
    Some(path)
}

/// A short H.264 clip with B-frames and a 12-frame GOP.
pub fn h264_mp4() -> Option<PathBuf> {
    clip(
        "h264.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=128x96:duration=2:rate=25",
            "-c:v",
            "libx264",
            "-bf",
            "3",
            "-g",
            "12",
            "-profile:v",
            "high",
            "-pix_fmt",
            "yuv420p",
        ],
    )
}

/// The same content as HEVC — the case that motivated owning the sample index.
pub fn hevc_mp4() -> Option<PathBuf> {
    clip(
        "hevc.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=128x96:duration=2:rate=25",
            "-c:v",
            "libx265",
            "-x265-params",
            "log-level=error",
            "-g",
            "12",
            "-pix_fmt",
            "yuv420p",
        ],
    )
}

/// H.264 in a QuickTime container.
pub fn h264_mov() -> Option<PathBuf> {
    clip(
        "h264.mov",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=128x96:duration=1:rate=25",
            "-c:v",
            "libx264",
            "-g",
            "12",
            "-pix_fmt",
            "yuv420p",
        ],
    )
}

/// Ask ffprobe for values, as ground truth to compare rawshift against.
pub fn ffprobe(path: &Path, entries: &str, extra: &[&str]) -> Option<String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .arg(entries)
        .args(extra)
        .args(["-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(path)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
