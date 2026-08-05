//! Android MediaCodec backend.
//!
//! Java is used only for the definitive `MediaCodecList` capability query.
//! Decode and output acquisition use the NDK `AMediaCodec`/`AImageReader`
//! APIs. All NDK wrappers stay on one worker because their raw handles are
//! intentionally not `Send`.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use jni::objects::{JIntArray, JObject, JObjectArray, JString};
use jni::{Env, JValue, JavaVM, jni_sig, jni_str};
use ndk::hardware_buffer::HardwareBufferUsage;
use ndk::media::image_reader::{AcquireResult, Image, ImageFormat, ImageReader};
use ndk::media::media_codec::{
    DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection,
};
use ndk::media::media_format::MediaFormat;

use super::core::{
    ImageLayout, ImageView, MIME_AV1, MIME_HEVC, PlaneView, PreparedRequest, SessionKey,
    normalize_image, prepare,
};
use crate::{
    AndroidHwDecodeInitError, DecodedFrame, HwBackend, HwCodec, HwDecodeError, HwStillDecoder,
    StillDecodeRequest,
};

const API_P010: i32 = 33;
const COLOR_FORMAT_P010: i32 = 54;
const COLOR_FORMAT_YUV420_FLEXIBLE: i32 = 0x7f42_0888;
const BUFFER_FLAG_CODEC_CONFIG: u32 = 2;
const BUFFER_FLAG_END_OF_STREAM: u32 = 4;
const DECODE_DEADLINE: Duration = Duration::from_secs(5);
const POLL_SLICE: Duration = Duration::from_millis(25);

const HEVC_ONLY: &[HwCodec] = &[HwCodec::Hevc];
const AV1_ONLY: &[HwCodec] = &[HwCodec::Av1];
const BOTH_CODECS: &[HwCodec] = &[HwCodec::Hevc, HwCodec::Av1];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    name: String,
    p010: bool,
}

#[derive(Debug)]
struct Runtime {
    vm: JavaVM,
    sdk: i32,
    hevc: Vec<Candidate>,
    av1: Vec<Candidate>,
}

#[derive(Debug, Clone)]
struct RuntimeSnapshot {
    sdk: i32,
    candidates: Vec<Candidate>,
}

static RUNTIME: OnceLock<Mutex<Option<Runtime>>> = OnceLock::new();

fn runtime() -> &'static Mutex<Option<Runtime>> {
    RUNTIME.get_or_init(|| Mutex::new(None))
}

fn lock_runtime() -> std::sync::MutexGuard<'static, Option<Runtime>> {
    runtime()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn initialize(vm: JavaVM) -> Result<(), AndroidHwDecodeInitError> {
    let mut state = lock_runtime();
    if let Some(current) = state.as_ref() {
        return if current.vm.get_raw() == vm.get_raw() {
            Ok(())
        } else {
            Err(AndroidHwDecodeInitError::DifferentJavaVm)
        };
    }

    // Do not publish partial state. JNI/Java failures leave the slot empty so
    // applications can retry after their runtime is fully initialized.
    let discovery = vm.attach_current_thread(discover_codecs).map_err(|error| {
        AndroidHwDecodeInitError::Jni {
            operation: "MediaCodecList enumeration",
            message: error.to_string(),
        }
    })?;
    *state = Some(Runtime {
        vm,
        sdk: discovery.sdk,
        hevc: discovery.hevc,
        av1: discovery.av1,
    });
    Ok(())
}

#[derive(Debug)]
struct Discovery {
    sdk: i32,
    hevc: Vec<Candidate>,
    av1: Vec<Candidate>,
}

