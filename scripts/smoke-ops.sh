#!/usr/bin/env sh
set -eu

fail() {
  echo "smoke-ops failed: $1" >&2
  exit 1
}

is_uint() {
  case "$1" in
    "" | *[!0-9]*)
      return 1
      ;;
    *)
      return 0
      ;;
  esac
}

extract_field() {
  key="$1"
  line="$2"
  value="$(echo "$line" | awk -v key="$key" '{
    for (i = 1; i <= NF; i++) {
      split($i, pair, "=")
      if (pair[1] == key) {
        print pair[2]
        exit 0
      }
    }
    exit 1
  }')"
  [ -n "$value" ] || fail "missing key '$key' in summary line: $line"
  echo "$value"
}

assert_single_summary_line() {
  file="$1"
  pattern="$2"
  label="$3"

  count="$(grep -Ec "$pattern" "$file" || true)"
  [ "$count" -eq 1 ] || fail "$label expected exactly one summary line matching pattern '$pattern'"
  grep -E "$pattern" "$file"
}

run_frigg() {
  label="$1"
  shift

  output_file="$output_dir/$label.out"
  if ! "$frigg_bin" "$@" >"$output_file" 2>&1; then
    cat "$output_file" >&2 || true
    fail "$label command failed: frigg $*"
  fi

  if grep -Fq "summary status=failed" "$output_file"; then
    cat "$output_file" >&2 || true
    fail "$label emitted a failed summary"
  fi

  echo "$output_file"
}

run_frigg_expect_fail_fast() {
  label="$1"
  shift

  output_file="$output_dir/$label.out"
  "$frigg_bin" "$@" >"$output_file" 2>&1 &
  pid=$!

  elapsed=0
  while kill -0 "$pid" >/dev/null 2>&1; do
    if [ "$elapsed" -ge 5 ]; then
      kill "$pid" >/dev/null 2>&1 || true
      wait "$pid" >/dev/null 2>&1 || true
      cat "$output_file" >&2 || true
      fail "$label command did not fail within timeout: frigg $*"
    fi
    sleep 1
    elapsed=$((elapsed + 1))
  done

  if wait "$pid"; then
    cat "$output_file" >&2 || true
    fail "$label command unexpectedly succeeded: frigg $*"
  fi

  if ! grep -Fq "summary status=failed" "$output_file"; then
    cat "$output_file" >&2 || true
    fail "$label expected deterministic failed summary output"
  fi

  echo "$output_file"
}

command -v cargo >/dev/null 2>&1 || fail "cargo is required"
command -v python3 >/dev/null 2>&1 || fail "python3 is required"

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$repo_root"

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/frigg-smoke-ops.XXXXXX")"
workspace_root="$tmp_root/workspace"
output_dir="$tmp_root/output"

cleanup() {
  rm -rf "$tmp_root"
}
trap cleanup EXIT INT TERM HUP

mkdir -p "$workspace_root/src/nested" "$output_dir"

frigg_bin="$repo_root/target/debug/frigg"
build_output="$output_dir/build.out"
if ! CARGO_TERM_COLOR=never cargo build -q -p frigg >"$build_output" 2>&1; then
  cat "$build_output" >&2 || true
  fail "failed to build frigg binary"
fi

[ -x "$frigg_bin" ] || fail "missing executable frigg binary at $frigg_bin"

cat >"$workspace_root/README.md" <<'EOF'
# Smoke Fixture
Deterministic workspace fixture for operational command checks.
EOF

cat >"$workspace_root/src/lib.rs" <<'EOF'
pub fn fixture_value() -> &'static str {
    "v1"
}
EOF

cat >"$workspace_root/src/nested/data.txt" <<'EOF'
fixture-data-v1
EOF

init_out="$(run_frigg init-1 init --workspace-root "$workspace_root")"
init_summary="$(assert_single_summary_line "$init_out" '^init summary status=ok repositories=[0-9]+$' "init")"
[ "$(extract_field repositories "$init_summary")" = "1" ] || fail "init expected repositories=1"

verify_out="$(run_frigg verify-1 verify --workspace-root "$workspace_root")"
verify_summary="$(assert_single_summary_line "$verify_out" '^verify summary status=ok repositories=[0-9]+$' "verify")"
[ "$(extract_field repositories "$verify_summary")" = "1" ] || fail "verify expected repositories=1"

