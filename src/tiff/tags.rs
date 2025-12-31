//! TIFF tag definitions.
//!
//! This module defines known TIFF tags used in RAW image formats,
//! including standard TIFF/EXIF tags and format-specific extensions.

use binrw::{BinRead, BinWrite};
use std::fmt;

/// Known TIFF tag IDs.
///
/// This enum contains tags commonly found in TIFF-based RAW formats.
/// Unknown tags are handled separately via the parser's unknown tag storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, BinRead, BinWrite)]
#[brw(repr = u16)]
#[repr(u16)]
pub enum TiffTag {
    // ========================================
    // Baseline TIFF Tags
    // ========================================
    /// Image width in pixels (0x0100)
    ImageWidth = 0x0100,
    /// Image height/length in pixels (0x0101)
    ImageLength = 0x0101,
    /// Bits per sample/channel (0x0102)
    BitsPerSample = 0x0102,
    /// Compression scheme (0x0103)
    Compression = 0x0103,
    /// Photometric interpretation (0x0106)
    PhotometricInterpretation = 0x0106,
    /// Image description (0x010E)
    ImageDescription = 0x010E,
    /// Camera/scanner manufacturer (0x010F)
    Make = 0x010F,
    /// Camera/scanner model (0x0110)
    Model = 0x0110,
    /// Byte offsets to image strips (0x0111)
    StripOffsets = 0x0111,
    /// Image orientation (0x0112)
    Orientation = 0x0112,
    /// Samples per pixel (0x0115)
    SamplesPerPixel = 0x0115,
    /// Rows per strip (0x0116)
    RowsPerStrip = 0x0116,
    /// Byte counts for strips (0x0117)
    StripByteCounts = 0x0117,
    /// X resolution (0x011A)
    XResolution = 0x011A,
    /// Y resolution (0x011B)
    YResolution = 0x011B,
    /// Planar configuration (0x011C)
    PlanarConfiguration = 0x011C,
    /// Resolution unit (0x0128)
    ResolutionUnit = 0x0128,
    /// Software used (0x0131)
    Software = 0x0131,
    /// Date/time of modification (0x0132)
    DateTime = 0x0132,
    /// Predictor for compression (0x013D)
    Predictor = 0x013D,
    /// White point chromaticity (0x013E)
    WhitePoint = 0x013E,
    /// Primary chromaticities (0x013F)
    PrimaryChromaticities = 0x013F,
    /// Tile width (0x0142)
    TileWidth = 0x0142,
    /// Tile length/height (0x0143)
    TileLength = 0x0143,
    /// Byte offsets to tiles (0x0144)
    TileOffsets = 0x0144,
    /// Byte counts for tiles (0x0145)
    TileByteCounts = 0x0145,
    /// Offsets to SubIFDs (0x014A) - critical for RAW formats
    SubIFDs = 0x014A,
    /// JPEG tables (0x015B)
    JPEGTables = 0x015B,
    /// YCbCr coefficients (0x0211)
    YCbCrCoefficients = 0x0211,
    /// YCbCr subsampling (0x0212)
    YCbCrSubSampling = 0x0212,
    /// YCbCr positioning (0x0213)
    YCbCrPositioning = 0x0213,
    /// Reference black/white (0x0214)
    ReferenceBlackWhite = 0x0214,

    // ========================================
    // EXIF Tags
    // ========================================
    /// Pointer to EXIF IFD (0x8769)
    ExifIFDPointer = 0x8769,
    /// Pointer to GPS IFD (0x8825)
    GPSInfoIFDPointer = 0x8825,

    // ========================================
    // CFA (Color Filter Array) Tags
    // ========================================
    /// CFA repeat pattern dimensions (0x828D)
    CFARepeatPatternDim = 0x828D,
    /// CFA pattern (0x828E)
    CFAPattern = 0x828E,