fn discover_codecs(env: &mut Env<'_>) -> jni::errors::Result<Discovery> {
    let sdk = env
        .get_static_field(
            jni_str!("android/os/Build$VERSION"),
            jni_str!("SDK_INT"),
            jni_sig!(int),
        )?
        .i()?;
    let list = env.new_object(
        jni_str!("android/media/MediaCodecList"),
        jni_sig!((int) -> void),
        &[JValue::Int(1)], // MediaCodecList.ALL_CODECS
    )?;
    let infos = env
        .call_method(
            &list,
            jni_str!("getCodecInfos"),
            jni_sig!(() -> [android.media.MediaCodecInfo]),
            &[],
        )?
        .l()?;
    let infos: JObjectArray<'_, JObject<'_>> = JObjectArray::<JObject>::cast_local(env, infos)?;

    let mut hevc = Vec::new();
    let mut av1 = Vec::new();
    for index in 0..infos.len(env)? {
        match env.with_local_frame(32, |env| {
            let info = infos.get_element(env, index)?;
            inspect_codec(env, &info)
        }) {
            Ok(found) => {
                for (mime, candidate) in found {
                    match mime {
                        MIME_HEVC => hevc.push(candidate),
                        MIME_AV1 => av1.push(candidate),
                        _ => unreachable!("only requested MIME types are returned"),
                    }
                }
            }
            Err(error) => tracing::debug!(
                target: "rawshift_hwdec::mediacodec",
                codec_index = index,
                %error,
                "skipping MediaCodecInfo after capability-query failure"
            ),
        }
    }
    Ok(Discovery {
        sdk,
        hevc: deduplicate(hevc),
        av1: deduplicate(av1),
    })
}

fn inspect_codec<'local>(
    env: &mut Env<'local>,
    info: &JObject<'_>,
) -> jni::errors::Result<Vec<(&'static str, Candidate)>> {
    if env
        .call_method(info, jni_str!("isEncoder"), jni_sig!(() -> boolean), &[])?
        .z()?
        || env
            .call_method(info, jni_str!("isAlias"), jni_sig!(() -> boolean), &[])?
            .z()?
        || !env
            .call_method(
                info,
                jni_str!("isHardwareAccelerated"),
                jni_sig!(() -> boolean),
                &[],
            )?
            .z()?
    {
        return Ok(Vec::new());
    }

    let name = env
        .call_method(info, jni_str!("getName"), jni_sig!(() -> JString), &[])?
        .l()?;
    let name = JString::cast_local(env, name)?.try_to_string(env)?;
    let types = env
        .call_method(
            info,
            jni_str!("getSupportedTypes"),
            jni_sig!(() -> [JString]),
            &[],
        )?
        .l()?;
    let types: JObjectArray<'_, JString<'_>> = JObjectArray::<JString>::cast_local(env, types)?;
    let mut found = Vec::new();
    for index in 0..types.len(env)? {
        let java_mime = types.get_element(env, index)?;
        let mime = java_mime.try_to_string(env)?;
        let mime = match mime.as_str() {
            MIME_HEVC => MIME_HEVC,
            MIME_AV1 => MIME_AV1,
            _ => continue,
        };
        let capabilities = env
            .call_method(
                info,
                jni_str!("getCapabilitiesForType"),
                jni_sig!("(Ljava/lang/String;)Landroid/media/MediaCodecInfo$CodecCapabilities;"),
                &[JValue::Object(java_mime.as_ref())],
            )?
            .l()?;
        if feature_required(env, &capabilities, "secure-playback")?
            || feature_required(env, &capabilities, "tunneled-playback")?
        {
            continue;
        }
        let formats = env
            .get_field(&capabilities, jni_str!("colorFormats"), jni_sig!([int]))?
            .l()?;
        let formats = JIntArray::cast_local(env, formats)?;
        let mut values = vec![0; formats.len(env)?];
        formats.get_region(env, 0, &mut values)?;
        found.push((
            mime,
            Candidate {
                name: name.clone(),
                p010: values.contains(&COLOR_FORMAT_P010),
            },
        ));
    }
    Ok(found)
}

fn feature_required(
    env: &mut Env<'_>,
    capabilities: &JObject<'_>,
    feature: &str,
) -> jni::errors::Result<bool> {
    let feature = env.new_string(feature)?;
    env.call_method(
        capabilities,
        jni_str!("isFeatureRequired"),
        jni_sig!((JString) -> boolean),
        &[JValue::Object(feature.as_ref())],
    )?
    .z()
}

