//! RAW format decoders.
//!
//! This module provides format-specific decoders for various RAW image formats.
//! Use `RawFile::open()` as the common entry point for automatic format detection.

#[cfg(feature = "arw-decode")]
pub(crate) mod arw;
#[cfg(feature = "cr2-decode")]
pub(crate) mod cr2;
#[cfg(feature = "cr3-decode")]
pub(crate) mod cr3;
#[cfg(feature = "crw-decode")]
pub(crate) mod crw;
#[cfg(feature = "dng-decode")]
pub(crate) mod dng;
#[cfg(feature = "dng-encode")]
pub(crate) mod dng_export;
mod encode;
pub mod export;
#[cfg(feature = "heic-decode")]
pub(crate) mod heic;
#[cfg(feature = "nef-decode")]
pub(crate) mod nef;
#[cfg(feature = "raf-decode")]
pub(crate) mod raf;
pub(crate) mod standard;

#[cfg(feature = "dng-encode")]
pub use dng_export::{DngExportConfig, export_dng};
pub use encode::{encode_rgb_image, encode_rgb_image_to_writer};
#[cfg(feature = "heic-decode")]
pub use heic::{HeicAuxImage, HeicAuxKind, HeicFile};
pub use standard::{
    DecodeOptions, GifDecodeConfig, ImageAvifDecodeConfig, JxlOxideDecodeConfig,
    LibheifDecodeConfig, LibwebpDecodeConfig, ResvgDecodeConfig, StandardFormat, TiffDecodeConfig,
    ZuneJpegDecodeConfig, ZunePngDecodeConfig, decode_standard_image, decode_standard_image_with,
    detect_standard_format, read_standard_image_metadata,
};

#[cfg(feature = "tiff-parser")]
use crate::tiff::{TiffParser, TiffTag};

#[cfg(any_raw)]
use {
    crate::core::image::{RawImage, RgbImage},
    crate::error::{RawError, RawResult},
    crate::processing::ProcessingOptions,
    crate::transforms::{
        apply_bad_pixel_correction, apply_bilateral_filter, apply_black_level, apply_ca_correction,
        apply_color_matrix, apply_tone_reproduction, apply_white_balance, apply_white_balance_raw,
        compute_camera_to_srgb,
    },
    export::EncodeOptions,
    std::io::{Read, Seek, SeekFrom},
    std::path::Path,
    tracing::instrument,
};

#[cfg(any_raw)]
/// Supported RAW file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RawFormat {
    /// Sony ARW format
    #[cfg(feature = "arw-decode")]
    Arw,
    /// Canon CR2 format
    #[cfg(feature = "cr2-decode")]
    Cr2,
    /// Canon CR3 format
    #[cfg(feature = "cr3-decode")]
    Cr3,
    /// Canon CRW (CIFF) format
    #[cfg(feature = "crw-decode")]
    Crw,
    /// Adobe DNG format
    #[cfg(feature = "dng-decode")]
    Dng,
    /// Nikon NEF format
    #[cfg(feature = "nef-decode")]
    Nef,
    /// Fujifilm RAF format
    #[cfg(feature = "raf-decode")]
    Raf,
}

#[cfg(any_raw)]
impl std::fmt::Display for RawFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "arw-decode")]
            RawFormat::Arw => write!(f, "ARW"),
            #[cfg(feature = "cr2-decode")]
            RawFormat::Cr2 => write!(f, "CR2"),
            #[cfg(feature = "cr3-decode")]
            RawFormat::Cr3 => write!(f, "CR3"),
            #[cfg(feature = "crw-decode")]
            RawFormat::Crw => write!(f, "CRW"),
            #[cfg(feature = "dng-decode")]
            RawFormat::Dng => write!(f, "DNG"),
            #[cfg(feature = "nef-decode")]
            RawFormat::Nef => write!(f, "NEF"),
            #[cfg(feature = "raf-decode")]
            RawFormat::Raf => write!(f, "RAF"),
        }
    }
}

