//! The decode → encode → temp file → decode → compare round trip.
//!
//! Pure compute and file IO, with no `iced` types anywhere, so the headless
//! runner and the unit tests drive exactly the same code the window does.
//!
//! Every stage returns a `Result` whose error is a displayable string rather
//! than a typed error: the gallery's job is to *render* the failure next to
//! the successes, not to react to its variant. A missing hardware decoder, a
//! truncated file, and an encoder that refuses a bit depth all reach the grid
//! the same way.

use std::path::{Path, PathBuf};
use std::time::Instant;

use rawshift_image::core::RgbImage;
use rawshift_image::core::metadata::ImageMetadata;
use rawshift_image::formats::export::OutputFormat;
use rawshift_image::formats::{
    StandardFormat, decode_standard_image, detect_standard_format, encode_rgb_image_to_vec,
    read_standard_image_metadata,
};

use crate::settings::FormatConfig;

/// How a source file was decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A standard delivery format, through `decode_standard_image`.
    Standard(StandardFormat),
    /// A camera RAW file, developed through `RawFile::process`.
    Raw,
}

impl SourceKind {
    pub fn label(self) -> String {
        match self {
            SourceKind::Standard(f) => f.name().to_owned(),
            SourceKind::Raw => "RAW".to_owned(),
        }
    }
}

/// A decoded source image and everything the UI needs to describe it.
#[derive(Debug, Clone)]
pub struct Source {
    pub path: PathBuf,
    pub kind: SourceKind,
    pub image: RgbImage,
    pub metadata: ImageMetadata,
    /// Bytes on disk, for the compression ratios in the results grid.
    pub file_bytes: u64,
}

impl Source {
    /// The file's stem, for labelling a row.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// Controls for the RAW development pipeline, used only for RAW sources.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawSettings {
    pub gamma: f32,
}

impl Default for RawSettings {
    fn default() -> Self {
        // 2.2 matches `ProcessingOptions`' own sRGB default.
        Self { gamma: 2.2 }
    }
}

/// Decode one source file.
///
/// RAW is tried first and deliberately so: DNG, ARW, NEF, and CR2 are all
/// TIFF-based, so `detect_standard_format` reports them as
/// [`StandardFormat::Tiff`] and the standard decoder would return the embedded
/// preview or fail, rather than developing the sensor data.
pub fn load_source(path: &Path, raw: RawSettings) -> Result<Source, String> {
    let file_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    if let Some(source) = try_load_raw(path, raw, file_bytes) {
        return source;
    }

    let bytes = std::fs::read(path).map_err(|e| format!("read failed: {e}"))?;
    let format = detect_standard_format(&bytes)
        .ok_or_else(|| "unrecognised image format (no magic-byte match)".to_owned())?;
    let image = decode_standard_image(&bytes, format)
        .map_err(|e| format!("{format} decode failed: {e}"))?;
    let metadata = read_standard_image_metadata(&bytes, format);
    Ok(Source {
        path: path.to_path_buf(),
        kind: SourceKind::Standard(format),
        image,
        metadata,
        file_bytes,
    })
}

/// Attempt a RAW decode.
///
/// `None` means "not a RAW file, try the standard path"; `Some(Err(..))` means
/// it *was* RAW and developing it failed, which must surface rather than be
/// retried as a standard image.
fn try_load_raw(path: &Path, raw: RawSettings, file_bytes: u64) -> Option<Result<Source, String>> {
    use rawshift_image::formats::RawFile;
    use rawshift_image::processing::ProcessingOptions;

    let file = std::fs::File::open(path).ok()?;
    let mut raw_file = RawFile::open(std::io::BufReader::new(file)).ok()?;

    let options = ProcessingOptions::new().gamma(raw.gamma);
    let metadata = raw_file.metadata();
    Some(
        raw_file
            .process(&options)
            .map(|image| Source {
                path: path.to_path_buf(),
                kind: SourceKind::Raw,
                image,
                metadata,
                file_bytes,
            })
            .map_err(|e| format!("RAW development failed: {e}")),
    )
}

/// How a round-tripped image compares to its source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Comparison {
    /// Peak signal-to-noise ratio in dB over all three channels, or `None`
    /// when the images are identical (PSNR is infinite there, and printing
    /// `inf` reads as a bug rather than as success).
    pub psnr_db: Option<f64>,
    /// The largest absolute per-channel difference, in 16-bit sample units.
    pub max_delta: u16,
    /// Whether every sample matched exactly.
    pub bit_exact: bool,
}

