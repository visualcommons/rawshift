// C ABI shim over jpegli's libjpeg-style encode API. See jpegli_shim.h.
//
// All FFI/`unsafe` interaction with jpegli is confined to this file and
// `jpegli.rs` (the `src/codecs` safety boundary — see PRINCIPLES.md). jpegli's
// public headers are C++ (they `#include <cstddef>`), so this shim is compiled
// as C++; its exported functions use C linkage for a stable ABI. setjmp/longjmp
// is safe here because the guarded scope holds only POD locals.

#include "jpegli_shim.h"

#if defined(RAWSHIFT_JPEGLI_VENDORED)
// Vendored build: include from the jpegli source tree (headers use `lib/`-prefixed
// includes internally, so the source root is on the include path).
#include "lib/jpegli/encode.h"
#else
// System build (pkg-config libjpegli): the installed public header.
#include <jpegli/encode.h>
#endif

#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>

namespace {

// jpegli/libjpeg error manager extended with a setjmp target + message buffer.
struct RawshiftErrorMgr {
  struct jpeg_error_mgr base;
  jmp_buf setjmp_buffer;
  char message[JMSG_LENGTH_MAX];
};

// Fatal-error hook: format the message and jump back to the setjmp point rather
// than letting jpegli's default handler call exit().
void RawshiftErrorExit(j_common_ptr cinfo) {
  RawshiftErrorMgr* err = reinterpret_cast<RawshiftErrorMgr*>(cinfo->err);
  (*cinfo->err->format_message)(cinfo, err->message);
  longjmp(err->setjmp_buffer, 1);
}

// Swallow non-fatal trace/warning output so the library never writes to stderr.
void RawshiftEmitMessage(j_common_ptr /*cinfo*/, int /*msg_level*/) {}

}  // namespace

extern "C" int rawshift_jpegli_encode(const RawshiftJpegliInput* in,
                                      uint8_t** out, size_t* out_len, char* err,
                                      size_t err_cap) {
  *out = nullptr;
  *out_len = 0;

  if (in->width == 0 || in->height == 0 || in->width > 65535 ||
      in->height > 65535) {
    snprintf(err, err_cap, "invalid JPEG dimensions %ux%u (max 65535)",
             in->width, in->height);
    return 1;
  }
  const size_t bytes_per_sample = (in->bits_per_sample == 16) ? 2 : 1;
  const size_t expected =
      static_cast<size_t>(in->width) * in->height * 3 * bytes_per_sample;
  if (in->pixels_len != expected) {
    snprintf(err, err_cap,
             "pixel buffer length mismatch: expected %zu, got %zu", expected,
             in->pixels_len);
    return 1;
  }

  struct jpeg_compress_struct cinfo;
  struct RawshiftErrorMgr jerr;
  unsigned char* buffer = nullptr;  // jpegli_mem_dest allocates with malloc
  unsigned long size = 0;

  cinfo.err = jpegli_std_error(&jerr.base);
  jerr.base.error_exit = RawshiftErrorExit;
  jerr.base.emit_message = RawshiftEmitMessage;

  // Any jpegli fatal error longjmps back here.
  if (setjmp(jerr.setjmp_buffer)) {
    snprintf(err, err_cap, "%s", jerr.message);
    jpegli_destroy_compress(&cinfo);
    if (buffer) free(buffer);
    return 1;
  }

  jpegli_create_compress(&cinfo);
  jpegli_mem_dest(&cinfo, &buffer, &size);

  cinfo.image_width = in->width;
  cinfo.image_height = in->height;
  cinfo.input_components = 3;
  cinfo.in_color_space = JCS_RGB;

  // `set_input_format` and `set_xyb_mode` must precede `set_defaults`.
  if (in->bits_per_sample == 16) {
    jpegli_set_input_format(&cinfo, JPEGLI_TYPE_UINT16, JPEGLI_NATIVE_ENDIAN);
  }
  if (in->xyb) {
    jpegli_set_xyb_mode(&cinfo);
  }

  jpegli_set_defaults(&cinfo);

  // Rate control must follow `set_defaults`.
  if (in->use_distance) {
    jpegli_set_distance(&cinfo, in->distance, FALSE);
  } else {
    jpegli_set_quality(&cinfo, in->quality, FALSE);
  }

  jpegli_set_progressive_level(&cinfo, in->progressive ? 2 : 0);

  // Chroma subsampling (XYB mode picks its own).
  if (!in->xyb) {
    int h = 1, v = 1;
    switch (in->subsampling) {
      case RAWSHIFT_JPEGLI_SUBSAMPLE_420:
        h = 2;
        v = 2;
        break;
      case RAWSHIFT_JPEGLI_SUBSAMPLE_422:
        h = 2;
        v = 1;
        break;
      default:  // 4:4:4
        break;
    }
    cinfo.comp_info[0].h_samp_factor = h;
    cinfo.comp_info[0].v_samp_factor = v;
    for (int c = 1; c < 3; ++c) {
      cinfo.comp_info[c].h_samp_factor = 1;
      cinfo.comp_info[c].v_samp_factor = 1;
    }
  }

  jpegli_start_compress(&cinfo, TRUE);

  const size_t row_stride =
      static_cast<size_t>(in->width) * 3 * bytes_per_sample;
  while (cinfo.next_scanline < cinfo.image_height) {
    JSAMPROW row = const_cast<JSAMPROW>(reinterpret_cast<const JSAMPLE*>(
        in->pixels + static_cast<size_t>(cinfo.next_scanline) * row_stride));
    jpegli_write_scanlines(&cinfo, &row, 1);
  }

  jpegli_finish_compress(&cinfo);
  jpegli_destroy_compress(&cinfo);

  *out = buffer;
  *out_len = static_cast<size_t>(size);
  return 0;
}

extern "C" void rawshift_jpegli_free(uint8_t* ptr) { free(ptr); }
