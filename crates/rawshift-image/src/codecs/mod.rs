#[cfg(feature = "arw-decode")]
pub(crate) mod arw;
#[cfg(feature = "tiff-parser")]
pub(crate) mod bit_pump;
#[cfg(feature = "heic-decode")]
pub(crate) mod heic;
#[cfg(feature = "jpeg-encode-jpegli")]
pub(crate) mod jpegli;
#[cfg(feature = "dng-decode")]
pub(crate) mod jxl;
#[cfg(feature = "jxl-encode-libjxl")]
pub(crate) mod jxl_libjxl;
#[cfg(feature = "tiff-parser")]
pub(crate) mod ljpeg;
#[cfg(any(feature = "webp-decode", feature = "webp-encode"))]
pub(crate) mod webp;