fn deduplicate(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut names = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| names.insert(candidate.name.clone()))
        .collect()
}

fn snapshot(codec: HwCodec) -> Option<RuntimeSnapshot> {
    let state = lock_runtime();
    let runtime = state.as_ref()?;
    Some(RuntimeSnapshot {
        sdk: runtime.sdk,
        candidates: match codec {
            HwCodec::Hevc => runtime.hevc.clone(),
            HwCodec::Av1 => runtime.av1.clone(),
        },
    })
}

pub(crate) fn available_codecs() -> &'static [HwCodec] {
    let state = lock_runtime();
    let Some(runtime) = state.as_ref() else {
        return &[];
    };
    match (runtime.hevc.is_empty(), runtime.av1.is_empty()) {
        (false, false) => BOTH_CODECS,
        (false, true) => HEVC_ONLY,
        (true, false) => AV1_ONLY,
        (true, true) => &[],
    }
}

pub(crate) fn backend() -> Option<HwBackend> {
    (!available_codecs().is_empty()).then_some(HwBackend::MediaCodec)
}

pub(crate) fn decoder(codec: HwCodec) -> Option<Box<dyn HwStillDecoder>> {
    let snapshot = snapshot(codec)?;
    if snapshot.candidates.is_empty() {
        None
    } else {
        MediaCodecStillDecoder::new(codec, snapshot)
            .map(|decoder| Box::new(decoder) as Box<dyn HwStillDecoder>)
    }
}

enum WorkerMessage {
    Decode {
        request: PreparedRequest,
        reply: SyncSender<Result<DecodedFrame, HwDecodeError>>,
    },
    Shutdown,
}

struct MediaCodecStillDecoder {
    codec: HwCodec,
    runtime: RuntimeSnapshot,
    sender: SyncSender<WorkerMessage>,
    worker: Option<JoinHandle<()>>,
}

impl MediaCodecStillDecoder {
    fn new(codec: HwCodec, runtime: RuntimeSnapshot) -> Option<Self> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_runtime = runtime.clone();
        let worker = match thread::Builder::new()
            .name(format!("rawshift-{codec}-mediacodec"))
            .spawn(move || worker_main(receiver, worker_runtime))
        {
            Ok(worker) => worker,
            Err(error) => {
                tracing::debug!(
                    target: "rawshift_hwdec::mediacodec",
                    %error,
                    "failed to create MediaCodec worker"
                );
                return None;
            }
        };
        Some(Self {
            codec,
            runtime,
            sender,
            worker: Some(worker),
        })
    }

    fn restart_worker(&mut self) -> Result<(), String> {
        // A timed-out NDK call may still own thread-affine handles. Detach the
        // old thread and create a completely fresh worker/session.
        self.worker.take();
        let (sender, receiver) = mpsc::sync_channel(1);
        let runtime = self.runtime.clone();
        self.sender = sender;
        self.worker = Some(
            thread::Builder::new()
                .name(format!("rawshift-{}-mediacodec", self.codec))
                .spawn(move || worker_main(receiver, runtime))
                .map_err(|error| error.to_string())?,
        );
        Ok(())
    }

    fn restart_suffix(&mut self) -> String {
        match self.restart_worker() {
            Ok(()) => "; a fresh worker is ready".to_string(),
            Err(error) => format!("; creating a fresh worker also failed: {error}"),
        }
    }
}

