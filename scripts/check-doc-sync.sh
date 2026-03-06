#!/usr/bin/env sh
set -eu

overview_file="docs/overview.md"
phases_file="docs/phases.md"
index_file="specs/index.md"
citation_script="scripts/check-citation-hygiene.sh"
tool_surface_script="scripts/check-tool-surface-parity.py"

fail() {
  echo "docs-sync check failed: $1" >&2
  exit 1
}

for file in "$overview_file" "$phases_file" "$index_file" "$citation_script" "$tool_surface_script"; do
  [ -f "$file" ] || fail "missing required file: $file"
done

# Shared marker contract:
# - all files must mention "sync note" (case-insensitive)
# - specs/index.md is the canonical sync-date source via "Updated: YYYY-MM-DD"
# - docs/overview.md must include that date literally
# - docs/phases.md must include a sync-note line that also mentions "date"
for file in "$overview_file" "$phases_file" "$index_file"; do
  if ! grep -Eiq "sync note" "$file"; then
    fail "$file is missing shared marker 'sync note' (add a sync note with date)."
  fi
done

sync_date="$(sed -n 's/^Updated:[[:space:]]*\([0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]\).*/\1/p' "$index_file" | head -n 1)"
[ -n "$sync_date" ] || fail "$index_file is missing canonical date line: Updated: YYYY-MM-DD"

if ! grep -Fq "$sync_date" "$overview_file"; then
  fail "$overview_file does not contain sync date '$sync_date' from $index_file."
fi

if ! awk 'BEGIN { ok=0 }
  {
    line=tolower($0)
    if (index(line, "sync note") && index(line, "date")) ok=1
  }
  END { exit ok ? 0 : 1 }' "$phases_file"; then
  fail "$phases_file must include a sync-note line that mentions 'date'."
fi

if ! citation_output="$(bash "$citation_script" 2>&1)"; then
  printf '%s\n' "$citation_output" >&2
  fail "citation hygiene validation failed via $citation_script."
fi

if ! tool_surface_output="$(python3 "$tool_surface_script" 2>&1)"; then
  printf '%s\n' "$tool_surface_output" >&2
  fail "tool surface parity validation failed via $tool_surface_script."
fi

echo "docs-sync check passed"
echo "sync date: $sync_date"
echo "$citation_output"
echo "$tool_surface_output"
echo "checked files:"
echo "- $overview_file"
echo "- $phases_file"
echo "- $index_file"
echo "- $citation_script"
echo "- $tool_surface_script"
