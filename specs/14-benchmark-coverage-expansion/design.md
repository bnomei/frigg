# Design — 14-benchmark-coverage-expansion

## Architecture
- `crates/mcp/benches/tool_latency.rs`
  - add workloads for precise `find_references` path and provenance-heavy repeated tool calls.
- `crates/search/benches/search_latency.rs`
  - add low-limit/high-cardinality and larger fixture scenarios.
- `crates/index/` (new bench if needed)
  - add reindex throughput benchmark (full + changed-only).
- `benchmarks/budgets.v1.json`
  - extend canonical workload IDs and budgets.
- `benchmarks/latest-report.md`
  - regenerate deterministic summary.

## Acceptance signals
- New benchmark IDs appear in budgets and reports.
- Existing report generator accepts new workloads with zero missing entries.
- Benchmarks execute successfully on local fixture data.