full_out="$(run_frigg reindex-full reindex --workspace-root "$workspace_root")"
full_summary="$(assert_single_summary_line "$full_out" '^reindex summary status=ok mode=[^[:space:]]+ repositories=[0-9]+ files_scanned=[0-9]+ files_changed=[0-9]+ files_deleted=[0-9]+ diagnostics_total=[0-9]+ diagnostics_walk=[0-9]+ diagnostics_read=[0-9]+ duration_ms=[0-9]+$' "reindex full")"
full_mode="$(extract_field mode "$full_summary")"
full_repos="$(extract_field repositories "$full_summary")"
full_scanned="$(extract_field files_scanned "$full_summary")"
full_changed="$(extract_field files_changed "$full_summary")"
full_deleted="$(extract_field files_deleted "$full_summary")"
full_diag_total="$(extract_field diagnostics_total "$full_summary")"
full_diag_walk="$(extract_field diagnostics_walk "$full_summary")"
full_diag_read="$(extract_field diagnostics_read "$full_summary")"

[ "$full_repos" = "1" ] || fail "reindex full expected repositories=1"
is_uint "$full_scanned" || fail "reindex full files_scanned must be numeric"
is_uint "$full_changed" || fail "reindex full files_changed must be numeric"
is_uint "$full_deleted" || fail "reindex full files_deleted must be numeric"
is_uint "$full_diag_total" || fail "reindex full diagnostics_total must be numeric"
is_uint "$full_diag_walk" || fail "reindex full diagnostics_walk must be numeric"
is_uint "$full_diag_read" || fail "reindex full diagnostics_read must be numeric"
[ "$full_changed" -eq "$full_scanned" ] || fail "reindex full expected files_changed=files_scanned"
[ "$full_diag_total" -eq 0 ] || fail "reindex full expected diagnostics_total=0"
[ "$full_diag_walk" -eq 0 ] || fail "reindex full expected diagnostics_walk=0"
[ "$full_diag_read" -eq 0 ] || fail "reindex full expected diagnostics_read=0"

changed_zero_out="$(run_frigg reindex-changed-0 reindex --changed --workspace-root "$workspace_root")"
changed_zero_summary="$(assert_single_summary_line "$changed_zero_out" '^reindex summary status=ok mode=[^[:space:]]+ repositories=[0-9]+ files_scanned=[0-9]+ files_changed=[0-9]+ files_deleted=[0-9]+ diagnostics_total=[0-9]+ diagnostics_walk=[0-9]+ diagnostics_read=[0-9]+ duration_ms=[0-9]+$' "reindex changed (baseline)")"
changed_mode="$(extract_field mode "$changed_zero_summary")"
changed_repos="$(extract_field repositories "$changed_zero_summary")"
changed_zero_scanned="$(extract_field files_scanned "$changed_zero_summary")"
changed_zero_changed="$(extract_field files_changed "$changed_zero_summary")"
changed_zero_deleted="$(extract_field files_deleted "$changed_zero_summary")"
changed_zero_diag_total="$(extract_field diagnostics_total "$changed_zero_summary")"
changed_zero_diag_walk="$(extract_field diagnostics_walk "$changed_zero_summary")"
changed_zero_diag_read="$(extract_field diagnostics_read "$changed_zero_summary")"

[ "$changed_repos" = "1" ] || fail "reindex changed expected repositories=1"
[ "$changed_mode" != "$full_mode" ] || fail "reindex --changed mode should differ from full mode"
is_uint "$changed_zero_scanned" || fail "reindex changed files_scanned must be numeric"
is_uint "$changed_zero_changed" || fail "reindex changed files_changed must be numeric"
is_uint "$changed_zero_deleted" || fail "reindex changed files_deleted must be numeric"
is_uint "$changed_zero_diag_total" || fail "reindex changed diagnostics_total must be numeric"
is_uint "$changed_zero_diag_walk" || fail "reindex changed diagnostics_walk must be numeric"
is_uint "$changed_zero_diag_read" || fail "reindex changed diagnostics_read must be numeric"
[ "$changed_zero_changed" -eq 0 ] || fail "reindex --changed baseline expected files_changed=0"
[ "$changed_zero_deleted" -eq 0 ] || fail "reindex --changed baseline expected files_deleted=0"
[ "$changed_zero_diag_total" -eq 0 ] || fail "reindex --changed baseline expected diagnostics_total=0"
[ "$changed_zero_diag_walk" -eq 0 ] || fail "reindex --changed baseline expected diagnostics_walk=0"
[ "$changed_zero_diag_read" -eq 0 ] || fail "reindex --changed baseline expected diagnostics_read=0"

printf '\n// mutation-v2\n' >>"$workspace_root/src/lib.rs"