    // ========================================
    // DNG Tags
    // ========================================
    /// DNG version (0xC612)
    DNGVersion = 0xC612,
    /// DNG backward version (0xC613)
    DNGBackwardVersion = 0xC613,
    /// Unique camera model (0xC614)
    UniqueCameraModel = 0xC614,
    /// Localized camera model (0xC615)
    LocalizedCameraModel = 0xC615,
    /// CFA plane color (0xC616)
    CFAPlaneColor = 0xC616,
    /// CFA layout (0xC617)
    CFALayout = 0xC617,
    /// Linearization table (0xC618)
    LinearizationTable = 0xC618,
    /// Black level repeat dim (0xC619)
    BlackLevelRepeatDim = 0xC619,
    /// Black level (0xC61A)
    BlackLevel = 0xC61A,
    /// Black level delta H (0xC61B)
    BlackLevelDeltaH = 0xC61B,
    /// Black level delta V (0xC61C)
    BlackLevelDeltaV = 0xC61C,
    /// White level (0xC61D)
    WhiteLevel = 0xC61D,
    /// Default scale (0xC61E)
    DefaultScale = 0xC61E,
    /// Default crop origin (0xC61F)
    DefaultCropOrigin = 0xC61F,
    /// Default crop size (0xC620)
    DefaultCropSize = 0xC620,
    /// Color matrix 1 (0xC621)
    ColorMatrix1 = 0xC621,
    /// Color matrix 2 (0xC622)
    ColorMatrix2 = 0xC622,
    /// Camera calibration 1 (0xC623)
    CameraCalibration1 = 0xC623,
    /// Camera calibration 2 (0xC624)
    CameraCalibration2 = 0xC624,
    /// Analog balance (0xC627)
    AnalogBalance = 0xC627,
    /// As shot neutral (0xC628)
    AsShotNeutral = 0xC628,
    /// As shot white XY (0xC629)
    AsShotWhiteXY = 0xC629,
    /// Baseline exposure (0xC62A)
    BaselineExposure = 0xC62A,
    /// Baseline noise (0xC62B)
    BaselineNoise = 0xC62B,
    /// Baseline sharpness (0xC62C)
    BaselineSharpness = 0xC62C,
    /// Active area (0xC68D)
    ActiveArea = 0xC68D,
    /// Masked areas (0xC68E)
    MaskedAreas = 0xC68E,
    /// Opcode list 1 (0xC740)
    OpcodeList1 = 0xC740,
    /// Opcode list 2 (0xC741)
    OpcodeList2 = 0xC741,
    /// Opcode list 3 (0xC74E)
    OpcodeList3 = 0xC74E,
    /// New subfile type (0x00FE)
    NewSubfileType = 0x00FE,

    // ========================================
    // Sony-Specific Tags
    // ========================================
    /// Sony raw file type (0x7200)
    SonyRawFileType = 0x7200,
    /// Sony tone curve (0x7010)
    SonyToneCurve = 0x7010,
    /// Sony crop top/left (0x74C7)
    SonyCropTopLeft = 0x74C7,
    /// Sony crop size (0x74C8)
    SonyCropSize = 0x74C8,
    /// Sony SR2 SubIFD length
    SR2SubIFDLength = 0x7201,
    /// Sony SR2 SubIFD key
    SR2SubIFDKey = 0x7221,

    // ========================================
    // Preview/Thumbnail Tags
    // ========================================
    /// JPEG interchange format offset (0x0201)
    JPEGInterchangeFormat = 0x0201,
    /// JPEG interchange format length (0x0202)
    JPEGInterchangeFormatLength = 0x0202,
}