impl HwStillDecoder for MediaCodecStillDecoder {
    fn decode_still(
        &mut self,
        request: &StillDecodeRequest<'_>,
    ) -> Result<DecodedFrame, HwDecodeError> {
        let prepared = prepare(request)?;
        debug_assert_eq!(prepared.codec, self.codec);
        let (reply, response) = mpsc::sync_channel(1);
        if self
            .sender
            .send(WorkerMessage::Decode {
                request: prepared,
                reply,
            })
            .is_err()
        {
            let suffix = self.restart_suffix();
            return Err(HwDecodeError::Decode {
                codec: self.codec,
                message: format!("MediaCodec worker exited unexpectedly{suffix}"),
            });
        }
        match response.recv_timeout(DECODE_DEADLINE) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let suffix = self.restart_suffix();
                Err(HwDecodeError::Decode {
                    codec: self.codec,
                    message: format!(
                        "MediaCodec decode exceeded the fixed 5 second deadline{suffix}"
                    ),
                })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let suffix = self.restart_suffix();
                Err(HwDecodeError::Decode {
                    codec: self.codec,
                    message: format!("MediaCodec worker exited without a result{suffix}"),
                })
            }
        }
    }
}

impl Drop for MediaCodecStillDecoder {
    fn drop(&mut self) {
        let _ = self.sender.send(WorkerMessage::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_main(receiver: Receiver<WorkerMessage>, runtime: RuntimeSnapshot) {
    let mut state = WorkerState {
        runtime,
        session: None,
    };
    while let Ok(message) = receiver.recv() {
        match message {
            WorkerMessage::Decode { request, reply } => {
                let _ = reply.send(state.decode(request));
            }
            WorkerMessage::Shutdown => break,
        }
    }
}

struct WorkerState {
    runtime: RuntimeSnapshot,
    session: Option<Session>,
}

impl WorkerState {
    fn decode(&mut self, request: PreparedRequest) -> Result<DecodedFrame, HwDecodeError> {
        if request.bit_depth > 8 && self.runtime.sdk < API_P010 {
            return Err(HwDecodeError::Unavailable {
                codec: request.codec,
                reason: format!(
                    "10-bit output requires Android API {API_P010}+ (device is API {})",
                    self.runtime.sdk
                ),
            });
        }
        let mut candidates: Vec<Candidate> = self
            .runtime
            .candidates
            .iter()
            .filter(|candidate| request.bit_depth == 8 || candidate.p010)
            .cloned()
            .collect();
        if candidates.is_empty() {
            return Err(HwDecodeError::Unavailable {
                codec: request.codec,
                reason: if request.bit_depth > 8 {
                    "no hardware decoder advertises COLOR_FormatYUVP010".to_string()
                } else {
                    "no hardware decoder candidate remains".to_string()
                },
            });
        }

        if let Some(session) = self.session.as_ref()
            && let Some(index) = candidates
                .iter()
                .position(|candidate| candidate.name == session.candidate.name)
        {
            candidates.swap(0, index);
        }

        let total_deadline = Instant::now() + DECODE_DEADLINE;
        let mut failures = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            let now = Instant::now();
            if now >= total_deadline {
                failures.push(format!(
                    "{}@deadline: total deadline exhausted",
                    candidate.name
                ));
                continue;
            }
            // Reserve a fair portion of the remaining fixed budget for every
            // candidate, so a broken first codec cannot prevent fallback.
            let remaining_candidates = (candidates.len() - index) as u32;
            let candidate_deadline = now + (total_deadline - now) / remaining_candidates;
            let result = catch_unwind(AssertUnwindSafe(|| {
                self.decode_candidate(candidate, &request, candidate_deadline)
            }))
            .unwrap_or_else(|panic| {
                Err(PlatformFailure::message(
                    "panic",
                    panic_message(panic.as_ref()),
                ))
            });
            match result {
                Ok(frame) => return Ok(frame),
                Err(failure) => {
                    tracing::debug!(
                        target: "rawshift_hwdec::mediacodec",
                        codec = %request.codec,
                        component = %candidate.name,
                        stage = failure.stage,
                        error = %failure.message,
                        "MediaCodec hardware candidate failed"
                    );
                    failures.push(format!(
                        "{}@{}: {}",
                        candidate.name, failure.stage, failure.message
                    ));
                    self.discard_session();
                }
            }
        }
        tracing::warn!(
            target: "rawshift_hwdec::mediacodec",
            codec = %request.codec,
            failures = %failures.join("; "),
            "all MediaCodec hardware candidates failed"
        );
        Err(HwDecodeError::Decode {
            codec: request.codec,
            message: format!(
                "all MediaCodec hardware candidates failed: {}",
                failures.join("; ")
            ),
        })
    }

    fn decode_candidate(
        &mut self,
        candidate: &Candidate,
        request: &PreparedRequest,
        deadline: Instant,
    ) -> Result<DecodedFrame, PlatformFailure> {
        let reusable = self.session.as_ref().is_some_and(|session| {
            session.candidate.name == candidate.name && session.key == request.key
        });
        if reusable {
            let session = self.session.as_mut().expect("checked above");
            session
                .codec
                .flush()
                .map_err(|e| PlatformFailure::new("flush", e))?;
            session.drain_images()?;
        } else {
            self.discard_session();
            self.session = Some(Session::create(candidate.clone(), request)?);
        }
        self.session
            .as_mut()
            .expect("session was created")
            .decode(request, deadline)
    }

    fn discard_session(&mut self) {
        if let Some(session) = self.session.take()
            && let Err(panic) = catch_unwind(AssertUnwindSafe(|| drop(session)))
        {
            tracing::debug!(
                target: "rawshift_hwdec::mediacodec",
                error = %panic_message(panic.as_ref()),
                "MediaCodec session teardown panicked"
            );
        }
    }
}

#[derive(Debug)]
struct PlatformFailure {
    stage: &'static str,
    message: String,
}

impl PlatformFailure {
    fn new(stage: &'static str, error: impl std::fmt::Display) -> Self {
        Self {
            stage,
            message: error.to_string(),
        }
    }

    fn message(stage: &'static str, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
        }
    }
}

struct Session {
    candidate: Candidate,
    key: SessionKey,
    codec: MediaCodec,
    reader: ImageReader,
    image_signal: Arc<(Mutex<u64>, Condvar)>,
}

impl Session {
    fn create(candidate: Candidate, request: &PreparedRequest) -> Result<Self, PlatformFailure> {
        let image_format = if request.bit_depth == 8 {
            ImageFormat::YUV_420_888
        } else {
            ImageFormat::__Unknown(COLOR_FORMAT_P010)
        };
        let mut reader = ImageReader::new_with_usage(
            request
                .width
                .try_into()
                .map_err(|_| PlatformFailure::message("reader", "width exceeds i32"))?,
            request
                .height
                .try_into()
                .map_err(|_| PlatformFailure::message("reader", "height exceeds i32"))?,
            image_format,
            HardwareBufferUsage::CPU_READ_OFTEN,
            2,
        )
        .map_err(|e| PlatformFailure::new("reader", e))?;
        let image_signal = Arc::new((Mutex::new(0u64), Condvar::new()));
        let callback_signal = Arc::clone(&image_signal);
        reader
            .set_image_listener(Box::new(move |_| {
                let (lock, ready) = &*callback_signal;
                let mut generation = lock
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *generation = (*generation).wrapping_add(1);
                ready.notify_all();
            }))
            .map_err(|e| PlatformFailure::new("reader-listener", e))?;
        let window = reader
            .window()
            .map_err(|e| PlatformFailure::new("reader-window", e))?;
        let codec = MediaCodec::from_codec_name(&candidate.name).ok_or_else(|| {
            PlatformFailure::message("create", "AMediaCodec_createCodecByName returned null")
        })?;
        let mut format = MediaFormat::new();
        format.set_str("mime", request.mime);
        format.set_i32("width", request.width as i32);
        format.set_i32("height", request.height as i32);
        format.set_i32("max-input-size", request.access_unit.len() as i32);
        format.set_i32(
            "color-format",
            if request.bit_depth == 8 {
                COLOR_FORMAT_YUV420_FLEXIBLE
            } else {
                COLOR_FORMAT_P010
            },
        );
        format.set_buffer("csd-0", &request.csd0);
        codec
            .configure(&format, Some(&window), MediaCodecDirection::Decoder)
            .map_err(|e| PlatformFailure::new("configure", e))?;
        codec
            .start()
            .map_err(|e| PlatformFailure::new("start", e))?;
        Ok(Self {
            candidate,
            key: request.key.clone(),
            codec,
            reader,
            image_signal,
        })
    }