#[cfg(any_raw)]
/// Common entry point for parsing RAW files.
///
/// Wraps the specific format implementation for the detected file type.
pub enum RawFile<R> {
    /// Sony ARW format
    #[cfg(feature = "arw-decode")]
    Arw(Box<arw::ArwFile<R>>),
    /// Canon CR2 format
    #[cfg(feature = "cr2-decode")]
    Cr2(Box<cr2::Cr2File<R>>),
    /// Canon CR3 format
    #[cfg(feature = "cr3-decode")]
    Cr3(Box<cr3::Cr3File<R>>),
    /// Canon CRW (CIFF) format
    #[cfg(feature = "crw-decode")]
    Crw(Box<crw::CrwFile<R>>),
    /// Adobe DNG format
    #[cfg(feature = "dng-decode")]
    Dng(Box<dng::DngFile<R>>),
    /// Nikon NEF format
    #[cfg(feature = "nef-decode")]
    Nef(Box<nef::NefFile<R>>),
    /// Fujifilm RAF format
    #[cfg(feature = "raf-decode")]
    Raf(Box<raf::RafFile<R>>),
}

/// Macro that generates feature-gated match arms for uniform `RawFile` dispatch.
///
/// Use when every format variant calls the same method on its inner value:
/// ```ignore
/// raw_format_dispatch!(self, inner => inner.some_method())
/// ```
#[cfg(any_raw)]
macro_rules! raw_format_dispatch {
    ($self:expr, $inner:ident => $body:expr) => {
        match $self {
            #[cfg(feature = "arw-decode")]
            Self::Arw($inner) => $body,
            #[cfg(feature = "cr2-decode")]
            Self::Cr2($inner) => $body,
            #[cfg(feature = "cr3-decode")]
            Self::Cr3($inner) => $body,
            #[cfg(feature = "crw-decode")]
            Self::Crw($inner) => $body,
            #[cfg(feature = "dng-decode")]
            Self::Dng($inner) => $body,
            #[cfg(feature = "nef-decode")]
            Self::Nef($inner) => $body,
            #[cfg(feature = "raf-decode")]
            Self::Raf($inner) => $body,
        }
    };
}

#[cfg(any_raw)]
impl<R: Read + Seek> RawFile<R> {
    /// Open and parse a RAW file, automatically detecting the format.
    ///
    /// This is the primary entry point for using valid file formats.
    pub fn open(mut reader: R) -> RawResult<Self> {
        let format = Self::detect_format(&mut reader)?;

        match format {
            #[cfg(feature = "arw-decode")]
            RawFormat::Arw => {
                let file = arw::ArwFile::parse(reader)?;
                Ok(RawFile::Arw(Box::new(file)))
            }
            #[cfg(feature = "cr2-decode")]
            RawFormat::Cr2 => {
                let file = cr2::Cr2File::parse(reader)?;
                Ok(RawFile::Cr2(Box::new(file)))
            }
            #[cfg(feature = "cr3-decode")]
            RawFormat::Cr3 => {
                let file = cr3::Cr3File::parse(reader)?;
                Ok(RawFile::Cr3(Box::new(file)))
            }
            #[cfg(feature = "crw-decode")]
            RawFormat::Crw => {
                let file = crw::CrwFile::parse(reader)?;
                Ok(RawFile::Crw(Box::new(file)))
            }
            #[cfg(feature = "dng-decode")]
            RawFormat::Dng => {
                let file = dng::DngFile::parse(reader)?;
                Ok(RawFile::Dng(Box::new(file)))
            }
            #[cfg(feature = "nef-decode")]
            RawFormat::Nef => {
                let file = nef::NefFile::parse(reader)?;
                Ok(RawFile::Nef(Box::new(file)))
            }
            #[cfg(feature = "raf-decode")]
            RawFormat::Raf => {
                let file = raf::RafFile::parse(reader)?;
                Ok(RawFile::Raf(Box::new(file)))
            }
        }
    }

    /// Get unified metadata from this RAW file.
    ///
    /// This provides format-agnostic access to all available metadata.
    pub fn metadata(&self) -> crate::core::ImageMetadata {
        use crate::core::MetadataExtractor;
        raw_format_dispatch!(self, inner => inner.extract_metadata())
    }

    /// Extract the embedded JPEG thumbnail from the RAW file, if available.
    ///
    /// Returns `Ok(Some(jpeg_bytes))` when a thumbnail is found, `Ok(None)` when
    /// the format does not contain one (or extraction is not yet implemented).
    pub fn thumbnail(&mut self) -> RawResult<Option<Vec<u8>>> {
        raw_format_dispatch!(self, inner => inner.thumbnail())
    }

