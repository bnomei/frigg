#!/bin/sh
set -eu

bin=frigg
repo=bnomei/frigg

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Install Frigg from a GitHub Release archive.

Usage:
  curl -fsSL https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh | sh

Environment:
  FRIGG_VERSION      Release version or tag, for example 0.10.1 or v0.10.1.
                     Defaults to the latest GitHub Release.
  FRIGG_INSTALL_DIR  Install directory. Defaults to $HOME/.local/bin.

Supported targets:
  x86_64-unknown-linux-gnu
  aarch64-unknown-linux-gnu
  x86_64-apple-darwin
  aarch64-apple-darwin

Windows users should use the release .zip asset or Scoop.
EOF
}

target() {
  os=$(uname -s)
  arch=$(uname -m)

  case "$os:$arch" in
    Linux:x86_64|Linux:amd64) printf '%s\n' x86_64-unknown-linux-gnu ;;
    Linux:aarch64|Linux:arm64) printf '%s\n' aarch64-unknown-linux-gnu ;;
    Darwin:x86_64|Darwin:amd64) printf '%s\n' x86_64-apple-darwin ;;
    Darwin:aarch64|Darwin:arm64) printf '%s\n' aarch64-apple-darwin ;;
    Linux:*) die "unsupported GNU/glibc Linux architecture '$arch'" ;;
    Darwin:*) die "unsupported macOS architecture '$arch'" ;;
    *) die "unsupported operating system '$os'; use a release asset instead" ;;
  esac
}

tag() {
  if [ -n "${FRIGG_VERSION:-}" ]; then
    case "$FRIGG_VERSION" in
      v*) printf '%s\n' "$FRIGG_VERSION" ;;
      *) printf 'v%s\n' "$FRIGG_VERSION" ;;
    esac
    return
  fi

  json=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest") \
    || die "failed to resolve latest Frigg release"
  latest=$(printf '%s\n' "$json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
  [ -n "$latest" ] || die "latest Frigg release response did not include tag_name"
  printf '%s\n' "$latest"
}

install_dir() {
  if [ -n "${FRIGG_INSTALL_DIR:-}" ]; then
    printf '%s\n' "$FRIGG_INSTALL_DIR"
  else
    [ -n "${HOME:-}" ] || die "HOME is not set; set FRIGG_INSTALL_DIR"
    printf '%s/.local/bin\n' "$HOME"
  fi
}

verify_sha256() {
  archive_path=$1
  checksum_path=$2
  archive_dir=$(dirname "$archive_path")
  checksum_file=$(basename "$checksum_path")

  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$archive_dir" && sha256sum -c "$checksum_file") || die "checksum mismatch"
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    expected=$(awk '{print $1; exit}' "$checksum_path")
    actual=$(shasum -a 256 "$archive_path" | awk '{print $1; exit}')
    [ -n "$expected" ] || die "checksum file did not contain a SHA-256 digest"
    [ "$actual" = "$expected" ] || die "checksum mismatch"
    return
  fi

  die "sha256sum or shasum is required to verify the release archive"
}

main() {
  case "${1:-}" in
    -h|--help)
      usage
      exit 0
      ;;
    "")
      ;;
    *)
      usage >&2
      die "unknown argument '$1'"
      ;;
  esac

  target=$(target)
  tag=$(tag)
  install_dir=$(install_dir)
  archive="$bin-$tag-$target.tar.gz"
  url="https://github.com/$repo/releases/download/$tag/$archive"

  tmp=$(mktemp -d "${TMPDIR:-/tmp}/frigg-install.XXXXXX") || die "failed to create temporary directory"
  trap 'rm -rf "$tmp"' 0 1 2 3 15

  printf 'Downloading %s\n' "$url"
  curl -fsSL -o "$tmp/$archive" "$url" || die "failed to download release archive"
  curl -fsSL -o "$tmp/$archive.sha256" "$url.sha256" || die "failed to download release checksum"

  verify_sha256 "$tmp/$archive" "$tmp/$archive.sha256"
  tar -xzf "$tmp/$archive" -C "$tmp" || die "failed to extract release archive"
  [ -f "$tmp/$bin" ] || die "release archive did not contain $bin"

  mkdir -p "$install_dir" || die "failed to create install directory '$install_dir'"
  cp "$tmp/$bin" "$install_dir/$bin" || die "failed to install $bin"
  chmod 755 "$install_dir/$bin" || die "failed to mark $bin executable"

  if [ -n "${GITHUB_PATH:-}" ] && [ -e "$GITHUB_PATH" ]; then
    printf '%s\n' "$install_dir" >> "$GITHUB_PATH"
  fi

  printf 'Installed %s to %s\n' "$bin" "$install_dir/$bin"
}

main "$@"
