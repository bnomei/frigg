#!/bin/sh
set -eu

BIN_NAME=frigg
DEFAULT_REPOSITORY=bnomei/frigg

usage() {
  cat <<'EOF'
Install Frigg from a GitHub Release archive.

Usage:
  sh scripts/install.sh
  sh scripts/install.sh --help

Environment:
  FRIGG_VERSION       Release version or tag, for example 0.5.0 or v0.5.0.
                      When unset, the latest GitHub Release tag is resolved.
  FRIGG_INSTALL_DIR   Install directory. Defaults to $HOME/.local/bin.
  FRIGG_REPOSITORY    GitHub repository. Defaults to bnomei/frigg.
  FRIGG_INSTALL_DRY_RUN
                      Print resolved install details without downloading.

Test-only target overrides:
  FRIGG_INSTALL_OS
  FRIGG_INSTALL_ARCH
  FRIGG_INSTALL_TARGET

Supported targets:
  x86_64-unknown-linux-gnu
  aarch64-unknown-linux-gnu
  x86_64-apple-darwin
  aarch64-apple-darwin

Linux release assets are for GNU/glibc Linux. Alpine/musl Linux is not supported
by this installer. Windows users should download the matching release asset
manually.
EOF
}

die() {
  printf '%s\n' "error: $*" >&2
  exit 1
}

curl_https() {
  curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error "$@"
}

strip_leading_v() {
  case "$1" in
    v*) printf '%s\n' "${1#v}" ;;
    *) printf '%s\n' "$1" ;;
  esac
}

normalize_tag() {
  case "$1" in
    v*) printf '%s\n' "$1" ;;
    *) printf 'v%s\n' "$1" ;;
  esac
}

detect_target() {
  if [ -n "${FRIGG_INSTALL_TARGET:-}" ]; then
    case "$FRIGG_INSTALL_TARGET" in
      x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin)
        printf '%s\n' "$FRIGG_INSTALL_TARGET"
        return
        ;;
      *)
        die "unsupported Frigg install target '$FRIGG_INSTALL_TARGET'. Supported targets are GNU/glibc Linux x86_64/aarch64 and macOS x86_64/aarch64."
        ;;
    esac
  fi

  os=${FRIGG_INSTALL_OS:-$(uname -s)}
  arch=${FRIGG_INSTALL_ARCH:-$(uname -m)}

  case "$os" in
    Linux)
      case "$arch" in
        x86_64|amd64) printf '%s\n' x86_64-unknown-linux-gnu ;;
        aarch64|arm64) printf '%s\n' aarch64-unknown-linux-gnu ;;
        *) die "unsupported GNU/glibc Linux architecture '$arch'. Supported Linux architectures are x86_64 and aarch64; Alpine/musl is not supported." ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64|amd64) printf '%s\n' x86_64-apple-darwin ;;
        aarch64|arm64) printf '%s\n' aarch64-apple-darwin ;;
        *) die "unsupported macOS architecture '$arch'. Supported macOS architectures are x86_64 and arm64." ;;
      esac
      ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      die "Windows installation is manual: download the matching Frigg release asset from GitHub."
      ;;
    *)
      die "unsupported operating system '$os'. This installer supports GNU/glibc Linux and macOS release assets only."
      ;;
  esac
}

resolve_latest_tag() {
  repository=$1
  api_url="https://api.github.com/repos/${repository}/releases/latest"
  json=$(curl_https "$api_url") || die "failed to resolve latest Frigg release from $api_url"
  tag=$(printf '%s\n' "$json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
  [ -n "$tag" ] || die "latest Frigg release response did not include a tag_name"
  case "$tag" in
    v*) printf '%s\n' "$tag" ;;
    *) die "latest Frigg release tag '$tag' is not a v-prefixed release tag" ;;
  esac
}

resolve_install_dir() {
  if [ -n "${FRIGG_INSTALL_DIR:-}" ]; then
    printf '%s\n' "$FRIGG_INSTALL_DIR"
    return
  fi

  [ -n "${HOME:-}" ] || die "HOME is not set and FRIGG_INSTALL_DIR was not provided"
  printf '%s\n' "$HOME/.local/bin"
}

print_plan() {
  printf 'Frigg installer plan\n'
  printf '  repository:   %s\n' "$repository"
  printf '  target:       %s\n' "$target"
  printf '  version:      %s\n' "$version"
  printf '  tag:          %s\n' "$tag"
  printf '  archive URL:  %s\n' "$archive_url"
  printf '  checksum URL: %s\n' "$checksum_url"
  printf '  install dir:  %s\n' "$install_dir"
}

