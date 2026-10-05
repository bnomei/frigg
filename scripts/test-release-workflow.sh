#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKFLOW="$ROOT_DIR/.github/workflows/release.yml"

publishing_checkout_count="$(grep -Fc "ref: \${{ needs.release_ref.outputs.sha }}" "$WORKFLOW")"

if [[ "$publishing_checkout_count" != "5" ]]; then
  echo "Expected build, cache, Linux runtime smoke, container, and npm to checkout the validated release SHA; found $publishing_checkout_count matching checkouts." >&2
  exit 1
fi

for validation in \
  "git show-ref --verify --quiet" \
  "^{commit}" \
  "git rev-parse HEAD"
do
  grep -Fq "$validation" "$WORKFLOW" || {
    echo "Release workflow is missing tag validation: $validation" >&2
    exit 1
  }
done

for cache_contract in \
  "./dist/frigg cache make" \
  "cache load \"\$GITHUB_WORKSPACE/frigg-cache.zip\"" \
  "path: frigg-cache.zip" \
  "name: portable-cache" \
  "needs: [release_ref, build, cache, linux_runtime_smoke, container, npm]" \
  "fail_on_unmatched_files: true" \
  'FRIGG_SEMANTIC_RUNTIME_ENABLED: "false"'
do
  grep -Fq "$cache_contract" "$WORKFLOW" || {
    echo "Release workflow is missing portable cache contract: $cache_contract" >&2
    exit 1
  }
done

release_publisher_count="$(grep -Fc "uses: softprops/action-gh-release@v3" "$WORKFLOW")"
if [[ "$release_publisher_count" != "1" ]]; then
  echo "Expected exactly one final GitHub Release publisher; found $release_publisher_count." >&2
  exit 1
fi

grep -Fq "uses: taiki-e/install-action@v2" "$WORKFLOW" || {
  echo "Release workflow does not install the cross executable." >&2
  exit 1
}

grep -Fq "tool: cross@0.2.5" "$WORKFLOW" || {
  echo "Release workflow does not pin the cross executable version." >&2
  exit 1
}

if grep -Fq "uses: taiki-e/setup-cross-toolchain-action" "$WORKFLOW"; then
  echo "Release workflow configures a cross toolchain without installing the cross executable." >&2
  exit 1
fi

echo "Release workflow checkout contract is valid."
