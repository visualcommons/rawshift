#![cfg_attr(not(target_os = "android"), allow(dead_code))]

#[cfg(target_os = "android")]
mod android {
    use std::time::Instant;

    use jni::errors::ThrowRuntimeExAndDefault;
    use jni::objects::{JClass, JString};
    use jni::{Env, EnvUnowned};
    use rawshift_hwdec::{
        ChromaSubsampling, CodecConfig, HwCodec, HwDecodeError, PixelFormat, StillDecodeRequest,
        available_codecs, backend, decoder, initialize_android_hw_decode,
    };

    const HEVC_8: &[u8] = include_bytes!("../../fixtures/data/hevc-8.rshw");
    const HEVC_10: &[u8] = include_bytes!("../../fixtures/data/hevc-10.rshw");
    const AV1_8: &[u8] = include_bytes!("../../fixtures/data/av1-8.rshw");
    const AV1_10: &[u8] = include_bytes!("../../fixtures/data/av1-10.rshw");

    struct Fixture<'a> {
        codec: HwCodec,
        depth: u8,
        width: u32,
        height: u32,
        config: &'a [u8],
        payload: &'a [u8],
        expected: &'a [u8],
    }

    enum Outcome {
        Pass(String),
        Unavailable(String),
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_org_visualcommons_rawshift_harness_NativeHarness_runSuite<
        'local,
    >(
        mut unowned_env: EnvUnowned<'local>,
        _class: JClass<'local>,
        sdk: i32,
    ) -> JString<'local> {
        unowned_env
            .with_env(|env| -> jni::errors::Result<_> {
                let report = run_suite(env, sdk).unwrap_or_else(|error| format!("FAIL\n{error}"));
                JString::from_str(env, report)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    }

    fn run_suite(env: &Env<'_>, sdk: i32) -> Result<String, String> {
        initialize_android_hw_decode(env.get_java_vm().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        // Prove same-VM initialization is idempotent, not merely documented.
        initialize_android_hw_decode(env.get_java_vm().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let found = available_codecs();
        for codec in [HwCodec::Hevc, HwCodec::Av1] {
            if !found.contains(&codec) {
                return Err(format!("no hardware {codec} decoder was discovered"));
            }
        }

        let mut lines = vec![format!(
            "PASS sdk={sdk} backend={} codecs={found:?}",
            backend().ok_or_else(|| "backend() returned None after discovery".to_string())?
        )];
        for bytes in [HEVC_8, AV1_8] {
            match run_fixture(bytes)? {
                Outcome::Pass(line) => lines.push(line),
                Outcome::Unavailable(reason) => {
                    return Err(format!("required 8-bit fixture unavailable: {reason}"));
                }
            }
        }

        if sdk >= 33 {
            let mut p010_passes = 0;
            for bytes in [HEVC_10, AV1_10] {
                match run_fixture(bytes)? {
                    Outcome::Pass(line) => {
                        p010_passes += 1;
                        lines.push(line);
                    }
                    Outcome::Unavailable(reason) => lines.push(format!("SKIP p010 {reason}")),
                }
            }
            if p010_passes == 0 {
                return Err("API 33+ device exposed no working hardware P010 path".to_string());
            }
        } else {
            lines.push("SKIP p010 requires API 33+".to_string());
        }
        Ok(lines.join("\n"))
    }

    fn run_fixture(bytes: &[u8]) -> Result<Outcome, String> {
        let fixture = fixture(bytes)?;
        let request = StillDecodeRequest {
            config: match fixture.codec {
                HwCodec::Hevc => CodecConfig::Hvcc(fixture.config),
                HwCodec::Av1 => CodecConfig::Av1c(fixture.config),
            },
            payload: fixture.payload,
            width: fixture.width,
            height: fixture.height,
            bit_depth: fixture.depth,
            chroma: ChromaSubsampling::Cs420,
        };
        let Some(mut decoder) = decoder(fixture.codec) else {
            return Ok(Outcome::Unavailable(format!(
                "{} {}-bit decoder() returned None",
                fixture.codec, fixture.depth
            )));
        };

        let cold_start = Instant::now();
        let cold = match decoder.decode_still(&request) {
            Ok(frame) => frame,
            Err(HwDecodeError::Unavailable { reason, .. }) => {
                return Ok(Outcome::Unavailable(format!(
                    "{} {}-bit: {reason}",
                    fixture.codec, fixture.depth
                )));
            }
            Err(error) => return Err(error.to_string()),
        };
        let cold_us = cold_start.elapsed().as_micros();
        verify_frame(&fixture, &cold)?;

        // The second request is byte-for-byte identical and therefore must
        // exercise the exact-format session reuse path.
        let warm_start = Instant::now();
        let warm = decoder.decode_still(&request).map_err(|e| e.to_string())?;
        let warm_us = warm_start.elapsed().as_micros();
        verify_frame(&fixture, &warm)?;
        Ok(Outcome::Pass(format!(
            "{} {}-bit cold_us={cold_us} warm_us={warm_us}",
            fixture.codec, fixture.depth
        )))
    }

    fn verify_frame(
        fixture: &Fixture<'_>,
        frame: &rawshift_hwdec::DecodedFrame,
    ) -> Result<(), String> {
        if (frame.width(), frame.height(), frame.bit_depth())
            != (fixture.width, fixture.height, fixture.depth)
        {
            return Err(format!(
                "{} output geometry/depth mismatch: {}x{} {}-bit",
                fixture.codec,
                frame.width(),
                frame.height(),
                frame.bit_depth()
            ));
        }
        let expected_format = if fixture.depth == 8 {
            PixelFormat::I420
        } else {
            PixelFormat::P010
        };
        if frame.format() != expected_format {
            return Err(format!(
                "{} expected {expected_format:?}, got {:?}",
                fixture.codec,
                frame.format()
            ));
        }
        let actual: Vec<_> = frame
            .planes()
            .iter()
            .flat_map(|plane| plane.data.iter().copied())
            .collect();
        if actual != fixture.expected {
            return Err(format!(
                "{} {}-bit decoded pixels differ (expected fnv={:016x}, actual fnv={:016x})",
                fixture.codec,
                fixture.depth,
                fnv1a(fixture.expected),
                fnv1a(&actual)
            ));
        }
        Ok(())
    }

    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    fn fixture(bytes: &[u8]) -> Result<Fixture<'_>, String> {
        if bytes.get(..8) != Some(b"RSHW0001") {
            return Err("bad fixture magic".to_string());
        }
        let codec = match bytes[8] {
            1 => HwCodec::Hevc,
            2 => HwCodec::Av1,
            value => return Err(format!("bad fixture codec {value}")),
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
        if expected_end != bytes.len() {
            return Err("bad fixture lengths".to_string());
        }
        Ok(Fixture {
            codec,
            depth,
            width,
            height,
            config: &bytes[26..config_end],
            payload: &bytes[config_end..payload_end],
            expected: &bytes[payload_end..expected_end],
        })
    }
}
