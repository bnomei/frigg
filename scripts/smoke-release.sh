#!/usr/bin/env bash
set -euo pipefail

: "${TARGET:?TARGET is required}"
BIN_NAME="${BIN_NAME:-frigg}"
OUT_DIR="${OUT_DIR:-dist}"
VERSION="${VERSION:-}"

ARCHIVE_PATH="${OUT_DIR}/${BIN_NAME}-v${VERSION}-${TARGET}.tar.gz"
if [[ -z "$VERSION" ]]; then
  archive_count=$(find "$OUT_DIR" -maxdepth 1 -type f -name "${BIN_NAME}-v*-${TARGET}.tar.gz" | wc -l | tr -d ' ')
  if [[ "$archive_count" != "1" ]]; then
    echo "Expected exactly one archive for target ${TARGET}, found ${archive_count}. Set VERSION to disambiguate." >&2
    exit 1
  fi
  ARCHIVE_PATH=$(find "$OUT_DIR" -maxdepth 1 -type f -name "${BIN_NAME}-v*-${TARGET}.tar.gz" | head -n 1)
fi

if [[ ! -f "$ARCHIVE_PATH" ]]; then
  echo "Archive not found: $ARCHIVE_PATH" >&2
  exit 1
fi

CHECKSUM_PATH="${ARCHIVE_PATH}.sha256"
if [[ ! -f "$CHECKSUM_PATH" ]]; then
  echo "Checksum not found: $CHECKSUM_PATH" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$(dirname "$ARCHIVE_PATH")" && sha256sum --check "$(basename "$CHECKSUM_PATH")")
elif command -v shasum >/dev/null 2>&1; then
  expected=$(awk '{print $1}' "$CHECKSUM_PATH")
  actual=$(shasum -a 256 "$ARCHIVE_PATH" | awk '{print $1}')
  if [[ "$actual" != "$expected" ]]; then
    echo "Checksum mismatch for $ARCHIVE_PATH" >&2
    exit 1
  fi
else
  echo "No SHA-256 tool found for checksum verification." >&2
  exit 1
fi

smoke_dir=$(mktemp -d)
cleanup() {
  rm -rf "$smoke_dir"
}
trap cleanup EXIT

tar -xzf "$ARCHIVE_PATH" -C "$smoke_dir"
BIN_PATH="${smoke_dir}/${BIN_NAME}"
if [[ ! -x "$BIN_PATH" ]]; then
  echo "Binary is not executable: $BIN_PATH" >&2
  exit 1
fi

"$BIN_PATH" --version >/dev/null
"$BIN_PATH" --help >/dev/null
"$BIN_PATH" serve --help >/dev/null