/// The result of encoding and re-decoding one source through one format.
#[derive(Debug, Clone)]
pub struct VariantOk {
    pub encoded_bytes: usize,
    pub temp_path: PathBuf,
    pub encode_ms: f64,
    pub decode_ms: f64,
    pub image: RgbImage,
    /// `None` when the re-decoded image has different dimensions from the
    /// source, which makes a per-sample comparison meaningless.
    pub comparison: Option<Comparison>,
    /// Set when the round trip claimed to be lossless but was not — the one
    /// outcome that is a real defect rather than expected loss.
    pub lossless_violated: bool,
}

/// One cell of the results grid.
#[derive(Debug, Clone)]
pub struct Variant {
    pub format: OutputFormat,
    /// The settings actually used, for the cell caption.
    pub summary: String,
    pub lossless: bool,
    pub outcome: Result<VariantOk, String>,
}

/// The directory this process writes its intermediate files to.
///
/// `std::env::temp_dir()` rather than a hardcoded `/tmp`: on macOS it is a
/// per-user directory and on Windows there is no `/tmp` at all. The PID keeps
/// concurrent runs from overwriting each other.
pub fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join(format!("rawshift-gallery-{}", std::process::id()))
}

/// Encode one source through one format, write it out, and decode it back.
///
/// The intermediate really does go through the filesystem rather than staying
/// in a buffer: reading it back is what exercises the decoder's own sniffing
/// and container parsing on bytes it did not produce in-process.
pub fn round_trip(source: &Source, config: &FormatConfig) -> Variant {
    let format = config.format();
    let variant = Variant {
        format,
        summary: config.summary(),
        lossless: config.is_lossless(),
        outcome: Err(String::new()),
    };
    Variant {
        outcome: run_round_trip(source, config),
        ..variant
    }
}

fn run_round_trip(source: &Source, config: &FormatConfig) -> Result<VariantOk, String> {
    let format = config.format();
    let options = config.to_encode_options();

    let started = Instant::now();
    let encoded = encode_rgb_image_to_vec(&source.image, &source.metadata, &options)
        .map_err(|e| format!("encode failed: {e}"))?;
    let encode_ms = started.elapsed().as_secs_f64() * 1000.0;

    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let stem = source
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_owned());
    let temp_path = dir.join(format!("{stem}.{}", format.extension()));
    std::fs::write(&temp_path, &encoded)
        .map_err(|e| format!("cannot write {}: {e}", temp_path.display()))?;

    let read_back = std::fs::read(&temp_path)
        .map_err(|e| format!("cannot read back {}: {e}", temp_path.display()))?;

    let started = Instant::now();
    let image = decode_encoded(&read_back, format)?;
    let decode_ms = started.elapsed().as_secs_f64() * 1000.0;

    let comparison = compare(&source.image, &image);
    // A lossless configuration that did not come back bit-exact is the one
    // result worth flagging as a defect rather than as expected loss.
    let lossless_violated = config.is_lossless() && comparison.is_some_and(|c| !c.bit_exact);

    Ok(VariantOk {
        encoded_bytes: encoded.len(),
        temp_path,
        encode_ms,
        decode_ms,
        image,
        comparison,
        lossless_violated,
    })
}

/// Decode bytes the gallery just wrote.
///
/// The format is known, but detection runs anyway and must agree: a decoder
/// that cannot recognise its own encoder's output is a defect this
/// demonstration should surface, not paper over.
fn decode_encoded(bytes: &[u8], format: OutputFormat) -> Result<RgbImage, String> {
    let standard = match format {
        OutputFormat::Png => StandardFormat::Png,
        OutputFormat::Jpeg => StandardFormat::Jpeg,
        OutputFormat::WebP => StandardFormat::WebP,
        OutputFormat::Avif => StandardFormat::Avif,
        OutputFormat::Jxl => StandardFormat::Jxl,
        OutputFormat::Dng => {
            return Err("DNG re-decode goes through the RAW pipeline, not implemented here".into());
        }
    };
    match detect_standard_format(bytes) {
        Some(detected) if detected == standard => {}
        Some(detected) => {
            return Err(format!(
                "sniffed as {detected} but {} was encoded",
                standard.name()
            ));
        }
        None => return Err(format!("{} output was not recognised", standard.name())),
    }
    decode_standard_image(bytes, standard).map_err(|e| format!("decode failed: {e}"))
}

/// Compare a round-tripped image against its source.
///
/// `None` when the dimensions differ, which makes a per-sample comparison
/// meaningless — better to say so than to report a delta computed over
/// mismatched pixels.
pub fn compare(source: &RgbImage, actual: &RgbImage) -> Option<Comparison> {
    if source.width() != actual.width() || source.height() != actual.height() {
        return None;
    }
    let (a, b) = (source.data(), actual.data());
    if a.len() != b.len() {
        return None;
    }

    let mut max_delta = 0u16;
    let mut sum_squared = 0f64;
    for (&x, &y) in a.iter().zip(b) {
        let delta = x.abs_diff(y);
        max_delta = max_delta.max(delta);
        sum_squared += f64::from(delta) * f64::from(delta);
    }

    let bit_exact = max_delta == 0;
    let psnr_db = (!bit_exact).then(|| {
        let mse = sum_squared / a.len() as f64;
        let peak = f64::from(u16::MAX);
        20.0 * peak.log10() - 10.0 * mse.log10()
    });

    Some(Comparison {
        psnr_db,
        max_delta,
        bit_exact,
    })
}

