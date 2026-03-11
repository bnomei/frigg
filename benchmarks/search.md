# Search Benchmark Methodology (`v1`)

## Scope

This benchmark plan covers latency tracking for the `searcher` crate text-search paths:

- literal search (`search_literal_with_filters`)
- manifest-backed literal search over indexed snapshots
- regex search (`search_regex_with_filters`)
- hybrid search (`search_hybrid_with_filters`) graph-backed, manifest-backed witness-recall, and semantic control-path modes (`disabled`, `degraded`)

Healthy sqlite-vec semantic retrieval is benchmarked separately in [`storage.md`](./storage.md) as a local vector top-k plus batched payload hot path. `search_latency` keeps the higher-level hybrid control workloads so release artifacts distinguish semantic runtime state handling from raw vector retrieval cost.
When the optional stage-attribution snapshot is present, the release report breaks the hybrid ranking tail into `anchor_blending`, `document_aggregation`, and `final_diversification`. `document_aggregation` preserves the winning anchor shown to clients while allowing corroborating anchors from the same document to strengthen its score before the single final diversification pass.

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
- fixed walk-backed roots for unindexed baseline coverage
- fixed manifest-backed indexed roots for candidate-universe reuse and witness-projection coverage
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

4. `literal/indexed-manifest-low-limit-high-cardinality`
- query: `needle_hotspot`
- filters: none
- limit: `5`
- fixture: manifest-backed indexed benchmark roots
- purpose: high-cardinality hotspot term through manifest-backed candidate discovery and deterministic low-limit ordering

5. `literal/repo+path+lang`
- query: `needle`
- filters:
  - `repository_id=repo-001`
  - `path_regex=^src/.*\.rs$`
  - `language=rust`
- purpose: filtered literal search path and normalization overhead

6. `regex/repo+path+lang`
- query: `needle\s+\d+`
- same filters as workload 5
- purpose: bounded regex path latency profile

7. `regex/global-sparse-required-literal`
- query: `literal_nohit_0_\d+_9`
- filters: none
- purpose: sparse-hit regex workload with safe required literals for deterministic prefilter acceleration coverage

8. `regex/global-no-hit-required-literal`
- query: `prefilter_absent_token_\d+`
- filters: none
- purpose: no-hit regex workload where safe required literal absence allows deterministic file-level prefilter skips

9. `hybrid/semantic-toggle-off`
- query: `needle_hotspot`
- filters: none
- semantic: `false` (request toggle)
- purpose: deterministic hybrid ranking path when semantic channel is explicitly disabled per-query

10. `hybrid/semantic-degraded-missing-credentials`
- query: `needle_hotspot`
- filters: none
- semantic: `true` (request toggle), runtime semantic mode enabled non-strict
- purpose: deterministic hybrid fallback path where semantic channel degrades due semantic startup-validation failure (`semantic_runtime.model` blank)

These two semantic hybrid workloads are intentionally control surfaces, not healthy semantic-ok retrieval benchmarks. They verify deterministic hybrid behavior when semantic is explicitly disabled or non-strictly unavailable, while the local sqlite-vec retrieval path is budgeted under `storage_hot_path_latency/semantic_vector_topk/hot-query-batch`.

11. `hybrid/graph-php-target-evidence`
- query: `OrderHandler handle listener`
- filters: none
- semantic: `false` (request toggle)
- purpose: deterministic hybrid ranking path where a warm cached graph artifact plus bounded exact/canonical runtime-path anchor intake contribute graph score to a concrete listener witness

12. `hybrid/benchmark-witness-recall`
- query: `benchmark latest report budget metrics`
- filters: none
- semantic: `false` (request toggle)
- fixture: manifest-backed indexed witness roots, warmed once before timing so the measured path reflects steady-state snapshot-scoped witness projection reuse
- purpose: deterministic benchmark-doc witness recall over benchmark report, methodology docs, and bench support artifacts through the shared candidate universe plus persisted witness projection path

13. `hybrid/path-witness-build-flow`
- query: `entry point bootstrap build flow command runner main config`
- filters: none
- semantic: `false` (request toggle)
- fixture: manifest-backed indexed witness roots, warmed once before timing so the measured path reflects steady-state snapshot-scoped witness projection reuse
- purpose: deterministic entrypoint/build-flow witness recall over runtime files and hidden GitHub workflow configs through the shared candidate universe plus persisted witness projection path

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
- `literal/indexed-manifest-low-limit-high-cardinality`
  - p50: <= 40 ms
  - p95: <= 60 ms
  - p99: <= 80 ms
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
  - p50: <= 130 ms
  - p95: <= 160 ms
  - p99: <= 190 ms
- `hybrid/semantic-degraded-missing-credentials`
  - p50: <= 130 ms
  - p95: <= 160 ms
  - p99: <= 190 ms
- `hybrid/graph-php-target-evidence`
  - p50: <= 30 ms
  - p95: <= 45 ms
  - p99: <= 60 ms
- `hybrid/benchmark-witness-recall`
  - p50: <= 420 ms
  - p95: <= 650 ms
  - p99: <= 750 ms
- `hybrid/path-witness-build-flow`
  - p50: <= 550 ms
  - p95: <= 1100 ms
  - p99: <= 1200 ms

## Measurement Guidance

- Run benchmarks on an otherwise idle machine when possible.
- Run at least twice and compare trend direction, not single-run noise.
- Use the same build profile and command flags across comparisons.
- The direct `search_latency/hybrid/*` budgets are intentionally higher than MCP tool latency because they exercise the raw searcher path, including graph-aware grounding work, without MCP-layer state reuse.
- The witness-recall workloads now benchmark the hot manifest-backed path after one deterministic warmup pass seeds snapshot-scoped witness projection rows; they still include hidden-artifact supplementation and excerpt extraction when ranking the returned matches.
- The graph workload now measures the warm cached graph artifact path after Criterion warmup, including exact-anchor and canonical runtime-path seed intake, rather than per-request graph assembly.
- The stage-attribution report names the last ranking stages as `anchor blend`, `doc corroboration`, and `final diversify` to mirror the real pipeline: blend anchors first, aggregate corroborating anchors per document, then diversify once across aggregated documents.
- The global low-limit literal workloads intentionally remain walk-backed baselines, so small regressions there should be compared against the indexed-manifest literal and witness workloads before budgets are changed.
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
  - includes a `Search Stage Attribution` section when `benchmarks/search-stage-attribution.latest.json` is present
