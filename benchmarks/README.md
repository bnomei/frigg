# Benchmarks

Benchmark plans, workloads, and result summaries for Frigg.

## Documents

- `search.md`: searcher latency workloads and budgets.
- `mcp-tools.md`: MCP tool-call latency workloads and budgets.
- `graph.md`: symbol-graph hot-path latency workloads and budgets.
- `storage.md`: storage hot-path latency workloads and budgets.
- `reindex.md`: indexer reindex latency workloads and budgets.
- `deep-search.md`: deep-search playbook-suite acceptance metrics and deterministic replay criteria.

## Budget Source

- `budgets.v1.json` is the canonical machine-readable budget contract.

## Report Generator

```bash
python3 benchmarks/generate_latency_report.py
```

The generator reads Criterion outputs from deterministic benchmark roots
(`target/criterion`, `crates/{graph,index,mcp,search,storage}/target/criterion`),
compares against `budgets.v1.json`, prints deterministic key-value summaries, and
writes `benchmarks/latest-report.md`.

## Release Gate Contract

The release-readiness gate (`scripts/check-release-readiness.sh`) consumes these benchmark artifacts:

- `benchmarks/budgets.v1.json` must exist.
- Gate executes:
  - `cargo bench -p frigg --bench search_latency -- --noplot`
  - `cargo bench -p frigg --bench tool_latency -- --noplot`
  - `cargo bench -p frigg --bench graph_hot_paths -- --noplot`
  - `cargo bench -p frigg --bench storage_hot_paths -- --noplot`
  - `cargo bench -p frigg --bench reindex_latency -- --noplot`
- `python3 benchmarks/generate_latency_report.py --fail-on-budget --output <tmp>`
- semantic runtime coverage is included through deterministic hybrid workload IDs in:
  - `search_latency/hybrid/*`
  - `mcp_tool_latency/search_hybrid/*`
- Fresh generator stdout must include `summary pass=<int> fail=<int> missing=<int>` with `fail=0` and `missing=0`.
- `benchmarks/latest-report.md` must retain workload/budget parity with the fresh machine-produced report artifact (same workload ids/order and budget cells).
