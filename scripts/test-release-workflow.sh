#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKFLOW="$ROOT_DIR/.github/workflows/release.yml"

publishing_checkout_count="$(grep -Fc "ref: \${{ needs.release_ref.outputs.sha }}" "$WORKFLOW")"

if [[ "$publishing_checkout_count" != "3" ]]; then
  echo "Expected build, container, and npm to checkout the validated release SHA; found $publishing_checkout_count matching checkouts." >&2
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

echo "Release workflow checkout contract is valid."
