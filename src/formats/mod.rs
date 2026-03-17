//! RAW format decoders.
//!
//! This module provides format-specific decoders for various RAW image formats.
//! Use `RawFile::open()` as the common entry point for automatic format detection.

pub(crate) mod arw;
pub(crate) mod cr2;
pub(crate) mod cr3;
pub(crate) mod crw;
pub(crate) mod dng;
pub(crate) mod dng_export;
pub mod export;
pub(crate) mod nef;
pub(crate) mod raf;
pub(crate) mod standard;

pub use dng_export::{DngExportConfig, export_dng};
pub use standard::{
    StandardFormat, decode_standard_image, detect_standard_format, read_standard_image_metadata,
};

use std::io::{Read, Seek, SeekFrom};

use crate::core::image::{RawImage, RgbImage};
use crate::core::metadata::ImageMetadata;
use crate::error::{EncodeError, RawError, RawResult};
use crate::processing::ProcessingOptions;
use crate::tiff::{TiffParser, TiffTag};
use crate::transforms::bad_pixel::apply_bad_pixel_correction;
use crate::transforms::black_level::apply_black_level;
use crate::transforms::ca_correction::apply_ca_correction;
use crate::transforms::color::{
    apply_color_matrix, apply_white_balance, apply_white_balance_raw, compute_camera_to_srgb,
};
use crate::transforms::denoise::apply_bilateral_filter;
use crate::transforms::tonemap::apply_tone_reproduction;
use export::EncodeOptions;
use std::path::Path;
use tracing::instrument;

/// Supported RAW file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RawFormat {
    /// Sony ARW format
    Arw,
    /// Canon CR2 format
    Cr2,
    /// Canon CR3 format
    Cr3,
    /// Canon CRW (CIFF) format
    Crw,
    /// Adobe DNG format (planned)
    Dng,
    /// Nikon NEF format
    Nef,
    /// Fujifilm RAF format
    Raf,
}

impl std::fmt::Display for RawFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RawFormat::Arw => write!(f, "ARW"),
            RawFormat::Cr2 => write!(f, "CR2"),
            RawFormat::Cr3 => write!(f, "CR3"),
            RawFormat::Crw => write!(f, "CRW"),
            RawFormat::Dng => write!(f, "DNG"),
            RawFormat::Nef => write!(f, "NEF"),
            RawFormat::Raf => write!(f, "RAF"),
        }
    }
}

/// Common entry point for parsing RAW files.
///
/// Wraps the specific format implementation for the detected file type.
pub enum RawFile<R> {
    /// Sony ARW format
    Arw(Box<arw::ArwFile<R>>),
    /// Canon CR2 format
    Cr2(Box<cr2::Cr2File<R>>),
    /// Canon CR3 format
    Cr3(Box<cr3::Cr3File<R>>),
    /// Canon CRW (CIFF) format
    Crw(Box<crw::CrwFile<R>>),
    /// Adobe DNG format
    Dng(Box<dng::DngFile<R>>),
    /// Nikon NEF format
    Nef(Box<nef::NefFile<R>>),
    /// Fujifilm RAF format
    Raf(Box<raf::RafFile<R>>),
}