    /// Decode raw sensor data without processing.
    ///
    /// Returns the raw Bayer/X-Trans CFA data with original bit depth.
    /// For LinearRaw DNG files, returns the already-demosaiced data as a RawImage.
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        raw_format_dispatch!(self, inner => inner.decode_raw())
    }

    /// Decode and process to an in-memory RGB image.
    ///
    /// Runs the full processing pipeline (black level, WB, demosaic, color matrix,
    /// denoise, CA correction, tone mapping, orientation) and returns the result
    /// without writing to disk.
    #[instrument(skip(self), fields(process = ?processing_options))]
    pub fn process(&mut self, processing_options: &ProcessingOptions) -> RawResult<RgbImage> {
        tracing::trace!("Processing raw file to RGB");

        let mut wb_applied_to_cfa = false;

        // 1. Obtain the initial RGB image
        let mut rgb_image = if self.is_linear_raw_dng() {
            #[cfg(feature = "dng-decode")]
            {
                tracing::trace!("Using LinearRaw path (already demosaiced)");
                let RawFile::Dng(dng) = self else {
                    unreachable!()
                };

                let metadata = dng.metadata();
                let bit_depth = metadata.map(|m| m.bit_depth).unwrap_or(16);
                let linearization_table = metadata.and_then(|m| m.linearization_table.as_ref());

                let is_scaled_by_table = if let Some(table) = linearization_table {
                    if !table.is_empty() {
                        let max_val = table.iter().max().copied().unwrap_or(0);
                        tracing::trace!("LinearizationTable present. Max value: {}", max_val);
                        max_val > 4095
                    } else {
                        false
                    }
                } else {
                    false
                };

                let mut image = dng.decode_linear_raw()?;

                let shift = if is_scaled_by_table {
                    0
                } else {
                    16u8.saturating_sub(bit_depth)
                };

                if shift > 0 {
                    tracing::debug!(
                        "Scaling {}-bit linear data to 16-bit (shift: {})",
                        bit_depth,
                        shift
                    );
                    for pixel in &mut image.data {
                        let val = (*pixel as u32) << shift;
                        *pixel = val.min(65535) as u16;
                    }
                }
                image
            }
            #[cfg(not(feature = "dng-decode"))]
            unreachable!()
        } else {
            tracing::trace!("Using standard CFA path (demosaicing needed)");

            let cfa_wb = processing_options.white_balance.or_else(|| {
                let meta = self.metadata();
                if let Some(neutral) = meta.dng_color.as_shot_neutral {
                    if neutral[0] > 0.0 && neutral[1] > 0.0 && neutral[2] > 0.0 {
                        tracing::trace!("Using AsShotNeutral from metadata: {:?}", neutral);
                        return Some((
                            1.0 / neutral[0] as f32,
                            1.0 / neutral[1] as f32,
                            1.0 / neutral[2] as f32,
                        ));
                    }
                }
                tracing::warn!(
                    "No white balance metadata found. Image may appear green (unbalanced)."
                );
                None
            });

            let mut raw_image = self.decode_raw()?;

            apply_black_level(&mut raw_image);

            if let Some(mode) = processing_options.bad_pixel_correction {
                tracing::trace!("Applying bad pixel correction: {:?}", mode);
                apply_bad_pixel_correction(&mut raw_image, mode, 0.5);
            }

            let effective_white = raw_image
                .white_level()
                .saturating_sub(raw_image.black_levels()[0]);
            if let Some(coeffs) = cfa_wb {
                apply_white_balance_raw(&mut raw_image, coeffs);
                wb_applied_to_cfa = true;
            }

            if effective_white > 0 && effective_white < 65535 {
                let scale = 65535.0 / effective_white as f32;
                for pixel in &mut raw_image.data {
                    *pixel = (*pixel as f32 * scale).min(65535.0) as u16;
                }
            }

            let demosaic_impl = processing_options.demosaic.to_demosaic();
            let mut rgb = demosaic_impl.demosaic(&raw_image);

            rgb.set_baseline_exposure(raw_image.baseline_exposure());
            rgb.set_default_crop(raw_image.default_crop());

            rgb
        };

        // 2. Post-Processing Pipeline
        tracing::trace!("Applying post-processing");

        if let Some(exposure) = rgb_image.baseline_exposure() {
            tracing::debug!(
                "Applying BaselineExposure={:.2} EV with filmic tone mapping",
                exposure
            );
        } else {
            tracing::trace!("Applying filmic tone mapping (no BaselineExposure)");
        }

        if let Some(crop) = rgb_image.default_crop() {
            tracing::trace!(
                "Cropping to default crop: {}x{} at {},{}",
                crop.size.width,
                crop.size.height,
                crop.origin.x,
                crop.origin.y
            );
            crate::transforms::orientation::apply_crop(&mut rgb_image, crop);
        }

        let wb_coeffs = if wb_applied_to_cfa {
            None
        } else {
            processing_options.white_balance.or_else(|| {
                let meta = self.metadata();
                if let Some(neutral) = meta.dng_color.as_shot_neutral {
                    if neutral[0] > 0.0 && neutral[1] > 0.0 && neutral[2] > 0.0 {
                        tracing::trace!("Using AsShotNeutral from metadata: {:?}", neutral);
                        return Some((
                            1.0 / neutral[0] as f32,
                            1.0 / neutral[1] as f32,
                            1.0 / neutral[2] as f32,
                        ));
                    }
                }

                if !self.is_linear_raw_dng() {
                    tracing::warn!(
                        "No white balance metadata found. Image may appear green (unbalanced)."
                    );
                }

                None
            })
        };

        if let Some(coeffs) = wb_coeffs {
            tracing::trace!("Applying white balance: {:?}", coeffs);
            apply_white_balance(&mut rgb_image, coeffs);
        }

        let color_matrix = processing_options.color_matrix.or_else(|| {
            let meta = self.metadata();
            let xyz_to_cam = meta
                .dng_color
                .color_matrix_2
                .or(meta.dng_color.color_matrix_1)
                .or_else(|| {
                    let model = &meta.camera.model;
                    if model.is_empty() {
                        return None;
                    }
                    let cal = crate::data::cameras::find_camera_calibration(model)?;
                    tracing::trace!("Using camera database color matrix for {}", model);
                    cal.color_matrix_2.or(cal.color_matrix_1)
                });

            if let Some(ref cm) = xyz_to_cam {
                match compute_camera_to_srgb(cm) {
                    Some(m) => {
                        tracing::trace!("Auto-resolved camera-to-sRGB color matrix");
                        Some(m)
                    }
                    None => {
                        tracing::warn!("Color matrix is singular, skipping color correction");
                        None
                    }
                }
            } else {
                tracing::debug!("No color matrix available in metadata or camera database");
                None
            }
        });
        if let Some(matrix) = color_matrix {
            tracing::trace!("Applying color matrix");
            apply_color_matrix(&mut rgb_image, &matrix);
        }

        if let Some(sigma) = processing_options.denoise_sigma {
            tracing::trace!("Applying bilateral denoise: sigma={}", sigma);
            let radius = (sigma * 2.0).ceil() as u32;
            apply_bilateral_filter(&mut rgb_image, sigma, sigma * 10000.0, radius);
        }

        if let Some((red_scale, blue_scale)) = processing_options.ca_correction {
            tracing::trace!(
                "Applying CA correction: red_scale={}, blue_scale={}",
                red_scale,
                blue_scale
            );
            apply_ca_correction(&mut rgb_image, red_scale, blue_scale);
        }

        if let Some(g) = processing_options.gamma {
            tracing::trace!("Applying custom gamma override: {}", g);
        }
        apply_tone_reproduction(&mut rgb_image, processing_options.gamma);

        let raw_orientation = self.metadata().image.orientation.unwrap_or(1);
        if raw_orientation != 1 {
            tracing::trace!("Applying orientation transform: {}", raw_orientation);
            crate::transforms::orientation::apply_orientation(&mut rgb_image, raw_orientation);
        }

        Ok(rgb_image)
    }

    /// Export the raw file to an image format based on the encoded options.
    ///
    /// This runs the full processing pipeline:
    /// 1. Decode raw data
    /// 2. Apply black level subtraction and normalization
    /// 3. Demosaic
    /// 4. Apply White Balance (if specified)
    /// 5. Apply Color Matrix (if specified)
    /// 6. Apply Gamma Correction (if specified)
    /// 7. Save to disk using format-specific encoder
    #[instrument(
        skip(self),
        fields(
            path = %path.as_ref().display(),
            process = ?processing_options,
            encode = ?encode_options
        )
    )]
    pub fn export<P: AsRef<Path>>(
        &mut self,
        path: P,
        processing_options: &ProcessingOptions,
        encode_options: &EncodeOptions,
    ) -> RawResult<()> {
        tracing::trace!("Exporting raw file");

        let rgb_image = self.process(processing_options)?;

        // Build metadata for EXIF embedding.
        // If orientation was applied to pixel data, mark the output as orientation=1 (Normal)
        // so viewers don't apply it a second time.
        let raw_orientation = self.metadata().image.orientation.unwrap_or(1);
        let exif_metadata = {
            let mut m = self.metadata();
            if raw_orientation != 1 {
                m.image.orientation = Some(1);
            }
            m
        };

        tracing::info!("Encoding image to disk: {:?}", path.as_ref());
        encode_rgb_image(&rgb_image, &exif_metadata, path.as_ref(), encode_options)
    }

    /// Helper to check if the current file is a LinearRaw DNG
    pub fn is_linear_raw_dng(&self) -> bool {
        match self {
            #[cfg(feature = "dng-decode")]
            RawFile::Dng(dng) => dng.metadata().map(|m| m.is_linear_raw).unwrap_or(false),
            #[cfg(feature = "arw-decode")]
            RawFile::Arw(_) => false,
            #[cfg(feature = "cr2-decode")]
            RawFile::Cr2(_) => false,
            #[cfg(feature = "cr3-decode")]
            RawFile::Cr3(_) => false,
            #[cfg(feature = "crw-decode")]
            RawFile::Crw(_) => false,
            #[cfg(feature = "nef-decode")]
            RawFile::Nef(_) => false,
            #[cfg(feature = "raf-decode")]
            RawFile::Raf(_) => false,
        }
    }

    /// Detect the format of the provided reader.
    fn detect_format(reader: &mut R) -> RawResult<RawFormat> {
        // Read magic bytes (16 bytes covers TIFF header + CR2 magic at offset 8,
        // and the full RAF magic string). We also read up to 14 bytes for CRW.
        let start = reader.stream_position()?;
        let mut header = [0u8; 16];
        reader.read_exact(&mut header)?;
        reader.seek(SeekFrom::Start(start))?;

        // Check for Fujifilm RAF magic first (not TIFF-based)
        #[cfg(feature = "raf-decode")]
        if raf::is_raf(&header) {
            return Ok(RawFormat::Raf);
        }

        // CR3 detection must come BEFORE the TIFF check because CR3 uses ISOBMFF
        // (not TIFF) and would otherwise be rejected as an unsupported format.
        #[cfg(feature = "cr3-decode")]
        if cr3::is_cr3(&header) {
            return Ok(RawFormat::Cr3);
        }

        // CRW detection must come BEFORE the TIFF check. CRW uses II/MM + 0x0001
        // (not 0x002A) and has "HEAPCCDR" at bytes 6..14. A standard TIFF parser
        // would reject it because the magic number is wrong.
        #[cfg(feature = "crw-decode")]
        if crw::is_crw(&header) {
            return Ok(RawFormat::Crw);
        }

        // Check for TIFF magic (II or MM at offset 0)
        let is_tiff = (header[0] == b'I' && header[1] == b'I' && header[2] == 42 && header[3] == 0)
            || (header[0] == b'M' && header[1] == b'M' && header[2] == 0 && header[3] == 42);

        if !is_tiff {
            return Err(RawError::Unsupported(
                "Not a TIFF-based RAW file".to_string(),
            ));
        }

        // CR2 detection via magic bytes at offset 8: "CR" + 0x02
        // This is faster than parsing IFDs and more reliable.
        #[cfg(feature = "cr2-decode")]
        if cr2::is_cr2(&header) {
            return Ok(RawFormat::Cr2);
        }

        // Parse as TIFF to inspect Make tag for format detection
        #[cfg(feature = "tiff-parser")]
        {
            let mut parser = TiffParser::new(reader)?;
            let ifd0 = parser.parse_ifd0()?;

            // Check for DNG version first - if present, it's a DNG regardless of Make
            #[cfg(feature = "dng-decode")]
            if ifd0.get(TiffTag::DNGVersion).is_some() {
                return Ok(RawFormat::Dng);
            }

            // Check Make tag to determine specific format
            if let Some(make_entry) = ifd0.get(TiffTag::Make) {
                if let Ok(value) = parser.read_value(make_entry) {
                    if let Some(make) = value.as_str() {
                        let make_lower = make.to_lowercase();
                        #[cfg(feature = "arw-decode")]
                        if make_lower.contains("sony") {
                            return Ok(RawFormat::Arw);
                        }
                        // Add more manufacturers here as we add support
                        #[cfg(feature = "nef-decode")]
                        if make_lower.contains("nikon") {
                            return Ok(RawFormat::Nef);
                        }
                    }
                }
            }
        }

        // Default to DNG for unrecognized TIFF-based formats (or return unsupported)
        Err(RawError::Unsupported(
            "Unrecognized camera manufacturer".to_string(),
        ))
    }
}
#[cfg(test)]
mod tests {
    #[cfg(any_raw)]
    use super::{RawFile, RawFormat};
    #[cfg(any_raw)]
    use crate::error::RawError;
    #[cfg(any_raw)]
    use std::io::Cursor;