    fn drain_images(&self) -> Result<(), PlatformFailure> {
        loop {
            match self
                .reader
                .acquire_next_image()
                .map_err(|e| PlatformFailure::new("drain-image", e))?
            {
                AcquireResult::Image(image) => drop(image),
                AcquireResult::NoBufferAvailable => return Ok(()),
                AcquireResult::MaxImagesAcquired => {
                    return Err(PlatformFailure::message(
                        "drain-image",
                        "AImageReader reports max images acquired",
                    ));
                }
            }
        }
    }

    fn decode(
        &mut self,
        request: &PreparedRequest,
        deadline: Instant,
    ) -> Result<DecodedFrame, PlatformFailure> {
        self.queue_access_unit(request, deadline)?;
        let mut frame = None;
        let mut eos = false;
        while Instant::now() < deadline && (!eos || frame.is_none()) {
            let timeout = poll_timeout(deadline);
            match self
                .codec
                .dequeue_output_buffer(timeout)
                .map_err(|e| PlatformFailure::new("dequeue-output", e))?
            {
                DequeuedOutputBufferInfoResult::Buffer(output) => {
                    let flags = output.info().flags();
                    let render = flags & BUFFER_FLAG_CODEC_CONFIG == 0 && output.info().size() > 0;
                    self.codec
                        .release_output_buffer(output, render)
                        .map_err(|e| PlatformFailure::new("release-output", e))?;
                    if render && frame.is_none() {
                        frame = Some(self.wait_for_image(request, deadline)?);
                    }
                    eos |= flags & BUFFER_FLAG_END_OF_STREAM != 0;
                }
                DequeuedOutputBufferInfoResult::TryAgainLater
                | DequeuedOutputBufferInfoResult::OutputFormatChanged
                | DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
            }
        }
        frame.ok_or_else(|| PlatformFailure::message("output", "deadline elapsed before a frame"))
    }

