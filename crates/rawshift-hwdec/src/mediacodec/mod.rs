//! Android MediaCodec still-frame decoder.

mod core;

#[cfg(hwdec_backend = "mediacodec")]
mod platform;

#[cfg(hwdec_backend = "mediacodec")]
pub(crate) use platform::{available_codecs, backend, decoder, initialize};