/// Remove this process's scratch directory.
///
/// Best-effort: a leftover directory in the system temp area is not worth
/// failing a run over, and the OS reclaims it.
pub fn clean_scratch() {
    let _ = std::fs::remove_dir_all(scratch_dir());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, fill: u16) -> RgbImage {
        RgbImage::new(width, height, vec![fill; (width * height * 3) as usize])
            .expect("valid buffer")
    }

    #[test]
    fn identical_images_are_bit_exact_with_no_psnr() {
        let comparison = compare(&image(4, 4, 30000), &image(4, 4, 30000)).expect("same size");
        assert!(comparison.bit_exact);
        assert_eq!(comparison.max_delta, 0);
        assert_eq!(
            comparison.psnr_db, None,
            "an exact match has infinite PSNR; reporting a number would be misleading"
        );
    }

    #[test]
    fn differing_images_report_delta_and_finite_psnr() {
        let comparison = compare(&image(4, 4, 30000), &image(4, 4, 30100)).expect("same size");
        assert!(!comparison.bit_exact);
        assert_eq!(comparison.max_delta, 100);
        let psnr = comparison.psnr_db.expect("finite PSNR");
        assert!(psnr.is_finite() && psnr > 0.0, "got {psnr}");
    }

    #[test]
    fn psnr_falls_as_error_grows() {
        let near = compare(&image(4, 4, 30000), &image(4, 4, 30010))
            .and_then(|c| c.psnr_db)
            .expect("finite");
        let far = compare(&image(4, 4, 30000), &image(4, 4, 40000))
            .and_then(|c| c.psnr_db)
            .expect("finite");
        assert!(near > far, "near={near} should beat far={far}");
    }

    #[test]
    fn mismatched_dimensions_have_no_comparison() {
        assert!(compare(&image(4, 4, 0), &image(8, 8, 0)).is_none());
    }

    #[test]
    fn scratch_dir_is_process_scoped() {
        let dir = scratch_dir();
        assert!(
            dir.to_string_lossy()
                .contains(&std::process::id().to_string()),
            "concurrent runs must not share a scratch directory: {}",
            dir.display()
        );
        assert!(dir.starts_with(std::env::temp_dir()));
    }

    #[test]
    fn unrecognised_bytes_report_an_error_rather_than_panicking() {
        let err = decode_encoded(b"definitely not an image", OutputFormat::Png)
            .expect_err("garbage must not decode");
        assert!(err.contains("not recognised"), "got: {err}");
    }

    /// The round trip must go through a real file, and a lossless PNG must
    /// come back bit-exact.
    #[test]
    fn png_round_trip_is_bit_exact_through_the_filesystem() {
        use rawshift_image::formats::export::PngEncodeConfig;

        let source = Source {
            path: PathBuf::from("synthetic.png"),
            kind: SourceKind::Standard(StandardFormat::Png),
            image: image(8, 8, 4096),
            metadata: ImageMetadata::default(),
            file_bytes: 0,
        };
        let config = FormatConfig::Png(PngEncodeConfig::default());
        let variant = round_trip(&source, &config);

        let ok = variant.outcome.expect("PNG round trip succeeds");
        assert!(ok.temp_path.exists(), "the intermediate must reach disk");
        assert!(ok.encoded_bytes > 0);
        let comparison = ok.comparison.expect("same dimensions");
        assert!(comparison.bit_exact, "max delta {}", comparison.max_delta);
        assert!(!ok.lossless_violated);
        clean_scratch();
    }

    /// A lossy configuration must not be reported as a lossless violation.
    #[test]
    fn lossy_jpeg_is_not_flagged_as_a_lossless_violation() {
        use rawshift_image::formats::export::JpegEncodeConfig;

        let source = Source {
            path: PathBuf::from("synthetic.jpg"),
            kind: SourceKind::Standard(StandardFormat::Jpeg),
            image: image(16, 16, 20000),
            metadata: ImageMetadata::default(),
            file_bytes: 0,
        };
        let variant = round_trip(&source, &FormatConfig::Jpeg(JpegEncodeConfig::default()));
        let ok = variant.outcome.expect("JPEG round trip succeeds");
        assert!(!ok.lossless_violated);
        clean_scratch();
    }
}
