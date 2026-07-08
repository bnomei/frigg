# Futura SLO snapshot (`FUT-023`)

Generated: `unix-1783549684`

## Posture targets (product contract)

| Surface | Target posture |
| --- | --- |
| Small-repo exact `search_text` p95 | **At or better than** local `rg` for equivalent scoped probes |
| Warm `search_symbol` p95 | Fast enough that known-name tasks never prefer shell |
| `search_batch` (4 probes) | Better wall-clock than 4 sequential MCP searches |
| Dirty hot-path index lag | Hot-path reindex prioritizes changed worktree files |
| Hybrid p95 | Allowed slower than exact; must still return pivots promptly |

## Methodology (head-to-head)

- **Fixture:** `/var/folders/12/8j3zt8x93jjgg25_tplmk3wm0000gn/T/frigg-futura-bench-synth-slo-vs-rg-90470-1783549684033684000` (materialized synth seed with `src/**/*.rs`, gitignored `*.tmp`)
- **Query:** `greeting` (literal)
- **Frigg path:** shipped `FriggMcpServer::search_text` after `workspace` adopt + 10 warmups; N=50 timed samples; `path_regex='^src/'`, `glob='**/*.rs'`
- **rg path:** subprocess `rg -n --glob '*.rs' 'greeting' <fixture>/src`; N=50 timed samples (includes process spawn — agent shell cost)
- **Pass rule (release):** `frigg.p95_ms <= rg.p95_ms * 1.25` (competitive; exact ≤ is flaky under process noise)
- **Debug:** soft 2s budget only; ratios logged; strict gate skipped

## Measured rg baseline

```json
{
  "max_ms": 8.579917,
  "mean_ms": 4.94270166,
  "min_ms": 3.7345,
  "n": 50,
  "p50_ms": 5.0078125,
  "p95_ms": 6.1419188999999985,
  "samples_ms": [
    3.7345,
    3.738375,
    3.787125,
    3.813875,
    3.839875,
    3.892125,
    3.9007080000000003,
    3.9160419999999996,
    3.9188749999999994,
    3.934667,
    4.03575,
    4.130542,
    4.183292,
    4.8185,
    4.9393329999999995,
    4.946166,
    4.955458,
    4.964792,
    4.9735,
    4.977291,
    4.984875,
    4.98675,
    4.9895,
    4.995042,
    5.001666,
    5.013959,
    5.022125,
    5.042583,
    5.049125,
    5.063708,
    5.064208,
    5.081542,
    5.093292,
    5.0966249999999995,
    5.099167,
    5.14675,
    5.154332999999999,
    5.15975,
    5.1830419999999995,
    5.191084,
    5.350166,
    5.410042000000001,
    5.489541,
    5.549292,
    5.576541,
    5.624167,
    5.916167,
    6.326625,
    6.4927079999999995,
    8.579917
  ]
}
```

| Metric | Value |
| --- | --- |
| N | 50 |
| mean_ms | 4.943 |
| p50_ms | 5.008 |
| p95_ms | 6.142 |
| query | `greeting` |
| path scope | `src/**/*.rs` |

## Measured warm Frigg `search_text`

```json
{
  "max_ms": 6.279959,
  "mean_ms": 4.59061748,
  "min_ms": 3.451833,
  "n": 50,
  "p50_ms": 4.7308544999999995,
  "p95_ms": 5.856420899999999,
  "samples_ms": [
    3.451833,
    3.467208,
    3.564458,
    3.607625,
    3.657916,
    3.702416,
    3.7287079999999997,
    3.752958,
    3.778458,
    3.791417,
    3.8223749999999996,
    3.823959,
    3.863375,
    3.881,
    3.890625,
    3.8932089999999997,
    4.038542,
    4.059,
    4.147584,
    4.163875,
    4.619416999999999,
    4.632958,
    4.634333,
    4.71,
    4.713709,
    4.747999999999999,
    4.755375,
    4.771415999999999,
    4.800292000000001,
    4.828958,
    4.865084,
    4.9374579999999995,
    4.963959,
    4.96425,
    4.967084,
    5.0018329999999995,
    5.068625,
    5.070333,
    5.079208,
    5.169708,
    5.237458,
    5.249833,
    5.255,
    5.262333,
    5.445041000000001,
    5.57325,
    5.638666,
    6.034584,
    6.166208999999999,
    6.279959
  ]
}
```

| Metric | Value |
| --- | --- |
| N | 50 |
| warmup discarded | 10 |
| mean_ms | 4.591 |
| p50_ms | 4.731 |
| p95_ms | 5.856 |
| query | `greeting` |
| path scope | `path_regex=^src/` + `glob=**/*.rs` |

## Comparison

| Tool | p50_ms | p95_ms |
| --- | ---: | ---: |
| local `rg` (subprocess) | 5.008 | 6.142 |
| warm Frigg `search_text` | 4.731 | 5.856 |
| ratio frigg/rg p95 | — | 0.954 |

**Status:** PASS — warm Frigg `search_text` p95 competitive with local `rg` (≤ 1.25×) on the same fixture/query/scope

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