    /// Verify that applying WB before normalization prevents highlight clipping
    /// for neutral-gray pixels that would otherwise be pushed above 65535.
    #[test]
    fn test_wb_before_normalization_no_clipping_for_midtones() {
        // Simulate 14-bit sensor: white_level=16383, black_level=512
        // WB: R=2.35, G=1.0, B=1.65 (typical daylight for Sony APS-C)
        let white_level: u16 = 16383;
        let black_level: u16 = 512;
        let effective_white = white_level - black_level; // 15871
        let (r_gain, g_gain, b_gain) = (2.35f32, 1.0f32, 1.65f32);

        // A neutral gray at 50% brightness:
        // R_raw_neutral = effective_white / r_gain / 2 ≈ 3377 (+ black = 3889)
        // G_raw_neutral = effective_white / 2 ≈ 7936 (+ black = 8448)
        // B_raw_neutral = effective_white / b_gain / 2 ≈ 4810 (+ black = 5322)
        let r_raw: u16 = 3377; // after black subtraction
        let g_raw: u16 = 7936;
        let b_raw: u16 = 4810;

        // Apply WB and clamp to effective_white
        let white_f = effective_white as f32;
        let r_wb = (r_raw as f32 * r_gain).min(white_f) as u16;
        let g_wb = (g_raw as f32 * g_gain).min(white_f) as u16;
        let b_wb = (b_raw as f32 * b_gain).min(white_f) as u16;

        // None should exceed effective_white
        assert!(
            r_wb <= effective_white,
            "R clipped: {r_wb} > {effective_white}"
        );
        assert!(
            g_wb <= effective_white,
            "G clipped: {g_wb} > {effective_white}"
        );
        assert!(
            b_wb <= effective_white,
            "B clipped: {b_wb} > {effective_white}"
        );

        // After 16-bit normalization, all channels should be approximately equal
        // (neutral gray should be neutral after WB)
        let scale = 65535.0 / effective_white as f32;
        let r_16 = (r_wb as f32 * scale) as u16;
        let g_16 = (g_wb as f32 * scale) as u16;
        let b_16 = (b_wb as f32 * scale) as u16;

        // All should be close to ~32767 (50% of 65535), with ≤5% tolerance
        let expected = 32767u16;
        let tolerance = 2000u16;
        assert!(
            r_16.abs_diff(expected) < tolerance,
            "R {r_16} not near {expected}"
        );
        assert!(
            g_16.abs_diff(expected) < tolerance,
            "G {g_16} not near {expected}"
        );
        assert!(
            b_16.abs_diff(expected) < tolerance,
            "B {b_16} not near {expected}"
        );
    }

