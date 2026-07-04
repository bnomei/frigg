#!/usr/bin/env bash
set -euo pipefail

: "${TARGET:?TARGET is required}"
PACKAGE_NAME="${PACKAGE_NAME:-frigg}"

BUILD_ARGS=(build --locked --release -p "$PACKAGE_NAME" --target "$TARGET")

target_omits_default_features() {
  case "$TARGET" in
    *musl* | x86_64-apple-darwin) return 0 ;;
    *) return 1 ;;
  esac
}

if [[ "${NO_DEFAULT_FEATURES:-}" == "1" ]] || target_omits_default_features; then
  BUILD_ARGS+=(--no-default-features)
fi
if [[ -n "${FEATURES:-}" ]]; then
  BUILD_ARGS+=(--features "$FEATURES")
fi

musl_cc() {
  case "$TARGET" in
    x86_64-unknown-linux-musl) printf '%s\n' x86_64-linux-musl-gcc ;;
    aarch64-unknown-linux-musl) printf '%s\n' aarch64-linux-musl-gcc ;;
    *) printf '%s\n' "" ;;
  esac
}

if [[ "$TARGET" == *"musl"* ]]; then
  if command -v cross >/dev/null 2>&1; then
    cross "${BUILD_ARGS[@]}"
  elif [[ -n "$(musl_cc)" ]] && command -v "$(musl_cc)" >/dev/null 2>&1; then
    cargo "${BUILD_ARGS[@]}"
  else
    cat >&2 <<EOF
Missing musl C toolchain for ${TARGET}.

Install cross and rerun:
  cargo install cross
  TARGET=${TARGET} scripts/build-release.sh

Or install $(musl_cc) on PATH and rerun with cargo.
EOF
    exit 1
  fi
else
  cargo "${BUILD_ARGS[@]}"
fi
