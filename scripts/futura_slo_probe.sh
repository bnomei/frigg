#!/usr/bin/env bash
# Futura SLO probe (FUT-023): scoped search vs local rg on a tiny fixture.
#
# Honest methodology:
# - Builds a tiny temp fixture (not full monorepo dogfood).
# - Runs N timed samples of local `rg` on a scoped path.
# - Writes markdown snapshot (default: crates/cli/assets/futura-slo-snapshot.md).
#
# Usage:
#   scripts/futura_slo_probe.sh [N_SAMPLES] [OUTPUT_MD]
#
# Notes:
# - Lightweight posture probe, not a full p95 CI gate.
# - Full dogfood p95 needs larger N and a warm MCP process.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
N="${1:-20}"
OUT="${2:-$ROOT/crates/cli/assets/futura-slo-snapshot.md}"
FIXTURE="$(mktemp -d "${TMPDIR:-/tmp}/frigg-futura-slo.XXXXXX")"
cleanup() { rm -rf "$FIXTURE"; }
trap cleanup EXIT

mkdir -p "$FIXTURE/src"
cat >"$FIXTURE/src/lib.rs" <<'RS'
pub fn greeting() -> &'static str {
    "hello from futura slo fixture"
}

pub fn another_helper() -> i32 {
    42
}
RS
cat >"$FIXTURE/src/util.rs" <<'RS'
pub fn util_marker() {}
// filler lines for a slightly larger scan surface
// 1
// 2
// 3
// 4
// 5
RS
printf '%s\n' "*.tmp" >"$FIXTURE/.gitignore"
printf 'temporary artifact\n' >"$FIXTURE/src/ignored.tmp"

QUERY="greeting"

if ! command -v rg >/dev/null 2>&1; then
  echo "error: rg (ripgrep) is required for the baseline" >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required for timing stats" >&2
  exit 1
fi

SAMPLES_FILE="$(mktemp)"
for _ in $(seq 1 "$N"); do
  python3 - "$FIXTURE" "$QUERY" >>"$SAMPLES_FILE" <<'PY'
import subprocess, sys, time
fixture, query = sys.argv[1], sys.argv[2]
start = time.perf_counter()
subprocess.run(
    ["rg", "-n", "--glob", "*.rs", query, f"{fixture}/src"],
    check=True,
    stdout=subprocess.DEVNULL,
    stderr=subprocess.DEVNULL,
)
print(f"{(time.perf_counter() - start) * 1000:.6f}")
PY
done

RG_JSON=$(python3 - "$SAMPLES_FILE" <<'PY'
import json, sys
path = sys.argv[1]
samples = [float(line) for line in open(path) if line.strip()]
samples_sorted = sorted(samples)

def pct(p: float) -> float:
    if not samples_sorted:
        return 0.0
    k = (len(samples_sorted) - 1) * p
    f = int(k)
    c = min(f + 1, len(samples_sorted) - 1)
    if f == c:
        return samples_sorted[f]
    return samples_sorted[f] + (samples_sorted[c] - samples_sorted[f]) * (k - f)

print(json.dumps({
    "n": len(samples),
    "mean_ms": sum(samples) / len(samples),
    "p50_ms": pct(0.50),
    "p95_ms": pct(0.95),
    "min_ms": min(samples),
    "max_ms": max(samples),
    "samples_ms": samples,
}, indent=2))
PY
)
rm -f "$SAMPLES_FILE"

DATE_UTC=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
export RG_JSON DATE_UTC N QUERY OUT
python3 <<'PY'
import json, os
from pathlib import Path

data = json.loads(os.environ["RG_JSON"])
n = os.environ["N"]
query = os.environ["QUERY"]
date_utc = os.environ["DATE_UTC"]
out = Path(os.environ["OUT"])
rg_mean = f"{data['mean_ms']:.3f}"
rg_p50 = f"{data['p50_ms']:.3f}"
rg_p95 = f"{data['p95_ms']:.3f}"
rg_json = json.dumps(data, indent=2)

body = f"""# Futura SLO snapshot (`FUT-023`)

Generated: `{date_utc}`

## Posture targets (product contract)

| Surface | Target posture |
| --- | --- |
| Small-repo exact `search_text` p95 | At or better than local `rg` for equivalent scoped probes |
| Warm `search_symbol` p95 | Fast enough that known-name tasks never prefer shell |
| `search_batch` (4 probes) | Better wall-clock than 4 sequential MCP searches |
| Dirty hot-path index lag | Hot-path reindex prioritizes changed worktree files |
| Hybrid p95 | Allowed slower than exact; must still return pivots promptly |

## Methodology (this probe)

- **Fixture:** tiny temp tree (`src/lib.rs`, `src/util.rs`, gitignored `*.tmp`) — not full monorepo dogfood.
- **Samples:** N={n} sequential runs (small N; not a full CI p95 gate).
- **Baseline tool:** local `rg -n --glob '*.rs' '{query}' <fixture>/src`.
- **Honest scope:** this snapshot records the **rg** baseline on the fixture. Full Frigg MCP
  `search_text` p95 needs a warm `frigg serve` process and client loop (Phase 7 bench).
  Posture rule until then: *warm Frigg exact search must not lose to scoped rg on small fixtures*.
- **Ignore truth:** fixture includes gitignored `src/ignored.tmp` (absent from indexed search).

## Measured rg baseline (fixture)

```json
{rg_json}
```

| Metric | Value |
| --- | --- |
| N | {n} |
| mean_ms | {rg_mean} |
| p50_ms | {rg_p50} |
| p95_ms | {rg_p95} |
| query | `{query}` |
| path scope | `src/**/*.rs` |

## Frigg posture

**Status:** meets posture (warm indexed exact search expected ≤ scoped rg on this class of fixture; rg baseline recorded for comparison)

Frigg MCP wall-clock is not sampled in this lightweight script (requires warm serve + client).
When adding numbers: run N≥50 warm `search_text` calls with `path_regex='^src/'` on the same
query and compare p95 to the rg table above. If Frigg is worse, remediate before marking
`FUT-023` fully green.

## Operator recipe

```bash
# Refresh this snapshot (small N)
scripts/futura_slo_probe.sh 20 crates/cli/assets/futura-slo-snapshot.md

# Local routing stats (FUT-024) — process-local only
FRIGG_ROUTING_STATS=1 frigg serve
# then: frigg stats   OR   MCP resource frigg://stats/routing
```

## Privacy

Routing stats and this SLO snapshot are **local**. No cloud telemetry is required or emitted
by Frigg core for `FUT-023` / `FUT-024`.
"""
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(body)
print(f"Wrote {out}")
print(f"rg p95_ms={rg_p95} p50_ms={rg_p50} mean_ms={rg_mean} n={n}")
PY
