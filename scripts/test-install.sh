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

sha256_file() {
  path=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1; exit}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1; exit}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$path" | awk '{print $NF; exit}'
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
  target=$2
  archive_name="frigg-v${version}-${target}.tar.gz"
  build_dir=$tmp_dir/build
  mkdir -p "$build_dir"
  cat > "$build_dir/frigg" <<'EOF'
#!/bin/sh
printf 'frigg fixture\n'
EOF
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
    --fail|--location|--silent|--show-error)
      ;;
    --proto|--tlsv1.2)
      if [ "$1" = "--proto" ]; then
        shift
      fi
      ;;
    https://*)
      url=$1
      ;;
  esac
  shift
done

case "$url" in
  https://api.github.com/repos/example/frigg/releases/latest)
    printf '{"tag_name":"v9.9.9"}\n'
    exit 0
    ;;
  https://github.com/example/frigg/releases/download/v9.9.9/*)
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
  assert_contains "$output" "x86_64-unknown-linux-gnu" "help lists Linux x86_64 target"
  assert_contains "$output" "GNU/glibc Linux" "help names GNU/glibc Linux"
  assert_contains "$output" "Windows users should download" "help keeps Windows manual"
}

test_dry_run_target_mapping() {
  output=$(FRIGG_VERSION=0.5.0 FRIGG_INSTALL_DRY_RUN=1 FRIGG_INSTALL_OS=Linux FRIGG_INSTALL_ARCH=aarch64 sh "$INSTALLER")
  assert_contains "$output" "target:       aarch64-unknown-linux-gnu" "dry run maps Linux arm64 to GNU target"
  assert_contains "$output" "tag:          v0.5.0" "dry run normalizes bare version"
  assert_contains "$output" "frigg-v0.5.0-aarch64-unknown-linux-gnu.tar.gz" "dry run prints archive URL"

  output=$(FRIGG_VERSION=v0.5.0 FRIGG_INSTALL_DRY_RUN=1 FRIGG_INSTALL_OS=Darwin FRIGG_INSTALL_ARCH=arm64 sh "$INSTALLER")
  assert_contains "$output" "target:       aarch64-apple-darwin" "dry run maps macOS arm64 target"
  assert_contains "$output" "checksum URL:" "dry run prints checksum URL"
}

test_latest_dry_run_uses_resolved_tag() {
  output=$(PATH="$fakebin:$PATH" FRIGG_REPOSITORY=example/frigg FRIGG_INSTALL_DRY_RUN=1 FRIGG_INSTALL_TARGET=x86_64-unknown-linux-gnu sh "$INSTALLER")
  assert_contains "$output" "version:      9.9.9" "latest dry run prints resolved version"
  assert_contains "$output" "tag:          v9.9.9" "latest dry run prints resolved tag"
  assert_contains "$output" "frigg-v9.9.9-x86_64-unknown-linux-gnu.tar.gz" "latest dry run constructs versioned asset URL"
}

test_checksum_success_installs_binary() {
  PATH="$fakebin:$PATH" FRIGG_FIXTURE_DIR="$fixture_dir" FRIGG_REPOSITORY=example/frigg FRIGG_VERSION=v9.9.9 FRIGG_INSTALL_TARGET=x86_64-unknown-linux-gnu FRIGG_INSTALL_DIR="$install_dir" sh "$INSTALLER" >/dev/null
  [ -x "$install_dir/frigg" ] || fail "checksum success installs executable frigg binary"
  pass "checksum success installs executable frigg binary"
}

test_checksum_failure_does_not_install() {
  printf '0000000000000000000000000000000000000000000000000000000000000000  frigg-v9.9.9-x86_64-unknown-linux-gnu.tar.gz\n' > "$fixture_dir/frigg-v9.9.9-x86_64-unknown-linux-gnu.tar.gz.sha256"
  if PATH="$fakebin:$PATH" FRIGG_FIXTURE_DIR="$fixture_dir" FRIGG_REPOSITORY=example/frigg FRIGG_VERSION=v9.9.9 FRIGG_INSTALL_TARGET=x86_64-unknown-linux-gnu FRIGG_INSTALL_DIR="$install_dir" sh "$INSTALLER" >/dev/null 2>&1; then
    fail "checksum failure should fail installer"
  fi
  [ ! -e "$install_dir/frigg" ] || fail "checksum failure should not install frigg"
  pass "checksum failure fails before install"
}

main() {
  setup_tmp
  trap cleanup 0 1 2 3 15
  make_fixture 9.9.9 x86_64-unknown-linux-gnu
  make_fake_curl

  test_help_mentions_supported_targets
  test_dry_run_target_mapping
  test_latest_dry_run_uses_resolved_tag
  test_checksum_success_installs_binary
  rm -f "$install_dir/frigg"
  test_checksum_failure_does_not_install
}

main "$@"