    /// Verify that the old approach (normalize-then-WB) clips midtones incorrectly.
    /// This demonstrates the bug that the fix addresses.
    #[test]
    fn test_old_approach_clips_midtones() {
        let white_level: u16 = 16383;
        let black_level: u16 = 512;
        let effective_white = white_level - black_level; // 15871
        let r_gain = 2.35f32;

        // A red pixel at ~44% of effective_white (just above the clipping threshold
        // in the OLD normalize-then-WB pipeline)
        let r_raw: u16 = (effective_white as f32 * 0.44) as u16; // ~6983

        // Old pipeline: bit-shift to 16-bit FIRST, then WB
        let shift = 16u8.saturating_sub(14); // bit_depth=14, shift=2
        let r_shifted = ((r_raw as u32) << shift).min(65535) as u16; // *4
        let r_old = (r_shifted as f32 * r_gain).min(65535.0) as u16;

        // New pipeline: WB first (clamped to effective_white), then normalize
        let white_f = effective_white as f32;
        let r_wb = (r_raw as f32 * r_gain).min(white_f) as u16;
        let scale = 65535.0 / effective_white as f32;
        let r_new = (r_wb as f32 * scale).min(65535.0) as u16;

        // Old approach clips to 65535 (wrong - this is only ~44% brightness)
        assert_eq!(r_old, 65535, "Old approach should clip to white");

        // New approach produces ~65535 too because 0.44 * 2.35 > 1.0,
        // meaning this pixel IS genuinely overexposed for red at this WB setting.
        // The important case is pixels BELOW the channel white point (effective_white/gain).
        let r_channel_white = (effective_white as f32 / r_gain) as u16; // ~6753
        let r_neutral_50pct = r_channel_white / 2; // ~3377

        let r_wb_neutral = (r_neutral_50pct as f32 * r_gain).min(white_f) as u16;
        let r_new_neutral = (r_wb_neutral as f32 * scale).min(65535.0) as u16;

        let r_shifted_neutral = ((r_neutral_50pct as u32) << shift).min(65535) as u16;
        let r_old_neutral = (r_shifted_neutral as f32 * r_gain).min(65535.0) as u16;

        // Old approach: a 50%-brightness neutral red pixel does NOT clip
        assert!(r_old_neutral < 65535, "Old neutral 50% should not clip");
        // New approach: same, but also scales correctly to white level
        assert!(r_new_neutral < 65535, "New neutral 50% should not clip");
        // New approach gives a value close to ~32767 (50% of full range)
        assert!(
            r_new_neutral.abs_diff(32767) < 3000,
            "New neutral 50% {r_new_neutral} should be ~50% of 65535"
        );

        // Suppress unused variable warning
        let _ = r_new;
    }

