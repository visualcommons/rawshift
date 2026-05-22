//! Ground truth structures for test validation.
//!
//! These structures match the JSON annotation schema defined for test files.
//! Some fields are reserved for future use.

#![allow(dead_code)]

use serde::Deserialize;

/// Complete ground truth for a RAW file.
#[derive(Debug, Deserialize)]
pub struct GroundTruth {
    pub file_name: String,
    pub file_size: u64,
    pub structure: StructureInfo,
    pub primary_raw_frame: RawFrameInfo,
    #[serde(default)]
    pub strip_data: Option<StripData>,
    #[serde(default)]
    pub tile_data: Option<TileData>,
    #[serde(default)]
    pub camera_info: Option<CameraInfo>,
    #[serde(default)]
    pub crop_info: Option<CropInfo>,
    #[serde(default)]
    pub levels: Option<LevelsInfo>,
}

/// TIFF structure information.
#[derive(Debug, Deserialize)]
pub struct StructureInfo {
    pub byte_order: String,
    pub is_big_tiff: bool,
    pub ifd_count: usize,
}

/// Primary raw frame information.
#[derive(Debug, Deserialize)]
pub struct RawFrameInfo {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub compression: u16,
    pub photometric_interp: u16,
    #[serde(default)]
    pub cfa_pattern: Option<Vec<u8>>,
    #[serde(default)]
    pub cfa_pattern_dim: Option<Vec<u8>>,
}

/// Strip-based data information.
#[derive(Debug, Deserialize)]
pub struct StripData {
    pub strip_offsets: Vec<u64>,
    pub strip_byte_counts: Vec<u64>,
}

/// Tile-based data information.
#[derive(Debug, Deserialize)]
pub struct TileData {
    pub tile_width: u32,
    pub tile_length: u32,
}

/// Camera information.
#[derive(Debug, Deserialize)]
pub struct CameraInfo {
    pub make: String,
    pub model: String,
    #[serde(default)]
    pub raw_file_type: Option<String>,
}

/// Crop information.
#[derive(Debug, Deserialize)]
pub struct CropInfo {
    pub default_crop_origin: Vec<u32>,
    pub default_crop_size: Vec<u32>,
}

/// Black/white level information.
#[derive(Debug, Deserialize)]
pub struct LevelsInfo {
    pub black_level: Vec<u16>,
    pub white_level: u16,
}

/// Load a ground truth JSON file.
pub fn load_ground_truth(
    path: &std::path::Path,
) -> Result<GroundTruth, Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(path)?;
    let gt: GroundTruth = serde_json::from_str(&contents)?;
    Ok(gt)
}

/// Get the path to a test data file.
pub fn test_data_path(filename: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_data")
        .join(filename)
}

/// Get the path to a test fixture file.
pub fn test_fixture_path(filename: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_fixtures")
        .join(filename)
}
