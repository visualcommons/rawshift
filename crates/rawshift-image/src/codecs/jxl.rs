//! JPEG XL decoder for DNG 1.7+ tile data.
//!
//! This module wraps the jxl-oxide crate to decode JPEG XL compressed
//! tiles used in DNG 1.7+ files (compression code 52546).

/// JPEG XL tile decoder for DNG files.
pub struct JxlDecoder;

impl JxlDecoder {
    /// Decode a single JPEG XL tile to u16 RGB data.
    ///
    /// The decoder handles both lossless and lossy JXL compression.
    /// Output is 16-bit values per channel, scaled from the internal f32 representation.
    ///
    /// Returns (width, height, channels, pixel_data).
    pub fn decode_tile(data: &[u8]) -> crate::error::RawResult<(usize, usize, usize, Vec<u16>)> {
        use crate::error::{FormatError, RawError};
        use jxl_oxide::JxlImage;

        let image = JxlImage::builder()
            .read(std::io::Cursor::new(data))
            .map_err(|e| {
                RawError::Format(FormatError::Decompression(format!(
                    "JXL decode error: {}",
                    e
                )))
            })?;

        let width = image.width() as usize;
        let height = image.height() as usize;

        // Render the first frame
        let render = image.render_frame(0).map_err(|e| {
            RawError::Format(FormatError::Decompression(format!(
                "JXL render error: {}",
                e
            )))
        })?;

        // Get interleaved pixel data
        let fb = render.image_all_channels();
        let channels = fb.channels();
        let buf = fb.buf();

        // Convert f32 [0, 1] to u16 [0, 65535]
        let output: Vec<u16> = buf
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 65535.0) as u16)
            .collect();

        Ok((width, height, channels, output))
    }

    /// Decode a JPEG XL tile and return u16 data with specified target bit depth.
    ///
    /// This handles bit depth conversion for raw sensor data where
    /// pixel values are stored at a specific bit depth (e.g., 10-bit, 14-bit).
    ///
    /// Returns (width, height, channels, pixel_data).
    pub fn decode_tile_with_depth(
        data: &[u8],
        target_bit_depth: u8,
    ) -> crate::error::RawResult<(usize, usize, usize, Vec<u16>)> {
        use crate::error::{FormatError, RawError};
        use jxl_oxide::JxlImage;

        let image = JxlImage::builder()
            .read(std::io::Cursor::new(data))
            .map_err(|e| {
                RawError::Format(FormatError::Decompression(format!(
                    "JXL decode error: {}",
                    e
                )))
            })?;

        let width = image.width() as usize;
        let height = image.height() as usize;

        let render = image.render_frame(0).map_err(|e| {
            RawError::Format(FormatError::Decompression(format!(
                "JXL render error: {}",
                e
            )))
        })?;

        let fb = render.image_all_channels();
        let channels = fb.channels();
        let buf = fb.buf();

        // Target max value based on bit depth
        let max_value = (1u32 << target_bit_depth) - 1;

        let output: Vec<u16> = buf
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * max_value as f32) as u16)
            .collect();

        Ok((width, height, channels, output))
    }
}

#[cfg(test)]
mod tests {
    use super::JxlDecoder;

    #[test]
    fn test_jxl_decoder_invalid_data() {
        let invalid_data = vec![0u8; 10];
        let result = JxlDecoder::decode_tile(&invalid_data);
        assert!(result.is_err());
    }
}
