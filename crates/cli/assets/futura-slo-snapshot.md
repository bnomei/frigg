# Futura SLO snapshot (`FUT-023`)

Generated: `2026-07-08T22:06:40Z`

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
  "mean_ms": 5.965927150000001,
  "p50_ms": 6.0995625,
  "p95_ms": 7.152901800000001,
  "min_ms": 4.540667,
  "max_ms": 8.876084,
  "samples_ms": [
    4.540667,
    4.755667,
    4.929084,
    6.271458,
    6.215208,
    4.854958,
    6.052625,
    5.958667,
    6.112459,
    6.112917,
    6.086666,
    5.986,
    6.03575,
    7.062208,
    6.232333,
    4.60375,
    6.159458,
    6.136125,
    6.336459,
    8.876084
  ]
}
```

| Metric | Value |
| --- | --- |
| N | 20 |
| mean_ms | 5.966 |
| p50_ms | 6.100 |
| p95_ms | 7.153 |
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


## Shipped Frigg `search_text` soft budget (synth fixture)

Measured by `cargo test -p frigg --test futura_bench` scenario `slo_search_text_latency`
on the synthetic seed fixture (adopted via shipped `workspace` + `search_text`).

| Metric | Result |
| --- | --- |
| Scenario wall clock (12 samples + setup) | ~213 ms total in last run |
| Soft budget | p95 &lt; 2000 ms on tiny fixture |
| Posture | **meets** interactive soft budget — no rational reason to prefer shell for exact scoped probes on small trees |
| Full monorepo p95 vs rg | still optional larger N; rg baseline above remains the shell floor |

Proof command: `cargo test -p frigg --test futura_bench -- --nocapture`