download_file() {
  url=$1
  dest=$2
  curl_https -o "$dest" "$url"
}

verify_checksum() {
  archive_path=$1
  checksum_path=$2
  archive_base=$(basename "$archive_path")
  checksum_base=$(basename "$checksum_path")
  archive_dir=$(dirname "$archive_path")

  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$archive_dir" && sha256sum --check "$checksum_base")
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    expected=$(awk '{print $1; exit}' "$checksum_path" | tr 'A-F' 'a-f')
    actual=$(shasum -a 256 "$archive_path" | awk '{print $1; exit}' | tr 'A-F' 'a-f')
    [ -n "$expected" ] || die "checksum file '$checksum_path' did not contain a SHA-256 digest"
    [ "$actual" = "$expected" ] || die "checksum mismatch for $archive_base"
    return
  fi

  if command -v openssl >/dev/null 2>&1; then
    expected=$(awk '{print $1; exit}' "$checksum_path" | tr 'A-F' 'a-f')
    actual=$(openssl dgst -sha256 "$archive_path" | awk '{print $NF; exit}' | tr 'A-F' 'a-f')
    [ -n "$expected" ] || die "checksum file '$checksum_path' did not contain a SHA-256 digest"
    [ "$actual" = "$expected" ] || die "checksum mismatch for $archive_base"
    return
  fi

  die "no SHA-256 verifier found. Install sha256sum, shasum, or openssl before running this installer."
}

install_binary() {
  archive_path=$1
  install_dir=$2

  mkdir -p "$install_dir" || die "failed to create install directory '$install_dir'"
  [ -d "$install_dir" ] || die "install path '$install_dir' is not a directory"
  [ -w "$install_dir" ] || die "install directory '$install_dir' is not writable; choose FRIGG_INSTALL_DIR instead of using sudo"

  extract_dir=$tmp_dir/extract
  mkdir -p "$extract_dir" || die "failed to create extraction directory"
  tar -xzf "$archive_path" -C "$extract_dir" || die "failed to extract release archive"

  src=$extract_dir/$BIN_NAME
  [ -f "$src" ] || die "release archive did not contain the $BIN_NAME binary"
  cp "$src" "$install_dir/$BIN_NAME" || die "failed to install $BIN_NAME to '$install_dir'"
  chmod 755 "$install_dir/$BIN_NAME" || die "failed to mark installed $BIN_NAME as executable"

  if [ -n "${GITHUB_PATH:-}" ] && [ -e "$GITHUB_PATH" ]; then
    printf '%s\n' "$install_dir" >> "$GITHUB_PATH"
  fi
}

cleanup() {
  if [ -n "${tmp_dir:-}" ] && [ -d "$tmp_dir" ]; then
    rm -rf "$tmp_dir"
  fi
}

main() {
  case "${1:-}" in
    -h|--help)
      usage
      return 0
      ;;
    "")
      ;;
    *)
      usage >&2
      die "unknown argument '$1'"
      ;;
  esac

  repository=${FRIGG_REPOSITORY:-$DEFAULT_REPOSITORY}
  install_dir=$(resolve_install_dir)
  target=$(detect_target)

  if [ -n "${FRIGG_VERSION:-}" ]; then
    tag=$(normalize_tag "$FRIGG_VERSION")
  else
    tag=$(resolve_latest_tag "$repository")
  fi
  version=$(strip_leading_v "$tag")

  archive_name="${BIN_NAME}-${tag}-${target}.tar.gz"
  archive_url="https://github.com/${repository}/releases/download/${tag}/${archive_name}"
  checksum_url="${archive_url}.sha256"

  if [ -n "${FRIGG_INSTALL_DRY_RUN:-}" ]; then
    print_plan
    return 0
  fi

  tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/frigg-install.XXXXXX") || die "failed to create temporary directory"
  trap cleanup 0 1 2 3 15

  archive_path=$tmp_dir/$archive_name
  checksum_path=$tmp_dir/$archive_name.sha256

  printf 'Downloading %s\n' "$archive_url"
  download_file "$archive_url" "$archive_path" || die "failed to download release archive"
  printf 'Downloading %s\n' "$checksum_url"
  download_file "$checksum_url" "$checksum_path" || die "failed to download release checksum"

  verify_checksum "$archive_path" "$checksum_path" || die "checksum verification failed"
  install_binary "$archive_path" "$install_dir"
  printf 'Installed %s to %s\n' "$BIN_NAME" "$install_dir/$BIN_NAME"
}

main "$@"
