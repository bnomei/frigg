# Graph Benchmark Methodology (`v1`)

## Scope

This benchmark plan tracks latency for `graph` crate hot paths in `SymbolGraph`:

- relation traversal over high-fanout symbol edges
- precise-reference retrieval for hotspot symbols
- cold-cache SCIP ingest over deterministic multi-document payloads

Harness location:

- `crates/cli/benches/graph_hot_paths.rs`

Run command:

```bash
cargo bench -p frigg --bench graph_hot_paths -- --noplot
```

## Reproducibility Model

The harness is deterministic by construction:

- fixed repository id (`repo-001`)
- fixed symbol ids and relation fanout size
- fixed precise-reference contention fixture size
- fixed cold-cache SCIP document/occurrence counts
- deterministic fixture ordering and repeated-input assertions before measurement

## Workloads

1. `relation_traversal/hot-fanout`
- purpose: benchmark outgoing/incoming relation traversal and hint ranking over a deterministic high-fanout graph.

2. `precise_references/hot-symbol-contention`
- purpose: benchmark repeated precise-reference lookup for a hotspot symbol with deterministic contention-style density.

3. `scip_ingest/cold-cache`
- purpose: benchmark ingesting deterministic SCIP payloads from a cold `SymbolGraph` state each iteration.

## Budget Targets

Budgets are defined in the canonical budget source:

- `benchmarks/budgets.v1.json`

Current graph targets (ms):

- `relation_traversal/hot-fanout`: p50 <= 20, p95 <= 50, p99 <= 80
- `precise_references/hot-symbol-contention`: p50 <= 35, p95 <= 90, p99 <= 140
- `scip_ingest/cold-cache`: p50 <= 80, p95 <= 180, p99 <= 280

## Reporting

Generate consolidated benchmark report output:

```bash
python3 benchmarks/generate_latency_report.py
```

