#[cfg(feature = "arw-decode")]
pub(crate) mod arw;
#[cfg(feature = "avif-encode-libaom")]
pub(crate) mod avif_libaom;
#[cfg(any(feature = "arw-decode", feature = "cr2-decode", feature = "nef-decode"))]
pub(crate) mod bit_pump;
#[cfg(feature = "heic-decode")]
pub(crate) mod heic;
#[cfg(feature = "jpeg-encode-jpegli")]
pub(crate) mod jpegli;
#[cfg(feature = "jxl-encode-libjxl")]
pub(crate) mod jxl_libjxl;
// Lossless JPEG stays in-repo only for the CR2/NEF/ARW paths; the DNG path
// uses gamut-dng's internal (and public) lossless-JPEG implementation.
#[cfg(any(feature = "arw-decode", feature = "cr2-decode", feature = "nef-decode"))]
pub(crate) mod ljpeg;
#[cfg(any(feature = "webp-decode", feature = "webp-encode"))]
pub(crate) mod webp;
