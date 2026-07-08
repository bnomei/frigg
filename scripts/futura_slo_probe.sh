#!/usr/bin/env bash
# Futura SLO probe (FUT-023): head-to-head warm Frigg search_text vs local rg.
#
# Runs the shipped futura_bench scenario `slo_search_text_vs_rg` which:
# - materializes a tiny fixture
# - times N warm FriggMcpServer::search_text samples (after warmup)
# - times N subprocess `rg` samples on the same query/path
# - fails if frigg.p95_ms > rg.p95_ms
# - writes markdown when FUTURA_SLO_OUT is set
#
# Usage:
#   scripts/futura_slo_probe.sh [OUTPUT_MD]
#   scripts/futura_slo_probe.sh crates/cli/assets/futura-slo-snapshot.md

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$ROOT/crates/cli/assets/futura-slo-snapshot.md}"

cd "$ROOT"
export FUTURA_SLO_OUT="$OUT"

echo "Running head-to-head FUT-023 probe (release futura_bench)..."
echo "FUTURA_SLO_OUT=$FUTURA_SLO_OUT"

cargo test -p frigg --test futura_bench --release -- --nocapture

echo "Wrote/updated: $OUT"
