//! Platform-independent MediaCodec request preparation and image-plane copy.

#![cfg_attr(not(hwdec_backend = "mediacodec"), allow(dead_code))]

use crate::bitstream::{av1, bits::ParseError, hevc};
use crate::{
    CodecConfig, ColorRange, DecodedFrame, HwCodec, HwDecodeError, PixelFormat, Plane,
    StillDecodeRequest,
};

pub(super) const MIME_HEVC: &str = "video/hevc";
pub(super) const MIME_AV1: &str = "video/av01";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionKey {
    pub codec: HwCodec,
    pub csd0: Vec<u8>,
    pub sequence_header: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
}

#[derive(Debug, Clone)]
pub(super) struct PreparedRequest {
    pub codec: HwCodec,
    pub mime: &'static str,
    pub csd0: Vec<u8>,
    pub access_unit: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub range: ColorRange,
    pub key: SessionKey,
}

fn parse_error(codec: HwCodec, error: ParseError) -> HwDecodeError {
    HwDecodeError::Decode {
        codec,
        message: error.0.to_string(),
    }
}

pub(super) fn prepare(request: &StillDecodeRequest<'_>) -> Result<PreparedRequest, HwDecodeError> {
    match request.config {
        CodecConfig::Hvcc(hvcc) => prepare_hevc(hvcc, request.payload),
        CodecConfig::Av1c(av1c) => prepare_av1(av1c, request.payload),
    }
}

fn prepare_hevc(hvcc_bytes: &[u8], payload: &[u8]) -> Result<PreparedRequest, HwDecodeError> {
    const NAL_VPS: u8 = 32;
    let codec = HwCodec::Hevc;
    let hvcc = hevc::parse_hvcc(hvcc_bytes).map_err(|e| parse_error(codec, e))?;
    let payload_nals = hevc::split_length_prefixed(payload, hvcc.nal_length_size)
        .map_err(|e| parse_error(codec, e))?;

    let mut sps = None;
    let mut has_pps = false;
    let mut has_vps = false;
    let mut has_irap = false;
    let mut csd0 = Vec::new();
    for nal in hvcc
        .nal_units
        .iter()
        .map(Vec::as_slice)
        .chain(payload_nals.iter().copied())
    {
        let kind = hevc::nal_type(nal).map_err(|e| parse_error(codec, e))?;
        match kind {
            NAL_VPS => has_vps = true,
            hevc::NAL_SPS => {
                sps = Some(hevc::parse_sps(nal).map_err(|e| parse_error(codec, e))?);
            }
            hevc::NAL_PPS => {
                hevc::parse_pps(nal).map_err(|e| parse_error(codec, e))?;
                has_pps = true;
            }
            kind if hevc::is_irap(kind) => has_irap = true,
            kind if kind < 32 => {
                return Err(HwDecodeError::Decode {
                    codec,
                    message: "HEVC non-IRAP coded slice is outside the still-picture scope"
                        .to_string(),
                });
            }
            _ => {}
        }
        if matches!(kind, NAL_VPS | hevc::NAL_SPS | hevc::NAL_PPS) {
            csd0.extend_from_slice(&[0, 0, 0, 1]);
            csd0.extend_from_slice(nal);
        }
    }
    let sps = sps.ok_or_else(|| HwDecodeError::Decode {
        codec,
        message: "HEVC stream carries no SPS".to_string(),
    })?;
    if !has_vps || !has_pps || !has_irap {
        let missing = if !has_vps {
            "VPS"
        } else if !has_pps {
            "PPS"
        } else {
            "IRAP coded slice"
        };
        return Err(HwDecodeError::Decode {
            codec,
            message: format!("HEVC stream carries no {missing}"),
        });
    }
    if sps.chroma_format_idc > 1 {
        return Err(HwDecodeError::Decode {
            codec,
            message: "HEVC 4:2:2/4:4:4 is outside the Main/Main10 scope".to_string(),
        });
    }
    let bit_depth = sps.bit_depth_luma.max(sps.bit_depth_chroma);
    if !matches!(bit_depth, 8..=10) {
        return Err(HwDecodeError::Decode {
            codec,
            message: "HEVC bit depth is outside the Main/Main10 scope".to_string(),
        });
    }
    let (width, height) = sps.cropped_size();
    if width == 0 || height == 0 {
        return Err(HwDecodeError::Decode {
            codec,
            message: "HEVC conformance window crops away the frame".to_string(),
        });
    }
    let mut access_unit = Vec::with_capacity(payload.len() + payload_nals.len() * 4);
    for nal in payload_nals {
        access_unit.extend_from_slice(&[0, 0, 0, 1]);
        access_unit.extend_from_slice(nal);
    }
    let range = if sps.video_full_range {
        ColorRange::Full
    } else {
        ColorRange::Limited
    };
    let key = SessionKey {
        codec,
        csd0: csd0.clone(),
        sequence_header: csd0.clone(),
        width,
        height,
        bit_depth,
    };
    Ok(PreparedRequest {
        codec,
        mime: MIME_HEVC,
        csd0,
        access_unit,
        width,
        height,
        bit_depth,
        range,
        key,
    })
}

