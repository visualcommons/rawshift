/* C ABI shim over jpegli's libjpeg-style encode API.
 *
 * jpegli (like libjpeg) signals fatal errors by calling `error_exit`, whose
 * default implementation calls `exit()`. That cannot be driven safely from
 * Rust, so the whole compress sequence lives here in C++ behind a `setjmp`
 * guard and is exposed as a single return-code function — the same clean
 * `Result`-shaped boundary the libjxl backend gets from libjxl's own C API.
 *
 * The implementation (jpegli_shim.cc) is the only place that includes jpegli's
 * C++ headers; this header is self-contained (stdint/stddef only) so bindgen
 * can process it without the jpegli include paths.
 */
#ifndef RAWSHIFT_JPEGLI_SHIM_H
#define RAWSHIFT_JPEGLI_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Chroma subsampling preset. Applied only in non-XYB mode; in XYB mode jpegli
 * chooses its own sampling and this field is ignored. */
typedef enum {
  RAWSHIFT_JPEGLI_SUBSAMPLE_420 = 0, /* 4:2:0 */
  RAWSHIFT_JPEGLI_SUBSAMPLE_422 = 1, /* 4:2:2 */
  RAWSHIFT_JPEGLI_SUBSAMPLE_444 = 2  /* 4:4:4 */
} RawshiftJpegliSubsampling;

/* Fully-resolved encode request. `pixels` is interleaved RGB with no alpha:
 * width*height*3 bytes for `bits_per_sample == 8`, or width*height*3*2
 * native-endian bytes for `bits_per_sample == 16`. */
typedef struct {
  const uint8_t* pixels;
  size_t pixels_len;
  uint32_t width;
  uint32_t height;
  uint32_t bits_per_sample; /* 8 or 16 */
  int use_distance;         /* 1 => use `distance`; 0 => use `quality` */
  float distance;           /* Butteraugli distance (used when use_distance) */
  int quality;              /* 1..=100 (used when !use_distance) */
  int progressive;          /* 0/1 */
  int xyb;                  /* 0/1 */
  int subsampling;          /* RawshiftJpegliSubsampling */
} RawshiftJpegliInput;

/* Encode `in` to a JPEG. On success returns 0 and sets `*out` to a malloc'd
 * buffer of `*out_len` bytes (free it with rawshift_jpegli_free). On failure
 * returns non-zero and writes a NUL-terminated message into `err` (capacity
 * `err_cap`); `*out` is left NULL. */
int rawshift_jpegli_encode(const RawshiftJpegliInput* in, uint8_t** out,
                           size_t* out_len, char* err, size_t err_cap);

/* Free a buffer returned by rawshift_jpegli_encode. */
void rawshift_jpegli_free(uint8_t* ptr);

#ifdef __cplusplus
}
#endif

#endif /* RAWSHIFT_JPEGLI_SHIM_H */
