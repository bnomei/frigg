# Reindex Benchmark Methodology (`v1`)

## Scope

This benchmark plan tracks indexer `reindex_repository` latency for full and changed-only modes.

Harness location:

- `crates/cli/benches/reindex_latency.rs`

Run command:

```bash
cargo bench -p frigg --bench reindex_latency
```

Canonical budget source:

- `benchmarks/budgets.v1.json`

## Reproducibility Model

The benchmark harness is deterministic by construction:

- fixed repository id (`repo-001`)
- fixed fixture size (`120` files, `48` lines per file)
- fixed changed-only delta shape (`12` modified files, `1` deleted file, `1` added file)
- fixed path layout (`src/module_XX/file_YYY.rs`)
- fixed mode-specific setup steps (`full`, `changed-only` no-op, `changed-only` delta)

## Workloads

1. `reindex_repository/full-throughput`
- purpose: full reindex baseline over a deterministic fixture

2. `reindex_repository/changed-only-noop`
- purpose: changed-only run where no file changes are detected after an initial full snapshot

3. `reindex_repository/changed-only-delta`
- purpose: changed-only run with deterministic add/modify/delete delta

## Budget Targets

Current reindex targets (ms):

- `reindex_repository/full-throughput`: p50 <= 60, p95 <= 120, p99 <= 180
- `reindex_repository/changed-only-noop`: p50 <= 60, p95 <= 130, p99 <= 200
- `reindex_repository/changed-only-delta`: p50 <= 60, p95 <= 130, p99 <= 200

## Reporting

Generate consolidated benchmark report output:

```bash
python3 benchmarks/generate_latency_report.py
```

Output contract:

- deterministic key-value lines to stdout:
  - `benchmark_report_version=...`
  - per-workload `status`, `p50_ms`, `p95_ms`, `p99_ms`, and budget fields
  - `summary pass=... fail=... missing=...`
  - `report_path=...`
- markdown report file:
  - `benchmarks/latest-report.md`
