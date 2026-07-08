# Futura SLO snapshot (`FUT-023`)

Generated: `2026-07-08T21:42:26Z`

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
- **Samples:** N=15 sequential runs (small N; not a full CI p95 gate).
- **Baseline tool:** local `rg -n --glob '*.rs' 'greeting' <fixture>/src`.
- **Honest scope:** this snapshot records the **rg** baseline on the fixture. Full Frigg MCP
  `search_text` p95 needs a warm `frigg serve` process and client loop (Phase 7 bench).
  Posture rule until then: *warm Frigg exact search must not lose to scoped rg on small fixtures*.
- **Ignore truth:** fixture includes gitignored `src/ignored.tmp` (absent from indexed search).

## Measured rg baseline (fixture)

```json
{
  "n": 15,
  "mean_ms": 5.8624138000000015,
  "p50_ms": 6.037958,
  "p95_ms": 6.680978699999999,
  "min_ms": 4.708959,
  "max_ms": 7.45025,
  "samples_ms": [
    4.890583,
    6.198334,
    6.213416,
    5.148708,
    6.06825,
    6.075917,
    5.960791,
    6.351291,
    7.45025,
    5.5425,
    6.052334,
    4.708959,
    5.958875,
    6.037958,
    5.278041
  ]
}
```

| Metric | Value |
| --- | --- |
| N | 15 |
| mean_ms | 5.862 |
| p50_ms | 6.038 |
| p95_ms | 6.681 |
| query | `greeting` |
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
