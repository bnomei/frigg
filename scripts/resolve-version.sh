#!/usr/bin/env bash
set -euo pipefail

PACKAGE_NAME="${PACKAGE_NAME:-frigg}"

if [[ "${RELEASE_TAG:-}" == v* ]]; then
  version="${RELEASE_TAG#v}"
elif [[ "${GITHUB_REF_NAME:-}" == v* ]]; then
  version="${GITHUB_REF_NAME#v}"
else
  version=$(python3 - <<'PY'
import json
import os
import subprocess

package_name = os.environ.get("PACKAGE_NAME", "frigg")
meta = json.loads(
    subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"])
)
for package in meta["packages"]:
    if package["name"] == package_name:
        print(package["version"])
        break
else:
    raise SystemExit(f"package not found in cargo metadata: {package_name}")
PY
  )
fi

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  echo "version=${version}" >> "$GITHUB_OUTPUT"
else
  echo "$version"
fi