impl<R: Read + Seek> RawFile<R> {
    /// Open and parse a RAW file, automatically detecting the format.
    ///
    /// This is the primary entry point for using valid file formats.
    pub fn open(mut reader: R) -> RawResult<Self> {
        let format = Self::detect_format(&mut reader)?;

        match format {
            RawFormat::Arw => {
                let file = arw::ArwFile::parse(reader)?;
                Ok(RawFile::Arw(Box::new(file)))
            }
            RawFormat::Cr2 => {
                let file = cr2::Cr2File::parse(reader)?;
                Ok(RawFile::Cr2(Box::new(file)))
            }
            RawFormat::Cr3 => {
                let file = cr3::Cr3File::parse(reader)?;
                Ok(RawFile::Cr3(Box::new(file)))
            }
            RawFormat::Crw => {
                let file = crw::CrwFile::parse(reader)?;
                Ok(RawFile::Crw(Box::new(file)))
            }
            RawFormat::Dng => {
                let file = dng::DngFile::parse(reader)?;
                Ok(RawFile::Dng(Box::new(file)))
            }
            RawFormat::Nef => {
                let file = nef::NefFile::parse(reader)?;
                Ok(RawFile::Nef(Box::new(file)))
            }
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
        match self {
            RawFile::Arw(arw) => arw.extract_metadata(),
            RawFile::Cr2(cr2) => cr2.extract_metadata(),
            RawFile::Cr3(cr3) => cr3.extract_metadata(),
            RawFile::Crw(crw) => crw.extract_metadata(),
            RawFile::Dng(dng) => dng.extract_metadata(),
            RawFile::Nef(nef) => nef.extract_metadata(),
            RawFile::Raf(raf) => raf.extract_metadata(),
        }
    }

    /// Extract the embedded JPEG thumbnail from the RAW file, if available.
    ///
    /// Returns `Ok(Some(jpeg_bytes))` when a thumbnail is found, `Ok(None)` when
    /// the format does not contain one (or extraction is not yet implemented).
    pub fn thumbnail(&mut self) -> RawResult<Option<Vec<u8>>> {
        match self {
            RawFile::Arw(arw) => arw.thumbnail(),
            RawFile::Cr2(cr2) => cr2.thumbnail(),
            RawFile::Cr3(cr3) => cr3.thumbnail(),
            RawFile::Crw(crw) => crw.thumbnail(),
            RawFile::Dng(dng) => dng.thumbnail(),
            RawFile::Nef(nef) => nef.thumbnail(),
            RawFile::Raf(raf) => raf.thumbnail(),
        }
    }

    /// Decode raw sensor data without processing.
    ///
    /// Returns the raw Bayer/X-Trans CFA data with original bit depth.
    /// For LinearRaw DNG files, returns the already-demosaiced data as a RawImage.
    pub fn decode_raw(&mut self) -> RawResult<RawImage> {
        match self {
            RawFile::Arw(arw) => arw.decode_raw(),
            RawFile::Cr2(cr2) => cr2.decode_raw(),
            RawFile::Cr3(cr3) => cr3.decode_raw(),
            RawFile::Crw(crw) => crw.decode_raw(),
            RawFile::Dng(dng) => dng.decode_raw(),
            RawFile::Nef(nef) => nef.decode_raw(),
            RawFile::Raf(raf) => raf.decode_raw(),
        }
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
            RawFile::Dng(dng) => dng.metadata().map(|m| m.is_linear_raw).unwrap_or(false),
            RawFile::Arw(_)
            | RawFile::Cr2(_)
            | RawFile::Cr3(_)
            | RawFile::Crw(_)
            | RawFile::Nef(_)
            | RawFile::Raf(_) => false,
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
        if raf::is_raf(&header) {
            return Ok(RawFormat::Raf);
        }

        // CR3 detection must come BEFORE the TIFF check because CR3 uses ISOBMFF
        // (not TIFF) and would otherwise be rejected as an unsupported format.
        if cr3::is_cr3(&header) {
            return Ok(RawFormat::Cr3);
        }

        // CRW detection must come BEFORE the TIFF check. CRW uses II/MM + 0x0001
        // (not 0x002A) and has "HEAPCCDR" at bytes 6..14. A standard TIFF parser
        // would reject it because the magic number is wrong.
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
        if cr2::is_cr2(&header) {
            return Ok(RawFormat::Cr2);
        }

        // Parse as TIFF to inspect Make tag for format detection
        let mut parser = TiffParser::new(reader)?;
        let ifd0 = parser.parse_ifd0()?;

        // Check for DNG version first - if present, it's a DNG regardless of Make
        if ifd0.get(TiffTag::DNGVersion).is_some() {
            return Ok(RawFormat::Dng);
        }

        // Check Make tag to determine specific format
        if let Some(make_entry) = ifd0.get(TiffTag::Make) {
            if let Ok(value) = parser.read_value(make_entry) {
                if let Some(make) = value.as_str() {
                    let make_lower = make.to_lowercase();
                    if make_lower.contains("sony") {
                        return Ok(RawFormat::Arw);
                    }
                    // Add more manufacturers here as we add support
                    if make_lower.contains("nikon") {
                        return Ok(RawFormat::Nef);
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

/// Encode a linear RGB image to a file with optional EXIF/ICC metadata.
///
/// This is a standalone function extracted from `RawFile::export()` so that
/// tests and callers can encode synthetic or pre-decoded images without going
/// through the full RAW decode pipeline.
///
/// `image` must contain 16-bit scene-linear RGB data normalized to [0, 65535].
/// Call `apply_tonemap` first if the image hasn't been tone-mapped yet.
pub fn encode_rgb_image(
    image: &RgbImage,
    metadata: &ImageMetadata,
    path: &Path,
    encode_options: &EncodeOptions,
) -> RawResult<()> {
    match encode_options {
        EncodeOptions::Png(opts) => {
            use zune_core::colorspace::ColorSpace;
            use zune_core::options::EncoderOptions;
            use zune_png::PngEncoder;

            let options = EncoderOptions::default()
                .set_width(image.width() as usize)
                .set_height(image.height() as usize)
                .set_colorspace(ColorSpace::RGB)
                .set_depth(opts.bit_depth);

            let data_bytes = if opts.bit_depth == zune_core::bit_depth::BitDepth::Sixteen {
                let mut bytes = Vec::with_capacity(image.data.len() * 2);
                for &pixel in &image.data {
                    bytes.extend_from_slice(&pixel.to_be_bytes());
                }
                bytes
            } else {
                let mut bytes = Vec::with_capacity(image.data.len());
                for &pixel in &image.data {
                    bytes.push((pixel >> 8) as u8);
                }
                bytes
            };

            let mut encoder = PngEncoder::new(&data_bytes, options);
            let mut output = Vec::new();
            encoder.encode(&mut output).map_err(|e| {
                RawError::Encode(EncodeError::Encoding {
                    format: "PNG",
                    message: format!("PNG encoding error: {:?}", e),
                })
            })?;
            let mut file = std::fs::File::create(path)?;
            use std::io::Write;
            file.write_all(&output)?;
        }
        EncodeOptions::Jpeg(opts) => {
            use crate::metadata::exif::ExifBuilder;
            use crate::metadata::icc::IccProfile;
            use jpeg_encoder::{ColorType, Encoder};

            let mut data_8bit = Vec::with_capacity(image.data.len());
            for &pixel in &image.data {
                data_8bit.push((pixel >> 8) as u8);
            }

            let quality = if opts.quality == 0 { 90 } else { opts.quality };
            let encoder = Encoder::new_file(path, quality)?;
            encoder.encode(
                &data_8bit,
                image.width() as u16,
                image.height() as u16,
                ColorType::Rgb,
            )?;

            if opts.embed_exif || opts.embed_icc {
                let mut jpeg_data = std::fs::read(path)?;

                if opts.embed_exif {
                    let exif_builder = ExifBuilder::new(metadata);
                    match exif_builder.append_to_jpeg(jpeg_data.clone()) {
                        Ok(data) => jpeg_data = data,
                        Err(e) => tracing::warn!("Failed to embed EXIF: {}", e),
                    }
                }

                if opts.embed_icc {
                    let icc = IccProfile::srgb();
                    match icc.append_to_jpeg(jpeg_data.clone()) {
                        Ok(data) => jpeg_data = data,
                        Err(e) => tracing::warn!("Failed to embed ICC: {}", e),
                    }
                }

                std::fs::write(path, jpeg_data)?;
            }
        }
        EncodeOptions::WebP(opts) => {
            use crate::codecs::webp::{build_webp_config, encode_webp_rgb, mux_webp};
            use crate::formats::export::WebPMode;
            use crate::metadata::exif::ExifBuilder;
            use crate::metadata::icc::IccProfile;

            let lossless = opts.mode == WebPMode::Lossless;
            let config = build_webp_config(lossless, opts.quality, opts.method, opts.near_lossless)
                .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            let mut data_8bit = Vec::with_capacity(image.data.len());
            for &pixel in &image.data {
                data_8bit.push((pixel >> 8) as u8);
            }

            let encoded = encode_webp_rgb(&data_8bit, image.width(), image.height(), &config)
                .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            let exif_bytes = if opts.embed_exif {
                let exif_builder = ExifBuilder::new(metadata);
                match exif_builder.build_bytes() {
                    Ok(bytes) => Some(bytes),
                    Err(e) => {
                        tracing::warn!("Failed to build EXIF for WebP: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            let icc_bytes = if opts.embed_icc {
                Some(IccProfile::srgb().as_bytes().to_vec())
            } else {
                None
            };

            let xmp_bytes = if opts.embed_xmp {
                metadata.xmp.as_deref()
            } else {
                None
            };

            let output = mux_webp(
                &encoded,
                exif_bytes.as_deref(),
                icc_bytes.as_deref(),
                xmp_bytes,
            )
            .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            std::fs::write(path, output)?;
        }
        #[cfg(feature = "avif")]
        EncodeOptions::Avif(opts) => {
            use crate::metadata::exif::ExifBuilder;
            use ravif::{Encoder, Img, RGBA8};

            let rgba_data: Vec<RGBA8> = image
                .data
                .chunks(3)
                .map(|rgb| {
                    RGBA8::new(
                        (rgb[0] >> 8) as u8,
                        (rgb[1] >> 8) as u8,
                        (rgb[2] >> 8) as u8,
                        255,
                    )
                })
                .collect();

            let img = Img::new(
                rgba_data.as_slice(),
                image.width() as usize,
                image.height() as usize,
            );

            let encoder = Encoder::new()
                .with_quality(opts.quality as f32)
                .with_speed(opts.speed);

            let result = encoder.encode_rgba(img).expect("Encode AVIF");
            std::fs::write(path, result.avif_file)?;

            if opts.embed_exif {
                let exif_builder = ExifBuilder::new(metadata);
                if let Err(e) = exif_builder.append_to_avif_file(path) {
                    tracing::warn!("Failed to embed EXIF in AVIF: {}", e);
                }
            }

            if opts.embed_icc {
                tracing::debug!(
                    "ICC profile embedding in AVIF is not yet supported by the encoder."
                );
            }
        }
        #[cfg(feature = "jxl-encode")]
        EncodeOptions::Jxl(opts) => {
            use zune_core::colorspace::ColorSpace;
            use zune_core::options::EncoderOptions;
            use zune_jpegxl::JxlSimpleEncoder;

            let data_8bit: Vec<u8> = image.data.iter().map(|&p| (p >> 8) as u8).collect();

            let quality = if opts.quality == 0.0 {
                100
            } else {
                opts.quality as u8
            };
            let enc_options = EncoderOptions::default()
                .set_width(image.width() as usize)
                .set_height(image.height() as usize)
                .set_colorspace(ColorSpace::RGB)
                .set_quality(quality);

            let encoder = JxlSimpleEncoder::new(&data_8bit, enc_options);
            let mut encoded: Vec<u8> = Vec::new();
            encoder.encode(&mut encoded).expect("Encode JXL");
            std::fs::write(path, &encoded)?;

            if opts.embed_exif {
                use crate::metadata::exif::ExifBuilder;
                let exif_builder = ExifBuilder::new(metadata);
                if let Err(e) = exif_builder.append_to_jxl_file(path) {
                    tracing::warn!("Failed to embed EXIF in JXL: {}", e);
                }
            }
        }
        EncodeOptions::Dng(config) => {
            export_dng(path, image, metadata, config)?;
        }
    }

    Ok(())
}

/// Encode a linear RGB image to a writer with optional EXIF/ICC metadata.
///
/// Like [`encode_rgb_image`] but writes to any `Write` implementor instead of a file path.
/// Currently supports PNG, JPEG, and WebP. For formats requiring post-processing
/// (DNG, AVIF, JXL), use [`encode_rgb_image`] with a file path.
pub fn encode_rgb_image_to_writer<W: std::io::Write>(
    image: &RgbImage,
    metadata: &ImageMetadata,
    writer: &mut W,
    encode_options: &EncodeOptions,
) -> RawResult<()> {
    match encode_options {
        EncodeOptions::Png(opts) => {
            use zune_core::colorspace::ColorSpace;
            use zune_core::options::EncoderOptions;
            use zune_png::PngEncoder;

            let options = EncoderOptions::default()
                .set_width(image.width() as usize)
                .set_height(image.height() as usize)
                .set_colorspace(ColorSpace::RGB)
                .set_depth(opts.bit_depth);

            let data_bytes = if opts.bit_depth == zune_core::bit_depth::BitDepth::Sixteen {
                let mut bytes = Vec::with_capacity(image.data.len() * 2);
                for &pixel in &image.data {
                    bytes.extend_from_slice(&pixel.to_be_bytes());
                }
                bytes
            } else {
                let mut bytes = Vec::with_capacity(image.data.len());
                for &pixel in &image.data {
                    bytes.push((pixel >> 8) as u8);
                }
                bytes
            };

            let mut encoder = PngEncoder::new(&data_bytes, options);
            let mut output = Vec::new();
            encoder.encode(&mut output).map_err(|e| {
                RawError::Encode(EncodeError::Encoding {
                    format: "PNG",
                    message: format!("PNG encoding error: {:?}", e),
                })
            })?;
            writer.write_all(&output)?;
        }
        EncodeOptions::Jpeg(opts) => {
            use crate::metadata::exif::ExifBuilder;
            use crate::metadata::icc::IccProfile;
            use jpeg_encoder::{ColorType, Encoder};

            let mut data_8bit = Vec::with_capacity(image.data.len());
            for &pixel in &image.data {
                data_8bit.push((pixel >> 8) as u8);
            }

            let quality = if opts.quality == 0 { 90 } else { opts.quality };
            let mut jpeg_buf = Vec::new();
            let encoder = Encoder::new(&mut jpeg_buf, quality);
            encoder.encode(
                &data_8bit,
                image.width() as u16,
                image.height() as u16,
                ColorType::Rgb,
            )?;

            if opts.embed_exif {
                let exif_builder = ExifBuilder::new(metadata);
                match exif_builder.append_to_jpeg(jpeg_buf.clone()) {
                    Ok(data) => jpeg_buf = data,
                    Err(e) => tracing::warn!("Failed to embed EXIF: {}", e),
                }
            }

            if opts.embed_icc {
                let icc = IccProfile::srgb();
                match icc.append_to_jpeg(jpeg_buf.clone()) {
                    Ok(data) => jpeg_buf = data,
                    Err(e) => tracing::warn!("Failed to embed ICC: {}", e),
                }
            }

            writer.write_all(&jpeg_buf)?;
        }
        EncodeOptions::WebP(opts) => {
            use crate::codecs::webp::{build_webp_config, encode_webp_rgb, mux_webp};
            use crate::formats::export::WebPMode;
            use crate::metadata::exif::ExifBuilder;
            use crate::metadata::icc::IccProfile;

            let lossless = opts.mode == WebPMode::Lossless;
            let config = build_webp_config(lossless, opts.quality, opts.method, opts.near_lossless)
                .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            let mut data_8bit = Vec::with_capacity(image.data.len());
            for &pixel in &image.data {
                data_8bit.push((pixel >> 8) as u8);
            }

            let encoded = encode_webp_rgb(&data_8bit, image.width(), image.height(), &config)
                .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            let exif_bytes = if opts.embed_exif {
                let exif_builder = ExifBuilder::new(metadata);
                match exif_builder.build_bytes() {
                    Ok(bytes) => Some(bytes),
                    Err(e) => {
                        tracing::warn!("Failed to build EXIF for WebP: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            let icc_bytes = if opts.embed_icc {
                Some(IccProfile::srgb().as_bytes().to_vec())
            } else {
                None
            };

            let xmp_bytes = if opts.embed_xmp {
                metadata.xmp.as_deref()
            } else {
                None
            };

            let output = mux_webp(
                &encoded,
                exif_bytes.as_deref(),
                icc_bytes.as_deref(),
                xmp_bytes,
            )
            .map_err(|e| RawError::Encode(EncodeError::WebP(e)))?;

            writer.write_all(&output)?;
        }
        _ => {
            return Err(RawError::Unsupported(
                "This format does not support writing to a generic writer. Use encode_rgb_image() with a file path.".to_string(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
