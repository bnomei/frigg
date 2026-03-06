#!/usr/bin/env sh
set -eu

overview_file="docs/overview.md"
index_file="specs/index.md"

fail() {
  echo "citation-hygiene check failed: $1" >&2
  exit 1
}

extract_urls() {
  file="$1"
  awk '
    BEGIN { in_fence=0 }
    /^```/ {
      in_fence = !in_fence
      next
    }
    in_fence { next }
    {
      line=$0
      while (match(line, /https?:\/\/[^[:space:]<>"`]+/)) {
        url=substr(line, RSTART, RLENGTH)
        while (url ~ /[),.;:!?]+$/) {
          sub(/[),.;:!?]+$/, "", url)
        }
        print url
        line=substr(line, RSTART + RLENGTH)
      }
    }
  ' "$file"
}

find_placeholders() {
  file="$1"
  awk '
    BEGIN { in_fence=0 }
    /^```/ {
      in_fence = !in_fence
      next
    }
    in_fence { next }
    {
      lower=tolower($0)
      if (lower ~ /(^|[^a-z0-9_])todo([^a-z0-9_]|$)/) {
        print NR "\tTODO\t" $0
      }
      if (lower ~ /(^|[^a-z0-9_])tbd([^a-z0-9_]|$)/) {
        print NR "\tTBD\t" $0
      }
      if (lower ~ /(^|[^a-z0-9_])fixme([^a-z0-9_]|$)/) {
        print NR "\tFIXME\t" $0
      }
      if (index(lower, "citation needed") > 0) {
        print NR "\tcitation needed\t" $0
      }
      if (index(lower, "legacy placeholder") > 0) {
        print NR "\tlegacy placeholder\t" $0
      }
    }
  ' "$file"
}

count_lines() {
  file="$1"
  if [ ! -s "$file" ]; then
    echo "0"
    return
  fi

  wc -l <"$file" | tr -d '[:space:]'
}

for file in "$overview_file" "$index_file"; do
  [ -f "$file" ] || fail "missing required file: $file"
done

sync_date="$(sed -n 's/^Updated:[[:space:]]*\([0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]\).*/\1/p' "$index_file" | head -n 1)"
[ -n "$sync_date" ] || fail "$index_file is missing canonical date line: Updated: YYYY-MM-DD"

registry_header="$(grep -n '^### Fact-check registry' "$overview_file" | head -n 1 || true)"
[ -n "$registry_header" ] || fail "$overview_file is missing heading: ### Fact-check registry (YYYY-MM-DD)"

registry_line="${registry_header%%:*}"
registry_heading="${registry_header#*:}"
registry_date="$(printf '%s\n' "$registry_heading" | sed -n 's/^### Fact-check registry (\([0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]\)).*/\1/p')"
[ -n "$registry_date" ] || fail "$overview_file must use heading format: ### Fact-check registry (YYYY-MM-DD)"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/frigg-citation-hygiene.XXXXXX")"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM HUP

body_section="$tmp_dir/body-section.md"
registry_section="$tmp_dir/registry-section.md"
body_urls="$tmp_dir/body-urls.txt"
registry_urls="$tmp_dir/registry-urls.txt"
missing_urls="$tmp_dir/missing-urls.txt"
placeholders="$tmp_dir/placeholders.txt"

if [ "$registry_line" -gt 1 ]; then
  sed -n "1,$((registry_line - 1))p" "$overview_file" >"$body_section"
else
  : >"$body_section"
fi
sed -n "$((registry_line + 1)),\$p" "$overview_file" >"$registry_section"

extract_urls "$body_section" | LC_ALL=C sort -u >"$body_urls"
extract_urls "$registry_section" | LC_ALL=C sort -u >"$registry_urls"
LC_ALL=C comm -23 "$body_urls" "$registry_urls" >"$missing_urls"
find_placeholders "$overview_file" >"$placeholders"

body_count="$(count_lines "$body_urls")"
registry_count="$(count_lines "$registry_urls")"
missing_count="$(count_lines "$missing_urls")"
placeholders_count="$(count_lines "$placeholders")"

echo "citation_hygiene body=$body_count registry=$registry_count missing=$missing_count placeholders=$placeholders_count"

status=0

if [ "$registry_date" != "$sync_date" ]; then
  echo "citation-hygiene check failed: fact-check registry date '$registry_date' does not match sync date '$sync_date' from $index_file" >&2
  status=1
fi

if [ "$missing_count" -gt 0 ]; then
  echo "citation-hygiene check failed: URLs used before fact-check registry are missing from the registry:" >&2
  sed 's/^/- /' "$missing_urls" >&2
  status=1
fi

if [ "$placeholders_count" -gt 0 ]; then
  echo "citation-hygiene check failed: placeholder markers are not allowed in $overview_file:" >&2
  awk -F '\t' '{ printf("- line %s [%s]: %s\n", $1, $2, $3) }' "$placeholders" >&2
  status=1
fi

if [ "$status" -ne 0 ]; then
  fail "resolve citation hygiene diagnostics above"
fi
