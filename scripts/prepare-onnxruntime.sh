#!/usr/bin/env bash
set -euo pipefail

: "${TARGET:?TARGET is required}"
ONNX_RUNTIME_VERSION="${ONNX_RUNTIME_VERSION:-1.24.2}"
ONNX_RUNTIME_CACHE_DIR="${ONNX_RUNTIME_CACHE_DIR:-target/onnxruntime}"

case "$TARGET" in
  x86_64-unknown-linux-gnu)
    archive_arch=x64
    expected_sha256=43725474ba5663642e17684717946693850e2005efbd724ac72da278fead25e6
    ;;
  aarch64-unknown-linux-gnu)
    archive_arch=aarch64
    expected_sha256=6715b3d19965a2a6981e78ed4ba24f17a8c30d2d26420dbed10aac7ceca0085e
    ;;
  *)
    exit 0
    ;;
esac

destination="$ONNX_RUNTIME_CACHE_DIR/$TARGET"
library="$destination/lib/libonnxruntime.so.$ONNX_RUNTIME_VERSION"
if [[ -f "$library" ]]; then
  printf '%s\n' "$library"
  exit 0
fi

archive="onnxruntime-linux-${archive_arch}-${ONNX_RUNTIME_VERSION}.tgz"
url="https://github.com/microsoft/onnxruntime/releases/download/v${ONNX_RUNTIME_VERSION}/${archive}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL --retry 3 -o "$tmp/$archive" "$url"
printf '%s  %s\n' "$expected_sha256" "$tmp/$archive" | sha256sum --check --status
mkdir -p "$destination"
tar -xzf "$tmp/$archive" -C "$destination" --strip-components=1

if [[ ! -f "$library" ]]; then
  echo "ONNX Runtime archive did not contain $library" >&2
  exit 1
fi

printf '%s\n' "$library"
