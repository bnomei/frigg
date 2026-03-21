#!/usr/bin/env bash
set -euo pipefail

: "${TARGET:?TARGET is required}"
PACKAGE_NAME="${PACKAGE_NAME:-frigg}"

if [[ "$TARGET" == *"musl"* ]]; then
  if command -v cross >/dev/null 2>&1; then
    cross build --locked --release -p "$PACKAGE_NAME" --target "$TARGET"
  else
    cargo build --locked --release -p "$PACKAGE_NAME" --target "$TARGET"
  fi
else
  cargo build --locked --release -p "$PACKAGE_NAME" --target "$TARGET"
fi
