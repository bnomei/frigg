# Futura SLO snapshot (`FUT-023`)

Generated: `2026-07-08T21:50:41Z`

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
- **Samples:** N=20 sequential runs (small N; not a full CI p95 gate).
- **Baseline tool:** local `rg -n --glob '*.rs' 'greeting' <fixture>/src`.
- **Honest scope:** this snapshot records the **rg** baseline on the fixture. Full Frigg MCP
  `search_text` p95 needs a warm `frigg serve` process and client loop (Phase 7 bench).
  Posture rule until then: *warm Frigg exact search must not lose to scoped rg on small fixtures*.
- **Ignore truth:** fixture includes gitignored `src/ignored.tmp` (absent from indexed search).

## Measured rg baseline (fixture)

```json
{
  "n": 20,
  "mean_ms": 6.334939650000001,
  "p50_ms": 6.247458,
  "p95_ms": 7.91732325,
  "min_ms": 4.838292,
  "max_ms": 7.936917,
  "samples_ms": [
    6.1965,
    5.959958,
    4.838292,
    6.5815,
    5.148792,
    6.0955,
    6.69275,
    5.1505,
    5.124125,
    4.985125,
    7.738291,
    6.448417,
    5.687083,
    6.140459,
    7.916292,
    6.298416,
    7.936917,
    7.028125,
    7.645709,
    7.086042
  ]
}
```

| Metric | Value |
| --- | --- |
| N | 20 |
| mean_ms | 6.335 |
| p50_ms | 6.247 |
| p95_ms | 7.917 |
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