impl TiffTag {
    /// Create a TiffTag from a raw u16 tag ID.
    /// Returns None if the tag is not known.
    pub fn from_u16(value: u16) -> Option<Self> {
        // Use a match for known tags
        match value {
            0x00FE => Some(TiffTag::NewSubfileType),
            0x0100 => Some(TiffTag::ImageWidth),
            0x0101 => Some(TiffTag::ImageLength),
            0x0102 => Some(TiffTag::BitsPerSample),
            0x0103 => Some(TiffTag::Compression),
            0x0106 => Some(TiffTag::PhotometricInterpretation),
            0x010E => Some(TiffTag::ImageDescription),
            0x010F => Some(TiffTag::Make),
            0x0110 => Some(TiffTag::Model),
            0x0111 => Some(TiffTag::StripOffsets),
            0x0112 => Some(TiffTag::Orientation),
            0x0115 => Some(TiffTag::SamplesPerPixel),
            0x0116 => Some(TiffTag::RowsPerStrip),
            0x0117 => Some(TiffTag::StripByteCounts),
            0x011A => Some(TiffTag::XResolution),
            0x011B => Some(TiffTag::YResolution),
            0x011C => Some(TiffTag::PlanarConfiguration),
            0x0128 => Some(TiffTag::ResolutionUnit),
            0x0131 => Some(TiffTag::Software),
            0x0132 => Some(TiffTag::DateTime),
            0x013D => Some(TiffTag::Predictor),
            0x013E => Some(TiffTag::WhitePoint),
            0x013F => Some(TiffTag::PrimaryChromaticities),
            0x0142 => Some(TiffTag::TileWidth),
            0x0143 => Some(TiffTag::TileLength),
            0x0144 => Some(TiffTag::TileOffsets),
            0x0145 => Some(TiffTag::TileByteCounts),
            0x014A => Some(TiffTag::SubIFDs),
            0x015B => Some(TiffTag::JPEGTables),
            0x0201 => Some(TiffTag::JPEGInterchangeFormat),
            0x0202 => Some(TiffTag::JPEGInterchangeFormatLength),
            0x0211 => Some(TiffTag::YCbCrCoefficients),
            0x0212 => Some(TiffTag::YCbCrSubSampling),
            0x0213 => Some(TiffTag::YCbCrPositioning),
            0x0214 => Some(TiffTag::ReferenceBlackWhite),
            0x7010 => Some(TiffTag::SonyToneCurve),
            0x7200 => Some(TiffTag::SonyRawFileType),
            0x7201 => Some(TiffTag::SR2SubIFDLength),
            0x7221 => Some(TiffTag::SR2SubIFDKey),
            0x74C7 => Some(TiffTag::SonyCropTopLeft),
            0x74C8 => Some(TiffTag::SonyCropSize),
            0x828D => Some(TiffTag::CFARepeatPatternDim),
            0x828E => Some(TiffTag::CFAPattern),
            0x8769 => Some(TiffTag::ExifIFDPointer),
            0x8825 => Some(TiffTag::GPSInfoIFDPointer),
            0xC612 => Some(TiffTag::DNGVersion),
            0xC613 => Some(TiffTag::DNGBackwardVersion),
            0xC614 => Some(TiffTag::UniqueCameraModel),
            0xC615 => Some(TiffTag::LocalizedCameraModel),
            0xC616 => Some(TiffTag::CFAPlaneColor),
            0xC617 => Some(TiffTag::CFALayout),
            0xC618 => Some(TiffTag::LinearizationTable),
            0xC619 => Some(TiffTag::BlackLevelRepeatDim),
            0xC61A => Some(TiffTag::BlackLevel),
            0xC61B => Some(TiffTag::BlackLevelDeltaH),
            0xC61C => Some(TiffTag::BlackLevelDeltaV),
            0xC61D => Some(TiffTag::WhiteLevel),
            0xC61E => Some(TiffTag::DefaultScale),
            0xC61F => Some(TiffTag::DefaultCropOrigin),
            0xC620 => Some(TiffTag::DefaultCropSize),
            0xC621 => Some(TiffTag::ColorMatrix1),
            0xC622 => Some(TiffTag::ColorMatrix2),
            0xC623 => Some(TiffTag::CameraCalibration1),
            0xC624 => Some(TiffTag::CameraCalibration2),
            0xC627 => Some(TiffTag::AnalogBalance),
            0xC628 => Some(TiffTag::AsShotNeutral),
            0xC629 => Some(TiffTag::AsShotWhiteXY),
            0xC62A => Some(TiffTag::BaselineExposure),
            0xC62B => Some(TiffTag::BaselineNoise),
            0xC62C => Some(TiffTag::BaselineSharpness),
            0xC68D => Some(TiffTag::ActiveArea),
            0xC68E => Some(TiffTag::MaskedAreas),
            0xC740 => Some(TiffTag::OpcodeList1),
            0xC741 => Some(TiffTag::OpcodeList2),
            0xC74E => Some(TiffTag::OpcodeList3),
            _ => None,
        }
    }

