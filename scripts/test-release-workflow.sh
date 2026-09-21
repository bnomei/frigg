#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKFLOW="$ROOT_DIR/.github/workflows/release.yml"

publishing_checkout_count="$(grep -Fc "ref: \${{ needs.release_ref.outputs.sha }}" "$WORKFLOW")"

if [[ "$publishing_checkout_count" != "4" ]]; then
  echo "Expected build, Linux runtime smoke, container, and npm to checkout the validated release SHA; found $publishing_checkout_count matching checkouts." >&2
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