    fn queue_access_unit(
        &self,
        request: &PreparedRequest,
        deadline: Instant,
    ) -> Result<(), PlatformFailure> {
        while Instant::now() < deadline {
            match self
                .codec
                .dequeue_input_buffer(poll_timeout(deadline))
                .map_err(|e| PlatformFailure::new("dequeue-input", e))?
            {
                DequeuedInputBufferResult::TryAgainLater => {}
                DequeuedInputBufferResult::Buffer(mut input) => {
                    let buffer = input.buffer_mut();
                    if buffer.len() < request.access_unit.len() {
                        return Err(PlatformFailure::message(
                            "input-buffer",
                            format!(
                                "codec supplied {} bytes for a {} byte access unit",
                                buffer.len(),
                                request.access_unit.len()
                            ),
                        ));
                    }
                    for (slot, byte) in buffer.iter_mut().zip(&request.access_unit) {
                        slot.write(*byte);
                    }
                    self.codec
                        .queue_input_buffer(
                            input,
                            0,
                            request.access_unit.len(),
                            0,
                            BUFFER_FLAG_END_OF_STREAM,
                        )
                        .map_err(|e| PlatformFailure::new("queue-input", e))?;
                    return Ok(());
                }
            }
        }
        Err(PlatformFailure::message(
            "input-buffer",
            "deadline elapsed before an input buffer",
        ))
    }