    /// Get the raw u16 value of this tag.
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    /// Get a human-readable name for this tag.
    pub fn name(&self) -> &'static str {
        match self {
            TiffTag::NewSubfileType => "NewSubfileType",
            TiffTag::ImageWidth => "ImageWidth",
            TiffTag::ImageLength => "ImageLength",
            TiffTag::BitsPerSample => "BitsPerSample",
            TiffTag::Compression => "Compression",
            TiffTag::PhotometricInterpretation => "PhotometricInterpretation",
            TiffTag::ImageDescription => "ImageDescription",
            TiffTag::Make => "Make",
            TiffTag::Model => "Model",
            TiffTag::StripOffsets => "StripOffsets",
            TiffTag::Orientation => "Orientation",
            TiffTag::SamplesPerPixel => "SamplesPerPixel",
            TiffTag::RowsPerStrip => "RowsPerStrip",
            TiffTag::StripByteCounts => "StripByteCounts",
            TiffTag::XResolution => "XResolution",
            TiffTag::YResolution => "YResolution",
            TiffTag::PlanarConfiguration => "PlanarConfiguration",
            TiffTag::ResolutionUnit => "ResolutionUnit",
            TiffTag::Software => "Software",
            TiffTag::DateTime => "DateTime",
            TiffTag::Predictor => "Predictor",
            TiffTag::WhitePoint => "WhitePoint",
            TiffTag::PrimaryChromaticities => "PrimaryChromaticities",
            TiffTag::TileWidth => "TileWidth",
            TiffTag::TileLength => "TileLength",
            TiffTag::TileOffsets => "TileOffsets",
            TiffTag::TileByteCounts => "TileByteCounts",
            TiffTag::SubIFDs => "SubIFDs",
            TiffTag::JPEGTables => "JPEGTables",
            TiffTag::YCbCrCoefficients => "YCbCrCoefficients",
            TiffTag::YCbCrSubSampling => "YCbCrSubSampling",
            TiffTag::YCbCrPositioning => "YCbCrPositioning",
            TiffTag::ReferenceBlackWhite => "ReferenceBlackWhite",
            TiffTag::ExifIFDPointer => "ExifIFDPointer",
            TiffTag::GPSInfoIFDPointer => "GPSInfoIFDPointer",
            TiffTag::CFARepeatPatternDim => "CFARepeatPatternDim",
            TiffTag::CFAPattern => "CFAPattern",
            TiffTag::DNGVersion => "DNGVersion",
            TiffTag::DNGBackwardVersion => "DNGBackwardVersion",
            TiffTag::UniqueCameraModel => "UniqueCameraModel",
            TiffTag::LocalizedCameraModel => "LocalizedCameraModel",
            TiffTag::CFAPlaneColor => "CFAPlaneColor",
            TiffTag::CFALayout => "CFALayout",
            TiffTag::LinearizationTable => "LinearizationTable",
            TiffTag::BlackLevelRepeatDim => "BlackLevelRepeatDim",
            TiffTag::BlackLevel => "BlackLevel",
            TiffTag::BlackLevelDeltaH => "BlackLevelDeltaH",
            TiffTag::BlackLevelDeltaV => "BlackLevelDeltaV",
            TiffTag::WhiteLevel => "WhiteLevel",
            TiffTag::DefaultScale => "DefaultScale",
            TiffTag::DefaultCropOrigin => "DefaultCropOrigin",
            TiffTag::DefaultCropSize => "DefaultCropSize",
            TiffTag::ColorMatrix1 => "ColorMatrix1",
            TiffTag::ColorMatrix2 => "ColorMatrix2",
            TiffTag::CameraCalibration1 => "CameraCalibration1",
            TiffTag::CameraCalibration2 => "CameraCalibration2",
            TiffTag::AnalogBalance => "AnalogBalance",
            TiffTag::AsShotNeutral => "AsShotNeutral",
            TiffTag::AsShotWhiteXY => "AsShotWhiteXY",
            TiffTag::BaselineExposure => "BaselineExposure",
            TiffTag::BaselineNoise => "BaselineNoise",
            TiffTag::BaselineSharpness => "BaselineSharpness",
            TiffTag::ActiveArea => "ActiveArea",
            TiffTag::MaskedAreas => "MaskedAreas",
            TiffTag::OpcodeList1 => "OpcodeList1",
            TiffTag::OpcodeList2 => "OpcodeList2",
            TiffTag::OpcodeList3 => "OpcodeList3",
            TiffTag::SonyRawFileType => "SonyRawFileType",
            TiffTag::SonyToneCurve => "SonyToneCurve",
            TiffTag::SonyCropTopLeft => "SonyCropTopLeft",
            TiffTag::SonyCropSize => "SonyCropSize",
            TiffTag::SR2SubIFDLength => "SR2SubIFDLength",
            TiffTag::SR2SubIFDKey => "SR2SubIFDKey",
            TiffTag::JPEGInterchangeFormat => "JPEGInterchangeFormat",
            TiffTag::JPEGInterchangeFormatLength => "JPEGInterchangeFormatLength",
        }
    }
}

impl fmt::Display for TiffTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (0x{:04X})", self.name(), self.as_u16())
    }
}

