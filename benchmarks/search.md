# Search Benchmark Methodology (`v1`)

## Scope

This benchmark plan covers latency tracking for the `searcher` crate text-search paths:

- literal search (`search_literal_with_filters`)
- regex search (`search_regex_with_filters`)
- hybrid search (`search_hybrid_with_filters`) semantic toggle/degraded modes

The harness lives in:

- `crates/cli/benches/search_latency.rs`

Run command:

```bash
cargo bench -p frigg --bench search_latency
```

Canonical budget source:

- `benchmarks/budgets.v1.json`

## Reproducibility Model

The benchmark harness creates deterministic temporary repositories at runtime:

- fixed number of repositories/files/lines
- fixed file names and content patterns
- no random data generation
- stable query set and stable filters

This keeps runs reproducible enough for regression tracking while still exercising filesystem IO and full search traversal.

## Workloads

The harness measures:

1. `literal/global`
- query: `needle`
- filters: none
- purpose: baseline literal throughput over full candidate set

2. `literal/global-low-limit`
- query: `needle`
- filters: none
- limit: `5`
- purpose: low-limit literal path latency and deterministic prefix behavior

3. `literal/global-low-limit-high-cardinality`
- query: `needle_hotspot`
- filters: none
- limit: `5`
- purpose: high-cardinality hotspot term with deterministic low-limit ordering

4. `literal/repo+path+lang`
- query: `needle`
- filters:
  - `repository_id=repo-001`
  - `path_regex=^src/.*\.rs$`
  - `language=rust`
- purpose: filtered literal search path and normalization overhead

5. `regex/repo+path+lang`
- query: `needle\s+\d+`
- same filters as workload 4
- purpose: bounded regex path latency profile

6. `regex/global-sparse-required-literal`
- query: `literal_nohit_0_\d+_9`
- filters: none
- purpose: sparse-hit regex workload with safe required literals for deterministic prefilter acceleration coverage

7. `regex/global-no-hit-required-literal`
- query: `prefilter_absent_token_\d+`
- filters: none
- purpose: no-hit regex workload where safe required literal absence allows deterministic file-level prefilter skips

8. `hybrid/semantic-toggle-off`
- query: `needle_hotspot`
- filters: none
- semantic: `false` (request toggle)
- purpose: deterministic hybrid ranking path when semantic channel is explicitly disabled per-query

9. `hybrid/semantic-degraded-missing-credentials`
- query: `needle_hotspot`
- filters: none
- semantic: `true` (request toggle), runtime semantic mode enabled non-strict
- purpose: deterministic hybrid fallback path where semantic channel degrades due semantic startup-validation failure (`semantic_runtime.model` blank)

## Budget Targets

Targets are strict release-readiness gates once benchmark producers run in `scripts/check-release-readiness.sh`; treat any `fail`/`missing` generator status as a release blocker until budgets or implementation are intentionally updated.

- `literal/global`
  - p50: <= 15 ms
  - p95: <= 40 ms
  - p99: <= 60 ms
- `literal/global-low-limit`
  - p50: <= 10 ms
  - p95: <= 25 ms
  - p99: <= 40 ms
- `literal/global-low-limit-high-cardinality`
  - p50: <= 12 ms
  - p95: <= 30 ms
  - p99: <= 45 ms
- `literal/repo+path+lang`
  - p50: <= 10 ms
  - p95: <= 25 ms
  - p99: <= 40 ms
- `regex/repo+path+lang`
  - p50: <= 20 ms
  - p95: <= 55 ms
  - p99: <= 80 ms
- `regex/global-sparse-required-literal`
  - p50: <= 15 ms
  - p95: <= 35 ms
  - p99: <= 55 ms
- `regex/global-no-hit-required-literal`
  - p50: <= 12 ms
  - p95: <= 30 ms
  - p99: <= 45 ms
- `hybrid/semantic-toggle-off`
  - p50: <= 15 ms
  - p95: <= 35 ms
  - p99: <= 55 ms
- `hybrid/semantic-degraded-missing-credentials`
  - p50: <= 18 ms
  - p95: <= 40 ms
  - p99: <= 60 ms

## Measurement Guidance

- Run benchmarks on an otherwise idle machine when possible.
- Run at least twice and compare trend direction, not single-run noise.
- Use the same build profile and command flags across comparisons.
- If p95/p99 drift upward by >20% versus recent baseline, treat as a regression candidate and investigate.

## Report Generation

Generate deterministic p50/p95/p99 budget reports from Criterion output:

```bash
python3 benchmarks/generate_latency_report.py
```

Output contract:

- stdout key-value lines:
  - `benchmark_report_version=...`
  - per-workload `status`, `p50_ms`, `p95_ms`, `p99_ms`, budget values
  - `summary pass=... fail=... missing=...`
  - `report_path=...`
- markdown file written to:
  - `benchmarks/latest-report.md`