fn prepare_av1(av1c_bytes: &[u8], payload: &[u8]) -> Result<PreparedRequest, HwDecodeError> {
    let codec = HwCodec::Av1;
    let av1c = av1::parse_av1c(av1c_bytes).map_err(|e| parse_error(codec, e))?;
    if av1c.seq_profile != 0 {
        return Err(HwDecodeError::Decode {
            codec,
            message: "AV1 profile 1/2 is outside the Profile 0 scope".to_string(),
        });
    }
    let picture =
        av1::parse_still_picture(av1c.config_obus, payload).map_err(|e| parse_error(codec, e))?;
    if !(picture.seq.mono_chrome || picture.seq.subsampling_x && picture.seq.subsampling_y) {
        return Err(HwDecodeError::Decode {
            codec,
            message: "AV1 4:2:2/4:4:4 is outside the Profile 0 scope".to_string(),
        });
    }
    if !matches!(picture.seq.bit_depth, 8 | 10) {
        return Err(HwDecodeError::Decode {
            codec,
            message: "AV1 bit depth is outside 8/10".to_string(),
        });
    }
    let width = picture.fh.upscaled_width;
    let height = picture.fh.frame_height;
    if width == 0 || height == 0 {
        return Err(HwDecodeError::Decode {
            codec,
            message: "AV1 frame dimensions are zero".to_string(),
        });
    }
    let sequence_header = av1::split_obus(av1c.config_obus)
        .map_err(|e| parse_error(codec, e))?
        .into_iter()
        .chain(av1::split_obus(payload).map_err(|e| parse_error(codec, e))?)
        .find(|obu| obu.obu_type == av1::OBU_SEQUENCE_HEADER)
        .map(|obu| obu.payload.to_vec())
        .ok_or_else(|| HwDecodeError::Decode {
            codec,
            message: "AV1 stream carries no sequence header".to_string(),
        })?;
    let range = if picture.seq.color_range_full {
        ColorRange::Full
    } else {
        ColorRange::Limited
    };
    // Android's MediaCodec contract defines AV1 csd-0 as the complete
    // AV1CodecConfigurationRecord (`av1C`) data.
    let csd0 = av1c_bytes.to_vec();
    let key = SessionKey {
        codec,
        csd0: csd0.clone(),
        sequence_header,
        width,
        height,
        bit_depth: picture.seq.bit_depth,
    };
    Ok(PreparedRequest {
        codec,
        mime: MIME_AV1,
        csd0,
        access_unit: payload.to_vec(),
        width,
        height,
        bit_depth: picture.seq.bit_depth,
        range,
        key,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImageLayout {
    Yuv420,
    P010,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PlaneView<'a> {
    pub data: &'a [u8],
    pub row_stride: usize,
    pub pixel_stride: usize,
}

#[derive(Debug)]
pub(super) struct ImageView<'a> {
    pub layout: ImageLayout,
    pub width: u32,
    pub height: u32,
    pub crop: (u32, u32, u32, u32),
    pub planes: Vec<PlaneView<'a>>,
}

fn copy_samples(
    plane: PlaneView<'_>,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    sample_bytes: usize,
) -> Option<Vec<u8>> {
    if plane.pixel_stride < sample_bytes || plane.row_stride == 0 {
        return None;
    }
    let capacity = width.checked_mul(height)?.checked_mul(sample_bytes)?;
    let mut out = Vec::with_capacity(capacity);
    for row in 0..height {
        for column in 0..width {
            let start = (y + row)
                .checked_mul(plane.row_stride)?
                .checked_add((x + column).checked_mul(plane.pixel_stride)?)?;
            out.extend_from_slice(plane.data.get(start..start + sample_bytes)?);
        }
    }
    Some(out)
}

pub(super) fn normalize_image(
    image: &ImageView<'_>,
    expected_size: (u32, u32),
    bit_depth: u8,
    range: ColorRange,
) -> Result<DecodedFrame, HwDecodeError> {
    let invalid = |message: &str| HwDecodeError::InvalidFrame {
        message: message.to_string(),
    };
    let (left, top, right, bottom) = image.crop;
    if left > right || top > bottom || right > image.width || bottom > image.height {
        return Err(invalid("AImage crop rectangle is outside the image"));
    }
    let width = right - left;
    let height = bottom - top;
    if (width, height) != expected_size {
        return Err(invalid(
            "AImage crop dimensions differ from the coded picture",
        ));
    }
    if left % 2 != 0 || top % 2 != 0 {
        return Err(invalid("4:2:0 AImage crop origin must be even"));
    }
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = (width.div_ceil(2) as usize, height.div_ceil(2) as usize);
    match image.layout {
        ImageLayout::Yuv420 => {
            if bit_depth != 8 || image.planes.len() != 3 {
                return Err(invalid("YUV_420_888 requires three 8-bit planes"));
            }
            let y = copy_samples(image.planes[0], left as usize, top as usize, w, h, 1)
                .ok_or_else(|| invalid("AImage luma plane is too small"))?;
            let u = copy_samples(
                image.planes[1],
                left as usize / 2,
                top as usize / 2,
                cw,
                ch,
                1,
            )
            .ok_or_else(|| invalid("AImage Cb plane is too small"))?;
            let v = copy_samples(
                image.planes[2],
                left as usize / 2,
                top as usize / 2,
                cw,
                ch,
                1,
            )
            .ok_or_else(|| invalid("AImage Cr plane is too small"))?;
            DecodedFrame::new(
                PixelFormat::I420,
                width,
                height,
                bit_depth,
                range,
                vec![
                    Plane { data: y, stride: w },
                    Plane {
                        data: u,
                        stride: cw,
                    },
                    Plane {
                        data: v,
                        stride: cw,
                    },
                ],
            )
        }
        ImageLayout::P010 => {
            if bit_depth <= 8 || !matches!(image.planes.len(), 2 | 3) {
                return Err(invalid("P010 requires two or three high-bit-depth planes"));
            }
            let y = copy_samples(image.planes[0], left as usize, top as usize, w, h, 2)
                .ok_or_else(|| invalid("AImage P010 luma plane is too small"))?;
            let chroma = if image.planes.len() == 2 {
                copy_samples(
                    image.planes[1],
                    left as usize / 2,
                    top as usize / 2,
                    cw,
                    ch,
                    4,
                )
                .ok_or_else(|| invalid("AImage P010 chroma plane is too small"))?
            } else {
                let u = copy_samples(
                    image.planes[1],
                    left as usize / 2,
                    top as usize / 2,
                    cw,
                    ch,
                    2,
                )
                .ok_or_else(|| invalid("AImage P010 Cb plane is too small"))?;
                let v = copy_samples(
                    image.planes[2],
                    left as usize / 2,
                    top as usize / 2,
                    cw,
                    ch,
                    2,
                )
                .ok_or_else(|| invalid("AImage P010 Cr plane is too small"))?;
                let mut interleaved = Vec::with_capacity(cw * ch * 4);
                for (u, v) in u.chunks_exact(2).zip(v.chunks_exact(2)) {
                    interleaved.extend_from_slice(u);
                    interleaved.extend_from_slice(v);
                }
                interleaved
            };
            DecodedFrame::new(
                PixelFormat::P010,
                width,
                height,
                bit_depth,
                range,
                vec![
                    Plane {
                        data: y,
                        stride: w * 2,
                    },
                    Plane {
                        data: chroma,
                        stride: cw * 4,
                    },
                ],
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEVC_8: &[u8] =
        include_bytes!("../../../../android/mediacodec-harness/fixtures/data/hevc-8.rshw");
    const HEVC_10: &[u8] =
        include_bytes!("../../../../android/mediacodec-harness/fixtures/data/hevc-10.rshw");
    const AV1_8: &[u8] =
        include_bytes!("../../../../android/mediacodec-harness/fixtures/data/av1-8.rshw");
    const AV1_10: &[u8] =
        include_bytes!("../../../../android/mediacodec-harness/fixtures/data/av1-10.rshw");

    struct Fixture<'a> {
        codec: HwCodec,
        depth: u8,
        width: u32,
        height: u32,
        config: &'a [u8],
        payload: &'a [u8],
        expected: &'a [u8],
    }

    fn fixture(bytes: &[u8]) -> Fixture<'_> {
        assert_eq!(&bytes[..8], b"RSHW0001");
        let codec = match bytes[8] {
            1 => HwCodec::Hevc,
            2 => HwCodec::Av1,
            value => panic!("unknown fixture codec {value}"),
        };
        let depth = bytes[9];
        let width = u32::from(u16::from_le_bytes(bytes[10..12].try_into().unwrap()));
        let height = u32::from(u16::from_le_bytes(bytes[12..14].try_into().unwrap()));
        let config_len = u32::from_le_bytes(bytes[14..18].try_into().unwrap()) as usize;
        let payload_len = u32::from_le_bytes(bytes[18..22].try_into().unwrap()) as usize;
        let expected_len = u32::from_le_bytes(bytes[22..26].try_into().unwrap()) as usize;
        let config_end = 26 + config_len;
        let payload_end = config_end + payload_len;
        let expected_end = payload_end + expected_len;
        assert_eq!(expected_end, bytes.len());
        Fixture {
            codec,
            depth,
            width,
            height,
            config: &bytes[26..config_end],
            payload: &bytes[config_end..payload_end],
            expected: &bytes[payload_end..expected_end],
        }
    }

    fn request<'a>(fixture: &'a Fixture<'a>) -> StillDecodeRequest<'a> {
        StillDecodeRequest {
            config: match fixture.codec {
                HwCodec::Hevc => CodecConfig::Hvcc(fixture.config),
                HwCodec::Av1 => CodecConfig::Av1c(fixture.config),
            },
            payload: fixture.payload,
            width: fixture.width,
            height: fixture.height,
            bit_depth: fixture.depth,
            chroma: gamut_color::ChromaSubsampling::Cs420,
        }
    }

    #[test]
    fn checked_in_codec_fixtures_prepare_with_authoritative_geometry() {
        for bytes in [HEVC_8, HEVC_10, AV1_8, AV1_10] {
            let fixture = fixture(bytes);
            let prepared = prepare(&request(&fixture)).expect("fixture prepares");
            assert_eq!(prepared.codec, fixture.codec);
            assert_eq!(prepared.bit_depth, fixture.depth);
            assert_eq!((prepared.width, prepared.height), (64, 64));
            assert!(!prepared.csd0.is_empty());
            assert!(!prepared.access_unit.is_empty());
            match fixture.codec {
                HwCodec::Hevc => assert!(prepared.csd0.starts_with(&[0, 0, 0, 1])),
                HwCodec::Av1 => assert_eq!(prepared.csd0, fixture.config),
            }
        }
    }

    #[test]
    fn checked_in_expected_planes_round_trip_through_normalization() {
        for bytes in [HEVC_8, AV1_8] {
            let fixture = fixture(bytes);
            let pixels = (fixture.width * fixture.height) as usize;
            let chroma = pixels / 4;
            let image = ImageView {
                layout: ImageLayout::Yuv420,
                width: fixture.width,
                height: fixture.height,
                crop: (0, 0, fixture.width, fixture.height),
                planes: vec![
                    PlaneView {
                        data: &fixture.expected[..pixels],
                        row_stride: fixture.width as usize,
                        pixel_stride: 1,
                    },
                    PlaneView {
                        data: &fixture.expected[pixels..pixels + chroma],
                        row_stride: fixture.width as usize / 2,
                        pixel_stride: 1,
                    },
                    PlaneView {
                        data: &fixture.expected[pixels + chroma..],
                        row_stride: fixture.width as usize / 2,
                        pixel_stride: 1,
                    },
                ],
            };
            let frame = normalize_image(&image, (64, 64), 8, ColorRange::Limited).unwrap();
            let flattened: Vec<_> = frame
                .planes()
                .iter()
                .flat_map(|plane| plane.data.iter().copied())
                .collect();
            assert_eq!(flattened, fixture.expected);
        }

        for bytes in [HEVC_10, AV1_10] {
            let fixture = fixture(bytes);
            let luma_bytes = (fixture.width * fixture.height * 2) as usize;
            let image = ImageView {
                layout: ImageLayout::P010,
                width: fixture.width,
                height: fixture.height,
                crop: (0, 0, fixture.width, fixture.height),
                planes: vec![
                    PlaneView {
                        data: &fixture.expected[..luma_bytes],
                        row_stride: fixture.width as usize * 2,
                        pixel_stride: 2,
                    },
                    PlaneView {
                        data: &fixture.expected[luma_bytes..],
                        row_stride: fixture.width as usize * 2,
                        pixel_stride: 4,
                    },
                ],
            };
            let frame = normalize_image(&image, (64, 64), 10, ColorRange::Limited).unwrap();
            let flattened: Vec<_> = frame
                .planes()
                .iter()
                .flat_map(|plane| plane.data.iter().copied())
                .collect();
            assert_eq!(flattened, fixture.expected);
        }
    }

    #[test]
    fn normalizes_padded_semiplanar_yuv_to_i420() {
        let y = [1, 2, 3, 4, 0, 0, 5, 6, 7, 8, 0, 0];
        let uv = [10, 20, 11, 21, 0, 0];
        let image = ImageView {
            layout: ImageLayout::Yuv420,
            width: 4,
            height: 2,
            crop: (0, 0, 4, 2),
            planes: vec![
                PlaneView {
                    data: &y,
                    row_stride: 6,
                    pixel_stride: 1,
                },
                PlaneView {
                    data: &uv,
                    row_stride: 6,
                    pixel_stride: 2,
                },
                PlaneView {
                    data: &uv[1..],
                    row_stride: 6,
                    pixel_stride: 2,
                },
            ],
        };
        let frame = normalize_image(&image, (4, 2), 8, ColorRange::Limited).unwrap();
        assert_eq!(frame.format(), PixelFormat::I420);
        assert_eq!(frame.planes()[0].data, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(frame.planes()[1].data, [10, 11]);
        assert_eq!(frame.planes()[2].data, [20, 21]);
    }

    #[test]
    fn normalizes_three_plane_p010_to_interleaved_msb_words() {
        let y = [0x00, 0x04, 0x00, 0x08, 0, 0, 0x00, 0x0c, 0x00, 0x10];
        let u = [0x00, 0x14, 0, 0];
        let v = [0x00, 0x18, 0, 0];
        let image = ImageView {
            layout: ImageLayout::P010,
            width: 2,
            height: 2,
            crop: (0, 0, 2, 2),
            planes: vec![
                PlaneView {
                    data: &y,
                    row_stride: 6,
                    pixel_stride: 2,
                },
                PlaneView {
                    data: &u,
                    row_stride: 4,
                    pixel_stride: 4,
                },
                PlaneView {
                    data: &v,
                    row_stride: 4,
                    pixel_stride: 4,
                },
            ],
        };
        let frame = normalize_image(&image, (2, 2), 10, ColorRange::Full).unwrap();
        assert_eq!(frame.format(), PixelFormat::P010);
        assert_eq!(frame.planes()[1].data, [0x00, 0x14, 0x00, 0x18]);
    }

    #[test]
    fn rejects_mismatched_crop_and_odd_origin() {
        let plane = PlaneView {
            data: &[0; 64],
            row_stride: 8,
            pixel_stride: 1,
        };
        let image = ImageView {
            layout: ImageLayout::Yuv420,
            width: 8,
            height: 8,
            crop: (1, 0, 5, 4),
            planes: vec![plane; 3],
        };
        assert!(normalize_image(&image, (4, 4), 8, ColorRange::Limited).is_err());
    }
}
