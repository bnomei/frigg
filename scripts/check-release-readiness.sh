#!/usr/bin/env sh
set -eu

fail() {
  echo "release-readiness check failed: $1" >&2
  exit 1
}

require_command() {
  command_name="$1"
  command -v "$command_name" >/dev/null 2>&1 || fail "required command is unavailable: $command_name"
}

require_file() {
  file="$1"
  [ -f "$file" ] || fail "missing required file: $file"
}

require_executable() {
  file="$1"
  [ -x "$file" ] || fail "required executable is missing or not executable: $file"
}

require_pattern() {
  pattern="$1"
  file="$2"
  message="$3"
  if ! grep -Eq -- "$pattern" "$file"; then
    fail "$message"
  fi
}

extract_summary_count() {
  key="$1"
  line="$2"
  value="$(echo "$line" | sed -n "s/.*$key=\\([0-9][0-9]*\\).*/\\1/p")"
  [ -n "$value" ] || fail "unable to parse $key from benchmark summary line: $line"
  echo "$value"
}

extract_report_workload_budget_signature() {
  report_file="$1"
  awk -F'|' '
    /^\| `[^`]+` \|/ {
      workload=$2
      budget=$7
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", workload)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", budget)
      print workload "|" budget
    }
  ' "$report_file"
}

run_required_check() {
  label="$1"
  shift

  output_file="$run_dir/$label.out"
  if ! "$@" >"$output_file" 2>&1; then
    cat "$output_file" >&2 || true
    fail "required check failed: $label"
  fi
  echo "$output_file"
}

forbidden_toggle="${FRIGG_RELEASE_READINESS_FORCE_FAIL:-0}"
if [ "$forbidden_toggle" = "1" ]; then
  fail "forced failure via FRIGG_RELEASE_READINESS_FORCE_FAIL=1"
fi

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$repo_root"

run_dir="$(mktemp -d "${TMPDIR:-/tmp}/frigg-release-readiness.XXXXXX")"
cleanup() {
  rm -rf "$run_dir"
}
trap cleanup EXIT INT TERM HUP

threat_model="docs/security/threat-model.md"
readiness_doc="docs/security/release-readiness.md"
errors_contract="contracts/errors.md"
tools_contract_readme="contracts/tools/v1/README.md"
contract_changelog="contracts/changelog.md"
bench_readme="benchmarks/README.md"
bench_report="benchmarks/latest-report.md"
bench_budget="benchmarks/budgets.v1.json"
bench_generator="benchmarks/generate_latency_report.py"
bench_search="benchmarks/search.md"
bench_mcp="benchmarks/mcp-tools.md"
smoke_script="scripts/smoke-ops.sh"
gate_script="scripts/check-release-readiness.sh"
citation_script="scripts/check-citation-hygiene.sh"
tool_surface_script="scripts/check-tool-surface-parity.py"

for file in "$threat_model" "$readiness_doc" "$errors_contract" "$tools_contract_readme" "$contract_changelog" "$bench_readme" "$bench_report" "$bench_budget" "$bench_generator" "$bench_search" "$bench_mcp" "$smoke_script" "$gate_script" "$citation_script" "$tool_surface_script"; do
  require_file "$file"
done
require_executable "$smoke_script"
require_command cargo
require_command python3
require_command cmp

require_pattern '^readiness_version:[[:space:]]*v1$' "$readiness_doc" "release readiness doc is missing readiness_version: v1"
require_pattern '^gate_security:[[:space:]]*pass$' "$readiness_doc" "release readiness doc must set gate_security: pass"
require_pattern '^gate_performance:[[:space:]]*pass$' "$readiness_doc" "release readiness doc must set gate_performance: pass"
require_pattern '^gate_operability:[[:space:]]*pass$' "$readiness_doc" "release readiness doc must set gate_operability: pass"
require_pattern '^last_verified:[[:space:]]*[0-9]{4}-[0-9]{2}-[0-9]{2}$' "$readiness_doc" "release readiness doc must include last_verified: YYYY-MM-DD"
readiness_last_verified="$(sed -n 's/^last_verified:[[:space:]]*\([0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]\)$/\1/p' "$readiness_doc" | head -n 1)"
[ -n "$readiness_last_verified" ] || fail "release readiness doc last_verified date could not be parsed"

for checklist_id in \
  RR-SEC-001 \
  RR-SEC-002 \
  RR-SEC-003 \
  RR-SEC-004 \
  RR-PERF-001 \
  RR-PERF-002 \
  RR-PERF-003 \
  RR-OPS-001 \
  RR-OPS-002 \
  RR-GATE-001 \
  RR-GATE-002 \
  RR-GATE-003
do
  require_pattern "^- \\[x\\] $checklist_id " "$readiness_doc" "release readiness checklist item is unchecked or missing: $checklist_id"
done

require_pattern 'bash scripts/check-release-readiness.sh' "$readiness_doc" "release readiness doc must include gate pass command example"
require_pattern 'bash scripts/check-citation-hygiene.sh' "$readiness_doc" "release readiness doc must include citation hygiene command example"
require_pattern 'python3 scripts/check-tool-surface-parity.py' "$readiness_doc" "release readiness doc must include tool-surface parity command example"
require_pattern 'FRIGG_RELEASE_READINESS_FORCE_FAIL=1 bash scripts/check-release-readiness.sh' "$readiness_doc" "release readiness doc must include controlled fail command example"

require_pattern '[Pp]ath traversal' "$threat_model" "threat model must mention path traversal threat coverage"
require_pattern '[Ww]orkspace-boundary|[Ww]orkspace roots|workspace-boundary enforcement' "$threat_model" "threat model must mention workspace boundary enforcement"
require_pattern 'Regex abuse|regex abuse' "$threat_model" "threat model must mention regex abuse coverage"
require_pattern 'cargo test -p frigg --test security' "$threat_model" "threat model must include mcp security regression command"
require_pattern 'cargo test -p frigg security' "$threat_model" "threat model must include searcher security regression command"

require_pattern 'write_surface_policy:[[:space:]]*v1' "$errors_contract" "errors contract must declare write_surface_policy: v1 marker"
require_pattern 'write_confirm_param:[[:space:]]*confirm' "$errors_contract" "errors contract must declare write_confirm_param: confirm marker"
require_pattern 'write_confirm_required:[[:space:]]*true' "$errors_contract" "errors contract must declare write_confirm_required: true marker"
require_pattern 'write_confirm_failure_error_code:[[:space:]]*confirmation_required' "$errors_contract" "errors contract must declare confirmation_required marker"
require_pattern 'write_no_side_effect_without_confirm:[[:space:]]*true' "$errors_contract" "errors contract must declare no-side-effect-before-confirm marker"

require_pattern 'write_surface_policy:[[:space:]]*v1' "$tools_contract_readme" "tools contract must declare write_surface_policy: v1 marker"
require_pattern 'current_public_tool_surface:[[:space:]]*read_only' "$tools_contract_readme" "tools contract must declare current_public_tool_surface: read_only marker"
require_pattern 'write_confirm_param:[[:space:]]*confirm' "$tools_contract_readme" "tools contract must declare write_confirm_param: confirm marker"
require_pattern 'write_confirm_semantics:[[:space:]]*reject_missing_or_false_confirm_before_side_effects' "$tools_contract_readme" "tools contract must declare deterministic confirm semantics marker"
require_pattern 'write_confirm_failure_error_code:[[:space:]]*confirmation_required' "$tools_contract_readme" "tools contract must declare confirmation_required marker"
require_pattern 'write_safety_invariant_workspace_boundary:[[:space:]]*required' "$tools_contract_readme" "tools contract must require workspace boundary invariant"
require_pattern 'write_safety_invariant_path_traversal_defense:[[:space:]]*required' "$tools_contract_readme" "tools contract must require path traversal invariant"
require_pattern 'write_safety_invariant_regex_budget_limits:[[:space:]]*required' "$tools_contract_readme" "tools contract must require regex safety invariant"
require_pattern 'write_safety_invariant_typed_deterministic_errors:[[:space:]]*required' "$tools_contract_readme" "tools contract must require typed deterministic errors invariant"

require_pattern 'write_surface_policy:[[:space:]]*v1' "$threat_model" "threat model must declare write_surface_policy: v1 marker"
require_pattern 'write_confirm_param:[[:space:]]*confirm' "$threat_model" "threat model must declare write_confirm_param: confirm marker"
require_pattern 'write_confirm_required:[[:space:]]*true' "$threat_model" "threat model must declare write_confirm_required: true marker"
require_pattern 'write_confirm_failure_error_code:[[:space:]]*confirmation_required' "$threat_model" "threat model must declare confirmation_required marker"
require_pattern 'write_no_side_effect_without_confirm:[[:space:]]*true' "$threat_model" "threat model must declare no-side-effect-before-confirm marker"
require_pattern 'write_safety_invariant_workspace_boundary:[[:space:]]*required' "$threat_model" "threat model must declare workspace boundary invariant marker"
require_pattern 'write_safety_invariant_path_traversal_defense:[[:space:]]*required' "$threat_model" "threat model must declare path traversal invariant marker"
require_pattern 'write_safety_invariant_regex_budget_limits:[[:space:]]*required' "$threat_model" "threat model must declare regex safety invariant marker"
require_pattern 'write_safety_invariant_typed_deterministic_errors:[[:space:]]*required' "$threat_model" "threat model must declare typed deterministic errors invariant marker"

changelog_top_date="$(sed -n 's/^##[[:space:]]*\([0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]\)$/\1/p' "$contract_changelog" | head -n 1)"
[ -n "$changelog_top_date" ] || fail "contracts changelog is missing a top YYYY-MM-DD heading"
[ "$changelog_top_date" = "$readiness_last_verified" ] || fail "contracts changelog top date ($changelog_top_date) must match release readiness last_verified ($readiness_last_verified)"

changelog_top_section="$run_dir/changelog-top-section.md"
awk '
  BEGIN { in_section = 0; seen_section = 0; }
  /^## [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$/ {
    if (seen_section == 1) {
      exit
    }
    in_section = 1
    seen_section = 1
    next
  }
  in_section == 1 {
    print
  }
' "$contract_changelog" >"$changelog_top_section"
require_pattern '^- spec:[[:space:]]*`[^`]+`$' "$changelog_top_section" "contracts changelog top section must include at least one spec entry"
require_pattern '^- change_set:[[:space:]]*`[^`]+`$' "$changelog_top_section" "contracts changelog top section must include at least one change_set entry"
require_pattern '^- summary:[[:space:]].+$' "$changelog_top_section" "contracts changelog top section must include at least one summary entry"

require_pattern '`budgets.v1.json` is the canonical machine-readable budget contract' "$bench_readme" "benchmarks README must declare canonical budget contract"
require_pattern 'summary pass=<int> fail=<int> missing=<int>' "$bench_readme" "benchmarks README must document release summary line shape"
require_pattern 'workload/budget parity' "$bench_readme" "benchmarks README must require committed report workload/budget parity with fresh generated output"
require_pattern 'p50' "$bench_search" "search benchmark doc must include p50 budget guidance"
require_pattern 'p95' "$bench_search" "search benchmark doc must include p95 budget guidance"
require_pattern 'p99' "$bench_search" "search benchmark doc must include p99 budget guidance"
require_pattern 'p50' "$bench_mcp" "mcp benchmark doc must include p50 budget guidance"
require_pattern 'p95' "$bench_mcp" "mcp benchmark doc must include p95 budget guidance"
require_pattern 'p99' "$bench_mcp" "mcp benchmark doc must include p99 budget guidance"

run_required_check "security-mcp" env CARGO_TERM_COLOR=never cargo test -p frigg --test security >/dev/null
run_required_check "security-searcher" env CARGO_TERM_COLOR=never cargo test -p frigg security >/dev/null
run_required_check "operability-smoke" "$smoke_script" >/dev/null
citation_output_file="$(run_required_check "citation-hygiene" bash "$citation_script")"
tool_surface_output_file="$(run_required_check "tool-surface-parity" python3 "$tool_surface_script")"

run_required_check "bench-search" env CARGO_TERM_COLOR=never cargo bench -p frigg --bench search_latency -- --noplot >/dev/null
run_required_check "bench-mcp" env CARGO_TERM_COLOR=never cargo bench -p frigg --bench tool_latency -- --noplot >/dev/null
run_required_check "bench-graph" env CARGO_TERM_COLOR=never cargo bench -p frigg --bench graph_hot_paths -- --noplot >/dev/null
run_required_check "bench-storage" env CARGO_TERM_COLOR=never cargo bench -p frigg --bench storage_hot_paths -- --noplot >/dev/null
run_required_check "bench-index" env CARGO_TERM_COLOR=never cargo bench -p frigg --bench reindex_latency -- --noplot >/dev/null

citation_summary_line="$(grep -E '^citation_hygiene body=[0-9]+ registry=[0-9]+ missing=[0-9]+ placeholders=[0-9]+$' "$citation_output_file" | tail -n 1 || true)"
[ -n "$citation_summary_line" ] || fail "citation hygiene check output is missing deterministic summary line"
tool_surface_summary_line="$(grep -E '^tool-surface-parity summary status=pass active_profile=[a-z_]+ profiles=core,extended$' "$tool_surface_output_file" | tail -n 1 || true)"
[ -n "$tool_surface_summary_line" ] || fail "tool-surface parity output is missing deterministic summary line"

fresh_bench_report="$run_dir/latest-report.fresh.md"
bench_output_file="$(run_required_check "benchmark-report" python3 "$bench_generator" --fail-on-budget --output "$fresh_bench_report")"
require_file "$fresh_bench_report"

summary_line="$(grep -E '^summary pass=[0-9]+ fail=[0-9]+ missing=[0-9]+$' "$bench_output_file" | tail -n 1 || true)"
[ -n "$summary_line" ] || fail "benchmark generator output is missing deterministic summary line"

pass_count="$(extract_summary_count "pass" "$summary_line")"
fail_count="$(extract_summary_count "fail" "$summary_line")"
missing_count="$(extract_summary_count "missing" "$summary_line")"

[ "$pass_count" -ge 1 ] || fail "benchmark latest report must show pass>=1"
[ "$fail_count" -eq 0 ] || fail "benchmark latest report must show fail=0"
[ "$missing_count" -eq 0 ] || fail "benchmark latest report must show missing=0"

fresh_signature="$run_dir/latest-report.fresh.signature"
committed_signature="$run_dir/latest-report.committed.signature"
extract_report_workload_budget_signature "$fresh_bench_report" >"$fresh_signature"
extract_report_workload_budget_signature "$bench_report" >"$committed_signature"
if ! cmp -s "$fresh_signature" "$committed_signature"; then
  fail "benchmark latest report workload/budget signature drifted; refresh benchmarks/latest-report.md from current budgets/workloads"
fi

echo "release-readiness check passed"
echo "readiness_version=v1"
echo "write_surface_policy=v1"
echo "$citation_summary_line"
echo "$tool_surface_summary_line"
echo "benchmark_summary pass=$pass_count fail=$fail_count missing=$missing_count"
echo "executed_checks:"
echo "- cargo test -p frigg --test security"
echo "- cargo test -p frigg security"
echo "- scripts/smoke-ops.sh"
echo "- bash scripts/check-citation-hygiene.sh"
echo "- python3 scripts/check-tool-surface-parity.py"
echo "- cargo bench -p frigg --bench search_latency -- --noplot"
echo "- cargo bench -p frigg --bench tool_latency -- --noplot"
echo "- cargo bench -p frigg --bench graph_hot_paths -- --noplot"
echo "- cargo bench -p frigg --bench storage_hot_paths -- --noplot"
echo "- cargo bench -p frigg --bench reindex_latency -- --noplot"
echo "- python3 benchmarks/generate_latency_report.py --fail-on-budget --output <tmp>"
echo "checked artifacts:"
echo "- $threat_model"
echo "- $readiness_doc"
echo "- $errors_contract"
echo "- $tools_contract_readme"
echo "- $contract_changelog"
echo "- $bench_budget"
echo "- $bench_report"
echo "- $bench_generator"
echo "- $smoke_script"
echo "- $tool_surface_script"