    fn wait_for_image(
        &self,
        request: &PreparedRequest,
        deadline: Instant,
    ) -> Result<DecodedFrame, PlatformFailure> {
        loop {
            match self
                .reader
                .acquire_next_image()
                .map_err(|e| PlatformFailure::new("acquire-image", e))?
            {
                AcquireResult::Image(image) => return image_to_frame(&image, request),
                AcquireResult::MaxImagesAcquired => {
                    return Err(PlatformFailure::message(
                        "acquire-image",
                        "AImageReader reports max images acquired",
                    ));
                }
                AcquireResult::NoBufferAvailable => {}
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(PlatformFailure::message(
                    "acquire-image",
                    "deadline elapsed before AImageReader delivered a frame",
                ));
            }
            let (lock, ready) = &*self.image_signal;
            let generation = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = ready
                .wait_timeout(generation, (deadline - now).min(POLL_SLICE))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.codec.stop();
    }
}

fn poll_timeout(deadline: Instant) -> Duration {
    deadline
        .saturating_duration_since(Instant::now())
        .min(POLL_SLICE)
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

fn image_to_frame(
    image: &Image,
    request: &PreparedRequest,
) -> Result<DecodedFrame, PlatformFailure> {
    let format = image
        .format()
        .map_err(|e| PlatformFailure::new("image-format", e))?;
    let layout = match format {
        ImageFormat::YUV_420_888 if request.bit_depth == 8 => ImageLayout::Yuv420,
        ImageFormat::__Unknown(COLOR_FORMAT_P010) if request.bit_depth > 8 => ImageLayout::P010,
        other => {
            return Err(PlatformFailure::message(
                "image-format",
                format!("unexpected AImage format {other:?}"),
            ));
        }
    };
    let width = positive_u32("image-width", image.width())?;
    let height = positive_u32("image-height", image.height())?;
    let crop = image
        .crop_rect()
        .map_err(|e| PlatformFailure::new("image-crop", e))?;
    let plane_count = image
        .number_of_planes()
        .map_err(|e| PlatformFailure::new("image-planes", e))?;
    if plane_count < 0 {
        return Err(PlatformFailure::message(
            "image-planes",
            "AImage returned a negative plane count",
        ));
    }
    let mut planes = Vec::with_capacity(plane_count as usize);
    for index in 0..plane_count {
        let data = image
            .plane_data(index)
            .map_err(|e| PlatformFailure::new("image-plane-data", e))?;
        let row_stride = positive_usize("image-row-stride", image.plane_row_stride(index))?;
        let pixel_stride = positive_usize("image-pixel-stride", image.plane_pixel_stride(index))?;
        planes.push(PlaneView {
            data,
            row_stride,
            pixel_stride,
        });
    }
    let view = ImageView {
        layout,
        width,
        height,
        crop: (
            nonnegative_u32("crop-left", crop.left)?,
            nonnegative_u32("crop-top", crop.top)?,
            nonnegative_u32("crop-right", crop.right)?,
            nonnegative_u32("crop-bottom", crop.bottom)?,
        ),
        planes,
    };
    normalize_image(
        &view,
        (request.width, request.height),
        request.bit_depth,
        request.range,
    )
    .map_err(|e| PlatformFailure::new("normalize-image", e))
}

fn positive_u32(
    stage: &'static str,
    value: Result<i32, ndk::media_error::MediaError>,
) -> Result<u32, PlatformFailure> {
    let value = value.map_err(|e| PlatformFailure::new(stage, e))?;
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| PlatformFailure::message(stage, format!("invalid value {value}")))
}

fn nonnegative_u32(stage: &'static str, value: i32) -> Result<u32, PlatformFailure> {
    u32::try_from(value)
        .map_err(|_| PlatformFailure::message(stage, format!("invalid value {value}")))
}

fn positive_usize(
    stage: &'static str,
    value: Result<i32, ndk::media_error::MediaError>,
) -> Result<usize, PlatformFailure> {
    let value = value.map_err(|e| PlatformFailure::new(stage, e))?;
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| PlatformFailure::message(stage, format!("invalid value {value}")))
}

// Compile-time assertion: prepared jobs and replies are the only values that
// cross threads; no NDK wrapper is made Send with unsafe code.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<PreparedRequest>();
    assert_send::<Result<DecodedFrame, HwDecodeError>>();
};
