# Search latency SLO snapshot

Generated: `unix-1783552169`

## Posture targets (product contract)

| Surface | Target posture | Gate status |
| --- | --- | --- |
| Small-fixture exact `search_text` p95 | Competitive with local `rg` (≤ 1.5× release noise budget) | **Measured / CI-gated** |
| Warm `search_symbol` p95 | Fast enough that known-name tasks never prefer shell | Posture only (not gated) |
| `search_batch` (4 probes) | Concurrent probes; better agent UX than multi-turn greps | Posture only (not gated) |
| Dirty hot-path index lag | Path-scoped live-disk when dirty | Posture only; lag p95 deferred |
| Hybrid p95 | Allowed slower than exact; must still return pivots promptly | Posture only (not gated) |
| Large-repo monorepo p95 | Competitive with scoped `rg` | **Deferred** (not measured) |

## Methodology (head-to-head)

- **Fixture:** `/var/folders/12/8j3zt8x93jjgg25_tplmk3wm0000gn/T/frigg-futura-bench-synth-slo-vs-rg-39574-1783552168661223000` (materialized synth seed with `src/**/*.rs`, gitignored `*.tmp`)
- **Query:** `greeting` (literal)
- **Frigg path:** shipped `FriggMcpServer::search_text` after `workspace` adopt + 10 warmups; N=50 timed samples; `path_regex='^src/'` only (no glob filter on timed path)
- **rg path:** subprocess `rg -n --glob '*.rs' 'greeting' <fixture>/src`; N=50 timed samples (includes process spawn — agent shell cost)
- **Pass rule (release):** `frigg.p95_ms <= rg.p95_ms * 1.5` (competitive noise budget; exact ≤ and 1.25× flake on small fixtures)
- **Debug:** soft 2s budget only; ratios logged; strict gate skipped

## Measured rg baseline

```json
{
  "max_ms": 5.948875,
  "mean_ms": 4.95392834,
  "min_ms": 3.820084,
  "n": 50,
  "p50_ms": 5.108646,
  "p95_ms": 5.7403231,
  "samples_ms": [
    3.820084,
    3.9542910000000004,
    3.954959,
    3.981292,
    4.016583,
    4.132000000000001,
    4.237,
    4.241709,
    4.259625000000001,
    4.3055,
    4.372375000000001,
    4.453625,
    4.458541,
    4.477959,
    4.493542,
    4.498832999999999,
    4.504958,
    4.668,
    4.772042,
    4.903917,
    4.97875,
    5.048,
    5.09675,
    5.098792,
    5.099167,
    5.118125,
    5.134667,
    5.149708,
    5.159000000000001,
    5.161291,
    5.1879170000000006,
    5.206708,
    5.210249999999999,
    5.23975,
    5.256625,
    5.345292,
    5.356292000000001,
    5.4964580000000005,
    5.499625,
    5.5254579999999995,
    5.530333,
    5.5447500000000005,
    5.571916,
    5.597167,
    5.64275,
    5.732291,
    5.732875,
    5.746417,
    5.7735829999999995,
    5.948875
  ]
}
```

| Metric | Value |
| --- | --- |
| N | 50 |
| mean_ms | 4.954 |
| p50_ms | 5.109 |
| p95_ms | 5.740 |
| query | `greeting` |
| path scope | `src/**/*.rs` |

## Measured warm Frigg `search_text`

```json
{
  "max_ms": 7.58325,
  "mean_ms": 5.263709199999999,
  "min_ms": 3.5768750000000002,
  "n": 50,
  "p50_ms": 5.3962705,
  "p95_ms": 6.3345293499999995,
  "samples_ms": [
    3.5768750000000002,
    3.6759589999999998,
    3.708542,
    3.7368330000000003,
    4.120083,
    4.1441669999999995,
    4.30575,
    4.378125,
    4.445708,
    4.4495000000000005,
    4.4985420000000005,
    4.8851249999999995,
    4.898958,
    4.901875,
    4.907917,
    4.919874999999999,
    4.996874999999999,
    5.081709,
    5.107375,
    5.11975,
    5.15825,
    5.291042,
    5.302083,
    5.368,
    5.392791,
    5.39975,
    5.438542,
    5.445625,
    5.448042,
    5.480874999999999,
    5.509958,
    5.524375,
    5.533417,
    5.537707999999999,
    5.608125,
    5.691125,
    5.713209,
    5.7384580000000005,
    5.813292000000001,
    5.818208,
    5.933333,
    6.038458,
    6.044625,
    6.092375,
    6.129459000000001,
    6.209125,
    6.22375,
    6.425167,
    6.4334999999999996,
    7.58325
  ]
}
```

| Metric | Value |
| --- | --- |
| N | 50 |
| warmup discarded | 10 |
| mean_ms | 5.264 |
| p50_ms | 5.396 |
| p95_ms | 6.335 |
| query | `greeting` |
| path scope | `path_regex=^src/` |

## Comparison

| Tool | p50_ms | p95_ms |
| --- | ---: | ---: |
| local `rg` (subprocess) | 5.109 | 5.740 |
| warm Frigg `search_text` | 5.396 | 6.335 |
| ratio frigg/rg p95 | — | 1.104 |

**Status:** PASS — warm Frigg `search_text` p95 competitive with local `rg` (≤ 1.5× release noise budget) on the same fixture/query/scope

## Operator recipe

```bash
# Binding (release + writes this file when FUTURA_SLO_OUT is set)
FUTURA_SLO_OUT=crates/cli/assets/futura-slo-snapshot.md cargo futura-bench
# Or: scripts/futura_slo_probe.sh crates/cli/assets/futura-slo-snapshot.md

# Contract-only (debug; soft SLO, no competitive gate):
# cargo test -p frigg --test futura_bench -- --nocapture

# Local routing stats — process-local only
FRIGG_ROUTING_STATS=1 frigg serve
# then: frigg stats   OR   MCP resource frigg://stats/routing
```

## Privacy

Routing stats and this SLO snapshot are **local**. No cloud telemetry is required or emitted
by Frigg core for / .
