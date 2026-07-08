# Futura SLO snapshot (`FUT-023`)

Generated: `2026-07-08T22:22:05Z`

## Posture targets (product contract)

| Surface | Target posture |
| --- | --- |
| Small-repo exact `search_text` p95 | **At or better than** local `rg` for equivalent scoped probes |
| Warm `search_symbol` p95 | Fast enough that known-name tasks never prefer shell |
| `search_batch` (4 probes) | Better wall-clock than 4 sequential MCP searches |
| Dirty hot-path index lag | Hot-path reindex prioritizes changed worktree files |
| Hybrid p95 | Allowed slower than exact; must still return pivots promptly |

## Methodology (head-to-head)

- **Fixture:** `/var/folders/12/8j3zt8x93jjgg25_tplmk3wm0000gn/T/frigg-futura-bench-synth-slo-vs-rg-36185-1783549311441566000` (materialized synth seed with `src/**/*.rs`, gitignored `*.tmp`)
- **Query:** `greeting` (literal)
- **Frigg path:** shipped `FriggMcpServer::search_text` after `workspace` adopt + 5 warmups; N=30 timed samples; `path_regex='^src/'` (equivalent scoped probe to `rg` on `src/`)
- **rg path:** subprocess `rg -n --glob '*.rs' 'greeting' <fixture>/src`; N=30 timed samples (includes process spawn — agent shell cost)
- **Pass rule:** `frigg.p95_ms <= rg.p95_ms` (strict agent-facing: warm Frigg must not lose to shell rg)

## Measured rg baseline

```json
{
  "max_ms": 6.192792,
  "mean_ms": 4.936005699999999,
  "min_ms": 3.736125,
  "n": 30,
  "p50_ms": 5.0255215,
  "p95_ms": 5.991970799999998,
  "samples_ms": [
    3.736125,
    3.768375,
    3.833709,
    3.857708,
    3.868625,
    3.9969589999999995,
    4.02325,
    4.367625,
    4.600957999999999,
    4.676208,
    4.706417,
    4.9655000000000005,
    4.973375000000001,
    5.018,
    5.020959,
    5.0300839999999996,
    5.032209,
    5.0563329999999995,
    5.070749999999999,
    5.100625,
    5.309125,
    5.598,
    5.610959,
    5.64875,
    5.649459,
    5.703042,
    5.71875,
    5.780542,
    6.1649579999999995,
    6.192792
  ]
}
```

| Metric | Value |
| --- | --- |
| N | 30 |
| mean_ms | 4.936 |
| p50_ms | 5.026 |
| p95_ms | 5.992 |
| query | `greeting` |
| path scope | `src/**/*.rs` |

## Measured warm Frigg `search_text`

```json
{
  "max_ms": 5.884582999999999,
  "mean_ms": 4.865165233333332,
  "min_ms": 3.8352500000000003,
  "n": 30,
  "p50_ms": 5.1545415000000006,
  "p95_ms": 5.8697916999999995,
  "samples_ms": [
    3.8352500000000003,
    3.9147499999999997,
    3.997375,
    4.003208,
    4.012458,
    4.047167,
    4.089542,
    4.130417,
    4.166417,
    4.332,
    4.334958,
    4.473833,
    4.7169170000000005,
    5.081958,
    5.088958,
    5.220125,
    5.223666,
    5.253417,
    5.276083,
    5.299167,
    5.316999999999999,
    5.337042,
    5.345125,
    5.407291,
    5.441584,
    5.483041,
    5.504375,
    5.856958,
    5.880292,
    5.884582999999999
  ]
}
```

| Metric | Value |
| --- | --- |
| N | 30 |
| warmup discarded | 5 |
| mean_ms | 4.865 |
| p50_ms | 5.155 |
| p95_ms | 5.870 |
| query | `greeting` |
| path scope | `path_regex=^src/` + `glob=**/*.rs` |

## Comparison

| Tool | p50_ms | p95_ms |
| --- | ---: | ---: |
| local `rg` (subprocess) | 5.026 | 5.992 |
| warm Frigg `search_text` | 5.155 | 5.870 |
| ratio frigg/rg p95 | — | 0.980 |

**Status:** PASS — warm Frigg `search_text` p95 ≤ local `rg` p95 on the same fixture/query/scope

## Operator recipe

```bash
# Head-to-head (writes this file when FUTURA_SLO_OUT is set)
FUTURA_SLO_OUT=crates/cli/assets/futura-slo-snapshot.md \
  cargo test -p frigg --test futura_bench -- --nocapture

# Or: scripts/futura_slo_probe.sh 30 crates/cli/assets/futura-slo-snapshot.md

# Local routing stats (FUT-024) — process-local only
FRIGG_ROUTING_STATS=1 frigg serve
# then: frigg stats   OR   MCP resource frigg://stats/routing
```

## Privacy

Routing stats and this SLO snapshot are **local**. No cloud telemetry is required or emitted
by Frigg core for `FUT-023` / `FUT-024`.
