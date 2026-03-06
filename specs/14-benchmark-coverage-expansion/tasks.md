# Tasks — 14-benchmark-coverage-expansion

Meta:
- Spec: 14-benchmark-coverage-expansion — Benchmark Coverage Expansion
- Depends on: 09-security-benchmarks-ops, 11-mcp-hotpath-caching-and-provenance, 12-search-index-hotpath-and-correctness
- Global scope:
  - crates/mcp/benches/
  - crates/search/benches/
  - crates/index/benches/
  - benchmarks/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T003: Extend benchmark budget/report artifacts for new workload IDs (owner: worker:019cbaa7-296c-7333-8138-2d53c05c9d94) (scope: benchmarks/) (depends: T001, T002)
  - Started_at: 2026-03-04T21:00:50Z
  - Completed_at: 2026-03-04T21:06:21Z
  - Completion note: Added budget rows and regenerated report entries for all new MCP/search/index workload IDs introduced by T001/T002.
  - Validation result: `python3 benchmarks/generate_latency_report.py` and `bash scripts/check-release-readiness.sh` passed with `summary pass=15 fail=0 missing=0`.
- [x] T001: Add MCP benchmark workloads for precise references and provenance-write overhead (owner: worker:019cba99-4736-7e11-a4db-49353a0da232) (scope: crates/mcp/benches/, fixtures/) (depends: -)
  - Started_at: 2026-03-04T20:45:40Z
  - Completed_at: 2026-03-04T20:54:27Z
  - Completion note: Added deterministic precise-reference and provenance-overhead MCP benchmark workloads plus SCIP fixture wiring.
  - Validation result: `cargo bench -p mcp --bench tool_latency -- --noplot` passed.
- [x] T002: Add search/index benchmark workloads for reindex throughput and low-limit high-cardinality search (owner: worker:019cba9d-6de2-76a2-8d9b-4c4c44d4ffbc) (scope: crates/search/benches/, crates/index/benches/) (depends: -)
  - Started_at: 2026-03-04T20:50:12Z
  - Completed_at: 2026-03-04T21:00:06Z
  - Completion note: Added new search high-cardinality low-limit workload and new index reindex throughput bench; mayor added `crates/index/Cargo.toml` bench wiring (`criterion` dev-dependency + `reindex_latency` bench target) to clear scope blocker.
  - Validation result: `cargo bench -p searcher --bench search_latency -- --noplot` and `cargo bench -p indexer --bench reindex_latency -- --noplot` passed.
