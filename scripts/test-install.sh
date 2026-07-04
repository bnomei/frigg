#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
INSTALLER="$SCRIPT_DIR/install.sh"

fail() {
  printf 'not ok - %s\n' "$*" >&2
  exit 1
}

pass() {
  printf 'ok - %s\n' "$*"
}

assert_contains() {
  haystack=$1
  needle=$2
  label=$3
  case "$haystack" in
    *"$needle"*) pass "$label" ;;
    *) fail "$label: expected output to contain '$needle'. Output was: $haystack" ;;
  esac
}

target() {
  os=$(uname -s)
  arch=$(uname -m)

  case "$os:$arch" in
    Linux:x86_64|Linux:amd64) printf '%s\n' x86_64-unknown-linux-gnu ;;
    Linux:aarch64|Linux:arm64) printf '%s\n' aarch64-unknown-linux-gnu ;;
    Darwin:x86_64|Darwin:amd64) printf '%s\n' x86_64-apple-darwin ;;
    Darwin:aarch64|Darwin:arm64) printf '%s\n' aarch64-apple-darwin ;;
    *) fail "unsupported test host: $os $arch" ;;
  esac
}

sha256_file() {
  path=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1; exit}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1; exit}'
  else
    fail "no SHA-256 tool available for fixture setup"
  fi
}

setup_tmp() {
  tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/frigg-install-test.XXXXXX")
  fixture_dir=$tmp_dir/fixtures
  fakebin=$tmp_dir/bin
  install_dir=$tmp_dir/install
  mkdir -p "$fixture_dir" "$fakebin" "$install_dir"
}

cleanup() {
  if [ -n "${tmp_dir:-}" ] && [ -d "$tmp_dir" ]; then
    rm -rf "$tmp_dir"
  fi
}

make_fixture() {
  version=$1
  release_target=$2
  archive_name="frigg-v${version}-${release_target}.tar.gz"
  build_dir=$tmp_dir/build
  mkdir -p "$build_dir"
  printf '#!/bin/sh\nprintf "frigg fixture\\n"\n' > "$build_dir/frigg"
  chmod 755 "$build_dir/frigg"
  tar -C "$build_dir" -czf "$fixture_dir/$archive_name" frigg
  digest=$(sha256_file "$fixture_dir/$archive_name")
  printf '%s  %s\n' "$digest" "$archive_name" > "$fixture_dir/$archive_name.sha256"
}

make_fake_curl() {
  cat > "$fakebin/curl" <<'EOF'
#!/bin/sh
out=
url=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      shift
      out=$1
      ;;
    http://*|https://*)
      url=$1
      ;;
  esac
  shift
done

case "$url" in
  https://api.github.com/repos/bnomei/frigg/releases/latest)
    printf '{"tag_name":"v9.9.9"}\n'
    exit 0
    ;;
  https://github.com/bnomei/frigg/releases/download/v9.9.9/*)
    file=${url##*/}
    if [ -n "$out" ]; then
      cp "$FRIGG_FIXTURE_DIR/$file" "$out"
    else
      cat "$FRIGG_FIXTURE_DIR/$file"
    fi
    exit 0
    ;;
  *)
    printf 'unexpected curl URL: %s\n' "$url" >&2
    exit 9
    ;;
esac
EOF
  chmod 755 "$fakebin/curl"
}

test_help_mentions_supported_targets() {
  output=$(sh "$INSTALLER" --help)
  assert_contains "$output" "curl -fsSL" "help shows minimal curl install"
  assert_contains "$output" "FRIGG_VERSION" "help documents pinned version"
  assert_contains "$output" "Windows users should use" "help keeps Windows path"
}

test_pinned_version_installs_binary() {
  PATH="$fakebin:$PATH" FRIGG_FIXTURE_DIR="$fixture_dir" FRIGG_VERSION=9.9.9 FRIGG_INSTALL_DIR="$install_dir" sh "$INSTALLER" >/dev/null
  [ -x "$install_dir/frigg" ] || fail "pinned version installs executable frigg binary"
  assert_contains "$("$install_dir/frigg")" "frigg fixture" "installed binary runs"
}

test_latest_version_installs_binary() {
  PATH="$fakebin:$PATH" FRIGG_FIXTURE_DIR="$fixture_dir" FRIGG_INSTALL_DIR="$install_dir" sh "$INSTALLER" >/dev/null
  [ -x "$install_dir/frigg" ] || fail "latest version installs executable frigg binary"
  pass "latest version installs executable frigg binary"
}

test_checksum_failure_does_not_install() {
  printf '0000000000000000000000000000000000000000000000000000000000000000  frigg-v9.9.9-%s.tar.gz\n' "$release_target" > "$fixture_dir/frigg-v9.9.9-$release_target.tar.gz.sha256"
  if PATH="$fakebin:$PATH" FRIGG_FIXTURE_DIR="$fixture_dir" FRIGG_VERSION=v9.9.9 FRIGG_INSTALL_DIR="$install_dir" sh "$INSTALLER" >/dev/null 2>&1; then
    fail "checksum failure should fail installer"
  fi
  [ ! -e "$install_dir/frigg" ] || fail "checksum failure should not install frigg"
  pass "checksum failure fails before install"
}

main() {
  setup_tmp
  trap cleanup 0 1 2 3 15
  release_target=$(target)
  make_fixture 9.9.9 "$release_target"
  make_fake_curl

  test_help_mentions_supported_targets
  test_pinned_version_installs_binary
  rm -f "$install_dir/frigg"
  test_latest_version_installs_binary
  rm -f "$install_dir/frigg"
  test_checksum_failure_does_not_install
}

main "$@"