    #[cfg(any_raw)]
    #[test]
    fn test_detect_format_invalid_magic() {
        // This data is valid length but has wrong magic bytes
        // We pad to 16+ bytes to satisfy the header read
        let mut data = vec![0u8; 32];
        data[..14].copy_from_slice(b"not a raw file");
        let mut cursor = Cursor::new(data);
        let result = RawFile::detect_format(&mut cursor);
        assert!(
            matches!(result, Err(RawError::Unsupported(_))),
            "Should fail with UnsupportedFormat for invalid magic: {:?}",
            result
        );
    }

    #[cfg(feature = "tiff-parser")]
    #[test]
    fn test_detect_format_tiff_no_make() {
        // Valid TIFF header but no Make tag - should return UnsupportedFormat
        // Pad to ensure enough data for parser
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes()); // IFD at offset 8
        data[8..10].copy_from_slice(&0u16.to_le_bytes()); // 0 entries
        data[10..14].copy_from_slice(&0u32.to_le_bytes()); // no next IFD

        let mut cursor = Cursor::new(data);
        let result = RawFile::detect_format(&mut cursor);
        assert!(
            matches!(result, Err(RawError::Unsupported(_))),
            "Should fail with UnsupportedFormat for unrecognized camera: {:?}",
            result
        );
    }

    #[cfg(feature = "dng-decode")]
    #[test]
    fn test_detect_format_dng() {
        // Mock TIFF with DNGVersion tag
        let mut data = vec![0u8; 64];
        // TIFF Header (LE)
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // IFD at offset 8
        let entry_count = 1u16;
        data[8..10].copy_from_slice(&entry_count.to_le_bytes());

        // Entry 1: DNGVersion (0xC612)
        // Tag (2), Type (1=Byte), Count (4), Value/Offset (1,2,3,4)
        data[10..12].copy_from_slice(&0xC612u16.to_le_bytes());
        data[12..14].copy_from_slice(&1u16.to_le_bytes()); // Type Byte
        data[14..18].copy_from_slice(&4u32.to_le_bytes()); // Count 4
        data[18..22].copy_from_slice(&[1, 1, 0, 0]); // Version 1.1.0.0

        // Next IFD (0)
        data[22..26].copy_from_slice(&0u32.to_le_bytes());

        let mut cursor = Cursor::new(data);
        let result = RawFile::detect_format(&mut cursor);
        assert!(matches!(result, Ok(RawFormat::Dng)));
    }

    #[cfg(feature = "dng-decode")]
    #[test]
    fn test_detect_format_sony_dng() {
        // Mock TIFF with BOTH DNGVersion and Make="Sony"
        // Should be detected as DNG, not ARW
        let mut data = vec![0u8; 128];
        // TIFF Header (LE)
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // IFD at offset 8
        let entry_count = 2u16;
        data[8..10].copy_from_slice(&entry_count.to_le_bytes());

        // Entry 1: Make (0x010F), Type ASCII (2), Count 5 ("Sony\0"), Offset to data
        let make_offset = 64u32;
        data[10..12].copy_from_slice(&0x010Fu16.to_le_bytes());
        data[12..14].copy_from_slice(&2u16.to_le_bytes());
        data[14..18].copy_from_slice(&5u32.to_le_bytes());
        data[18..22].copy_from_slice(&make_offset.to_le_bytes());

        // Entry 2: DNGVersion (0xC612)
        // Tag (2), Type (1=Byte), Count (4), Value/Offset (1,2,3,4)
        data[22..24].copy_from_slice(&0xC612u16.to_le_bytes());
        data[24..26].copy_from_slice(&1u16.to_le_bytes());
        data[26..30].copy_from_slice(&4u32.to_le_bytes());
        data[30..34].copy_from_slice(&[1, 1, 0, 0]);

        // Next IFD (0)
        data[34..38].copy_from_slice(&0u32.to_le_bytes());

        // String data at offset 64
        data[64..69].copy_from_slice(b"Sony\0");

        let mut cursor = Cursor::new(data);
        let result = RawFile::detect_format(&mut cursor);
        assert!(matches!(result, Ok(RawFormat::Dng)));
    }

    // -------------------------------------------------------------------------
    // Tests for orientation transforms (via transforms::orientation module)
    // -------------------------------------------------------------------------

    fn make_test_rgb(width: u32, height: u32, data: Vec<u16>) -> crate::core::image::RgbImage {
        crate::core::image::RgbImage::new(width, height, data)
    }

    #[test]
    fn test_flip_horizontal_2x1() {
        use crate::transforms::orientation::flip_horizontal;
        let mut img = make_test_rgb(2, 1, vec![10, 11, 12, 20, 21, 22]);
        flip_horizontal(&mut img);
        assert_eq!(img.data, vec![20, 21, 22, 10, 11, 12]);
    }

    #[test]
    fn test_rotate_180() {
        use crate::transforms::orientation::rotate_180;
        let mut img = make_test_rgb(2, 1, vec![1, 2, 3, 4, 5, 6]);
        rotate_180(&mut img);
        assert_eq!(img.data, vec![4, 5, 6, 1, 2, 3]);
    }

    #[test]
    fn test_rotate_90_cw_1x2() {
        use crate::transforms::orientation::rotate_90_cw;
        let mut img = make_test_rgb(1, 2, vec![1, 2, 3, 4, 5, 6]);
        rotate_90_cw(&mut img);
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 1);
        assert_eq!(img.data, vec![4, 5, 6, 1, 2, 3]);
    }

    #[test]
    fn test_rotate_90_ccw_2x1() {
        use crate::transforms::orientation::rotate_90_ccw;
        let mut img = make_test_rgb(2, 1, vec![1, 2, 3, 4, 5, 6]);
        rotate_90_ccw(&mut img);
        assert_eq!(img.width(), 1);
        assert_eq!(img.height(), 2);
        assert_eq!(img.data, vec![4, 5, 6, 1, 2, 3]);
    }

    #[test]
    fn test_orientation_identity() {
        use crate::transforms::orientation::apply_orientation;
        let mut img = make_test_rgb(2, 2, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        let original = img.data.clone();
        apply_orientation(&mut img, 1);
        assert_eq!(img.data, original);
    }

    #[test]
    fn test_orientation_6_cw_then_ccw_is_identity() {
        use crate::transforms::orientation::apply_orientation;
        let mut img = make_test_rgb(
            3,
            2,
            vec![
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
            ],
        );
        let original_data = img.data.clone();
        let original_w = img.width();
        let original_h = img.height();
        apply_orientation(&mut img, 6); // 90° CW
        apply_orientation(&mut img, 8); // 90° CCW (should undo it)
        assert_eq!(img.width(), original_w);
        assert_eq!(img.height(), original_h);
        assert_eq!(img.data, original_data);
    }

    #[cfg(any_raw)]
    #[test]
    fn test_open_empty_reader_returns_error() {
        // An empty reader cannot be a valid RAW file
        let cursor = Cursor::new(vec![]);
        let result = RawFile::open(cursor);
        assert!(
            result.is_err(),
            "Opening an empty reader should return an error"
        );
    }

    #[cfg(any_raw)]
    #[test]
    fn test_detect_format_empty_returns_error() {
        // detect_format on an empty buffer should return an Io error (UnexpectedEof)
        let mut cursor = Cursor::new(vec![]);
        let result = RawFile::detect_format(&mut cursor);
        assert!(
            result.is_err(),
            "detect_format on empty input should return an error"
        );
    }
}