/// Compression type values for the Compression tag (0x0103).
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(repr = u16)]
#[repr(u16)]
pub enum CompressionType {
    /// No compression (1)
    Uncompressed = 1,
    /// CCITT Group 3 (2)
    CcittGroup3 = 2,
    /// CCITT Group 4 (4)
    CcittGroup4 = 4,
    /// LZW (5)
    Lzw = 5,
    /// Old-style JPEG (6)
    OldJpeg = 6,
    /// JPEG (7)
    Jpeg = 7,
    /// Adobe Deflate (8)
    AdobeDeflate = 8,
    /// PackBits (32773)
    PackBits = 32773,
    /// Sony ARW compressed (32767)
    SonyArwCompressed = 32767,
    /// Lossy JPEG (34892, used in DNG)
    LossyJpeg = 34892,
}

impl CompressionType {
    /// Parse from u16.
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(CompressionType::Uncompressed),
            2 => Some(CompressionType::CcittGroup3),
            4 => Some(CompressionType::CcittGroup4),
            5 => Some(CompressionType::Lzw),
            6 => Some(CompressionType::OldJpeg),
            7 => Some(CompressionType::Jpeg),
            8 => Some(CompressionType::AdobeDeflate),
            32773 => Some(CompressionType::PackBits),
            32767 => Some(CompressionType::SonyArwCompressed),
            34892 => Some(CompressionType::LossyJpeg),
            _ => None,
        }
    }
}

/// Photometric interpretation values (0x0106).
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(repr = u16)]
#[repr(u16)]
pub enum PhotometricInterpretation {
    /// WhiteIsZero (0)
    WhiteIsZero = 0,
    /// BlackIsZero (1)
    BlackIsZero = 1,
    /// RGB (2)
    Rgb = 2,
    /// Palette color (3)
    PaletteColor = 3,
    /// Transparency mask (4)
    TransparencyMask = 4,
    /// CMYK (5)
    Cmyk = 5,
    /// YCbCr (6)
    YCbCr = 6,
    /// CIE L*a*b* (8)
    CieLab = 8,
    /// Color Filter Array / Bayer (32803)
    Cfa = 32803,
    /// Linear Raw (34892)
    LinearRaw = 34892,
}

impl PhotometricInterpretation {
    /// Parse from u16.
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(PhotometricInterpretation::WhiteIsZero),
            1 => Some(PhotometricInterpretation::BlackIsZero),
            2 => Some(PhotometricInterpretation::Rgb),
            3 => Some(PhotometricInterpretation::PaletteColor),
            4 => Some(PhotometricInterpretation::TransparencyMask),
            5 => Some(PhotometricInterpretation::Cmyk),
            6 => Some(PhotometricInterpretation::YCbCr),
            8 => Some(PhotometricInterpretation::CieLab),
            32803 => Some(PhotometricInterpretation::Cfa),
            34892 => Some(PhotometricInterpretation::LinearRaw),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_from_u16() {
        assert_eq!(TiffTag::from_u16(0x0100), Some(TiffTag::ImageWidth));
        assert_eq!(TiffTag::from_u16(0x0101), Some(TiffTag::ImageLength));
        assert_eq!(TiffTag::from_u16(0x014A), Some(TiffTag::SubIFDs));
        assert_eq!(TiffTag::from_u16(0xFFFF), None); // Unknown tag
    }

    #[test]
    fn test_tag_as_u16() {
        assert_eq!(TiffTag::ImageWidth.as_u16(), 0x0100);
        assert_eq!(TiffTag::SubIFDs.as_u16(), 0x014A);
    }

    #[test]
    fn test_tag_display() {
        let s = format!("{}", TiffTag::ImageWidth);
        assert!(s.contains("ImageWidth"));
        assert!(s.contains("0x0100"));
    }

    #[test]
    fn test_compression_type() {
        assert_eq!(
            CompressionType::from_u16(1),
            Some(CompressionType::Uncompressed)
        );
        assert_eq!(CompressionType::from_u16(7), Some(CompressionType::Jpeg));
        assert_eq!(CompressionType::from_u16(32803), None); // Not a compression type
    }

    #[test]
    fn test_photometric_interpretation() {
        assert_eq!(
            PhotometricInterpretation::from_u16(32803),
            Some(PhotometricInterpretation::Cfa)
        );
        assert_eq!(
            PhotometricInterpretation::from_u16(2),
            Some(PhotometricInterpretation::Rgb)
        );
    }
}
