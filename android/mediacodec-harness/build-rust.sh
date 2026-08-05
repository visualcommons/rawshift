#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: build-rust.sh <jni-output-directory>" >&2
  exit 2
fi

ndk_root=${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}
if [[ -z "${ndk_root}" ]]; then
  echo "ANDROID_NDK_HOME must point to NDK 29.0.14206865" >&2
  exit 2
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) host_tag=linux-x86_64 ;;
  Darwin-x86_64|Darwin-arm64) host_tag=darwin-x86_64 ;;
  *) echo "unsupported build host: $(uname -s)-$(uname -m)" >&2; exit 2 ;;
esac

linker="${ndk_root}/toolchains/llvm/prebuilt/${host_tag}/bin/aarch64-linux-android29-clang"
if [[ ! -x "${linker}" ]]; then
  echo "Android API 29 linker not found: ${linker}" >&2
  exit 2
fi

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
output_dir=$1

rustup target add aarch64-linux-android
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="${linker}" \
  cargo build \
    --manifest-path "${script_dir}/native/Cargo.toml" \
    --target aarch64-linux-android \
    --release

mkdir -p "${output_dir}"
cp "${script_dir}/native/target/aarch64-linux-android/release/librawshift_mediacodec_harness_native.so" \
  "${output_dir}/librawshift_mediacodec_harness_native.so"
