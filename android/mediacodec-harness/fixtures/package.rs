//! Packages ffmpeg elementary streams and their source YUV into `.rshw`
//! fixtures consumed by the host tests and Android device harness.
//!
//! This deliberately uses only `std` so fixture regeneration does not alter
//! the workspace dependency graph. See `README.md` for exact commands.

use std::env;
use std::fs;
use std::path::Path;

const WIDTH: u16 = 64;
const HEIGHT: u16 = 64;

fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(
        args.len(),
        6,
        "package <hevc|av1> <8|10> <stream> <yuv> <out>"
    );
    let codec = &args[1];
    let depth: u8 = args[2].parse().expect("bit depth");
    let stream = fs::read(&args[3]).expect("stream");
    let source = fs::read(&args[4]).expect("source YUV");
    let (codec_id, config, payload) = match codec.as_str() {
        "hevc" => {
            let (config, payload) = package_hevc(&stream, depth);
            (1, config, payload)
        }
        "av1" => {
            let (config, payload) = package_av1(&stream, depth);
            (2, config, payload)
        }
        _ => panic!("unknown codec"),
    };
    let expected = expected_frame(depth, &source);
    let mut output = b"RSHW0001".to_vec();
    output.extend_from_slice(&[codec_id, depth]);
    output.extend_from_slice(&WIDTH.to_le_bytes());
    output.extend_from_slice(&HEIGHT.to_le_bytes());
    output.extend_from_slice(&(config.len() as u32).to_le_bytes());
    output.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    output.extend_from_slice(&(expected.len() as u32).to_le_bytes());
    output.extend_from_slice(&config);
    output.extend_from_slice(&payload);
    output.extend_from_slice(&expected);
    fs::write(Path::new(&args[5]), output).expect("fixture output");
}

fn package_hevc(stream: &[u8], depth: u8) -> (Vec<u8>, Vec<u8>) {
    let units = annex_b_units(stream);
    let mut parameter_sets = Vec::new();
    let mut payload = Vec::new();
    for unit in units {
        let kind = (unit[0] >> 1) & 0x3f;
        if matches!(kind, 32..=34) {
            parameter_sets.push((kind, unit));
        } else if kind < 32 {
            payload.extend_from_slice(&(unit.len() as u32).to_be_bytes());
            payload.extend_from_slice(unit);
        }
    }
    for required in 32..=34 {
        assert!(parameter_sets.iter().any(|(kind, _)| *kind == required));
    }
    assert!(!payload.is_empty(), "no coded HEVC slice");

    let mut hvcc = vec![0; 23];
    hvcc[0] = 1;
    hvcc[16] = 0xf1; // reserved bits + 4:2:0 chroma_format_idc
    hvcc[17] = 0xf8 | depth.saturating_sub(8);
    hvcc[18] = hvcc[17];
    hvcc[21] = 0xff; // four-byte NAL length fields
    hvcc[22] = 3;
    for kind in 32..=34 {
        let matching: Vec<_> = parameter_sets
            .iter()
            .filter(|(found, _)| *found == kind)
            .map(|(_, bytes)| *bytes)
            .collect();
        hvcc.push(0x80 | kind);
        hvcc.extend_from_slice(&(matching.len() as u16).to_be_bytes());
        for unit in matching {
            hvcc.extend_from_slice(&(unit.len() as u16).to_be_bytes());
            hvcc.extend_from_slice(unit);
        }
    }
    (hvcc, payload)
}

fn annex_b_units(stream: &[u8]) -> Vec<&[u8]> {
    fn start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
        (from..data.len().saturating_sub(2)).find_map(|index| {
            if data[index..].starts_with(&[0, 0, 0, 1]) {
                Some((index, 4))
            } else if data[index..].starts_with(&[0, 0, 1]) {
                Some((index, 3))
            } else {
                None
            }
        })
    }
    let mut units = Vec::new();
    let Some((first, first_prefix)) = start_code(stream, 0) else {
        return units;
    };
    let mut begin = first + first_prefix;
    loop {
        match start_code(stream, begin) {
            Some((next, next_prefix)) => {
                if next > begin {
                    units.push(&stream[begin..next]);
                }
                begin = next + next_prefix;
            }
            None => {
                if begin < stream.len() {
                    units.push(&stream[begin..]);
                }
                break;
            }
        }
    }
    units
}

fn package_av1(stream: &[u8], depth: u8) -> (Vec<u8>, Vec<u8>) {
    let mut config_obus = Vec::new();
    let mut payload = Vec::new();
    let mut offset = 0;
    while offset < stream.len() {
        let begin = offset;
        let header = stream[offset];
        offset += 1;
        assert_eq!(header & 0x80, 0, "AV1 forbidden bit");
        let kind = (header >> 3) & 0x0f;
        if header & 0x04 != 0 {
            offset += 1;
        }
        assert_ne!(header & 0x02, 0, "fixture OBU needs an explicit size");
        let (size, leb_bytes) = read_leb128(&stream[offset..]);
        offset += leb_bytes;
        offset += size;
        assert!(offset <= stream.len(), "truncated AV1 OBU");
        if kind == 1 {
            config_obus.extend_from_slice(&stream[begin..offset]);
        } else {
            payload.extend_from_slice(&stream[begin..offset]);
        }
    }
    assert!(!config_obus.is_empty(), "no AV1 sequence header");
    assert!(!payload.is_empty(), "no AV1 coded frame");
    let high_bitdepth = if depth == 10 { 0x40 } else { 0 };
    // Profile 0, level 0, 4:2:0, no initial presentation delay. The sequence
    // header remains authoritative, but these av1C summary bits must agree.
    let mut av1c = vec![0x81, 0, high_bitdepth | 0x0c, 0];
    av1c.extend_from_slice(&config_obus);
    (av1c, payload)
}

fn read_leb128(data: &[u8]) -> (usize, usize) {
    let mut value = 0usize;
    for (index, byte) in data.iter().copied().take(8).enumerate() {
        value |= usize::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return (value, index + 1);
        }
    }
    panic!("invalid leb128")
}

fn expected_frame(depth: u8, source: &[u8]) -> Vec<u8> {
    let pixels = usize::from(WIDTH) * usize::from(HEIGHT);
    match depth {
        8 => {
            assert_eq!(source.len(), pixels * 3 / 2);
            source.to_vec()
        }
        10 => {
            assert_eq!(source.len(), pixels * 3);
            let (y, chroma) = source.split_at(pixels * 2);
            let (u, v) = chroma.split_at(pixels / 2);
            let mut output = Vec::with_capacity(source.len());
            for word in y.chunks_exact(2) {
                output.extend_from_slice(
                    &(u16::from_le_bytes([word[0], word[1]]) << 6).to_le_bytes(),
                );
            }
            for (u, v) in u.chunks_exact(2).zip(v.chunks_exact(2)) {
                output.extend_from_slice(&(u16::from_le_bytes([u[0], u[1]]) << 6).to_le_bytes());
                output.extend_from_slice(&(u16::from_le_bytes([v[0], v[1]]) << 6).to_le_bytes());
            }
            output
        }
        _ => panic!("unsupported depth"),
    }
}
