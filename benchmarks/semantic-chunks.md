# Semantic Chunk Benchmark Methodology (`v1`)

## Scope

This benchmark plan tracks indexer semantic-chunk hot paths for allocation-heavy chunk splitting and batch candidate construction:

- per-file semantic chunk construction for large Rust source inputs
- per-file semantic chunk construction for heading-dense Markdown inputs
- manifest-driven semantic chunk candidate construction over mixed-language batches

This document does not cover sqlite-vec query latency. Healthy local semantic retrieval is benchmarked separately in [`storage.md`](./storage.md) because the vector top-k path runs against persisted storage projections, not the chunk-construction pipeline itself.

Harness location:

- `crates/cli/benches/semantic_chunk_hot_paths.rs`

Run command:

```bash
cargo bench -p frigg --bench semantic_chunk_hot_paths -- --noplot
```

Canonical budget source:

- `benchmarks/budgets.v1.json`

## Reproducibility Model

The harness is deterministic by construction:

- fixed large-source line counts and repeated token content
- fixed heading count and prose density for Markdown fanout
- fixed mixed-language manifest shape and file-count distribution
- deterministic temporary-root naming, fixture generation, and manifest building

## Workloads

1. `file/rust-large-split`
- purpose: benchmark semantic chunk construction for a large Rust file with repeated function boundaries and high content-byte volume.

2. `file/markdown-heading-fanout`
- purpose: benchmark semantic chunk construction for heading-dense Markdown that flushes chunk boundaries frequently.

3. `manifest/mixed-language-batch`
- purpose: benchmark semantic chunk candidate construction over a deterministic mixed-language manifest batch, excluding skipped playbook files.

## Budget Targets

Budgets are defined in the canonical budget source:

- `benchmarks/budgets.v1.json`

Current semantic chunk targets (ms):

- `file/rust-large-split`: p50 <= 2, p95 <= 5, p99 <= 8
- `file/markdown-heading-fanout`: p50 <= 1, p95 <= 3, p99 <= 5
- `manifest/mixed-language-batch`: p50 <= 60, p95 <= 90, p99 <= 120

## Reporting

Generate consolidated benchmark report output:

```bash
python3 benchmarks/generate_latency_report.py
```