changed_mutation_out="$(run_frigg reindex-changed-1 reindex --changed --workspace-root "$workspace_root")"
changed_mutation_summary="$(assert_single_summary_line "$changed_mutation_out" '^reindex summary status=ok mode=[^[:space:]]+ repositories=[0-9]+ files_scanned=[0-9]+ files_changed=[0-9]+ files_deleted=[0-9]+ diagnostics_total=[0-9]+ diagnostics_walk=[0-9]+ diagnostics_read=[0-9]+ duration_ms=[0-9]+$' "reindex changed (mutated)")"
changed_mutation_changed="$(extract_field files_changed "$changed_mutation_summary")"
changed_mutation_deleted="$(extract_field files_deleted "$changed_mutation_summary")"
changed_mutation_diag_total="$(extract_field diagnostics_total "$changed_mutation_summary")"
changed_mutation_diag_walk="$(extract_field diagnostics_walk "$changed_mutation_summary")"
changed_mutation_diag_read="$(extract_field diagnostics_read "$changed_mutation_summary")"

is_uint "$changed_mutation_changed" || fail "reindex changed (mutated) files_changed must be numeric"
is_uint "$changed_mutation_deleted" || fail "reindex changed (mutated) files_deleted must be numeric"
is_uint "$changed_mutation_diag_total" || fail "reindex changed (mutated) diagnostics_total must be numeric"
is_uint "$changed_mutation_diag_walk" || fail "reindex changed (mutated) diagnostics_walk must be numeric"
is_uint "$changed_mutation_diag_read" || fail "reindex changed (mutated) diagnostics_read must be numeric"
[ "$changed_mutation_changed" -ge 1 ] || fail "reindex --changed after mutation expected files_changed>=1"
[ "$changed_mutation_deleted" -eq 0 ] || fail "reindex --changed after mutation expected files_deleted=0"
[ "$changed_mutation_diag_total" -eq 0 ] || fail "reindex --changed after mutation expected diagnostics_total=0"
[ "$changed_mutation_diag_walk" -eq 0 ] || fail "reindex --changed after mutation expected diagnostics_walk=0"
[ "$changed_mutation_diag_read" -eq 0 ] || fail "reindex --changed after mutation expected diagnostics_read=0"

init_second_out="$(run_frigg init-2 init --workspace-root "$workspace_root")"
init_second_summary="$(assert_single_summary_line "$init_second_out" '^init summary status=ok repositories=[0-9]+$' "init (second)")"
[ "$init_second_summary" = "$init_summary" ] || fail "init summary drifted across repeated run"

verify_second_out="$(run_frigg verify-2 verify --workspace-root "$workspace_root")"
verify_second_summary="$(assert_single_summary_line "$verify_second_out" '^verify summary status=ok repositories=[0-9]+$' "verify (second)")"
[ "$verify_second_summary" = "$verify_summary" ] || fail "verify summary drifted across repeated run"

strict_workspace_root="$tmp_root/workspace-startup-legacy-schema"
strict_db_path="$strict_workspace_root/.frigg/storage.sqlite3"
mkdir -p "$strict_workspace_root/.frigg"
python3 - "$strict_db_path" <<'PY'
import sqlite3
import sys

db_path = sys.argv[1]
conn = sqlite3.connect(db_path)
conn.executescript(
    """
    CREATE TABLE embedding_vectors (
      embedding_id TEXT PRIMARY KEY,
      embedding BLOB NOT NULL,
      dimensions INTEGER NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    );
    """
)
conn.commit()
conn.close()
PY

startup_strict_fail_out="$(run_frigg_expect_fail_fast startup-strict-legacy-schema --workspace-root "$strict_workspace_root")"
startup_strict_summary="$(assert_single_summary_line "$startup_strict_fail_out" '^startup summary status=failed repositories=[0-9]+ repository_id=[^[:space:]]+ root=[^[:space:]]+ db=[^[:space:]]+ error=.*$' "startup strict legacy schema")"
[ "$(extract_field repositories "$startup_strict_summary")" = "1" ] || fail "startup strict legacy schema expected repositories=1"
grep -Fq "legacy non-sqlite-vec schema detected" "$startup_strict_fail_out" || fail "startup strict legacy schema expected non-sqlite-vec rejection message"

echo "smoke-ops passed"
echo "full_mode=$full_mode changed_mode=$changed_mode"
echo "baseline_changed=$changed_zero_changed baseline_deleted=$changed_zero_deleted"
echo "mutated_changed=$changed_mutation_changed mutated_deleted=$changed_mutation_deleted"
echo "startup_strict_legacy_schema=verified"
