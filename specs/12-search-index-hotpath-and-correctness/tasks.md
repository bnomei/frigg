# Tasks — 12-search-index-hotpath-and-correctness

Meta:
- Spec: 12-search-index-hotpath-and-correctness — Search/Index Hot Path and Correctness
- Depends on: 03-text-search-engine, 04-symbol-graph-heuristic-nav
- Global scope:
  - crates/search/
  - crates/index/
  - crates/mcp/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T003: Surface deterministic diagnostics for walker/read failures in search/index paths (owner: worker:019cbaa7-296c-7333-8138-2d53c05c9d94) (scope: crates/search/src/lib.rs, crates/index/src/lib.rs, crates/mcp/src/mcp/server.rs) (depends: -)
  - Started_at: 2026-03-04T21:09:37Z
  - Completed_at: 2026-03-04T21:20:22Z
  - Completion note: Added deterministic search execution diagnostics (walk/read), propagated manifest/symbol/source diagnostics into MCP tool note/provenance metadata.
  - Validation result: `cargo test -p searcher` passed (20 tests), `cargo test -p mcp --test tool_handlers` passed (10 tests).
- [x] T004: Add regressions and benchmark assertions for low-limit/large-corpus behavior (owner: worker:019cbaa7-296c-7333-8138-2d53c05c9d94) (scope: crates/search/src/lib.rs, crates/search/benches/search_latency.rs, benchmarks/) (depends: T001)
  - Started_at: 2026-03-04T21:09:37Z
  - Completed_at: 2026-03-04T21:20:22Z
  - Completion note: Added deterministic low-limit/large-corpus regression coverage and benchmark fixture assertion for high-cardinality low-limit behavior.
  - Validation result: `cargo test -p searcher` and `cargo bench -p searcher --bench search_latency -- --noplot` passed.
- [x] T001: Optimize search hot loop allocations and bounded-result behavior (owner: worker:019cba95-98a5-7de2-afd9-2abf90ec89d9) (scope: crates/search/src/lib.rs, crates/search/benches/search_latency.rs) (depends: -)
  - Started_at: 2026-03-04T20:41:46Z
  - Completed_at: 2026-03-04T20:48:59Z
  - Completion note: Reworked match-column extraction to reusable buffers with bounded deterministic retention for small limits; added low-limit regression benchmark/test.
  - Validation result: `cargo test -p searcher` and `cargo bench -p searcher --bench search_latency -- --noplot` passed.
- [x] T002: Preserve multiple same-line heuristic references and deterministic ordering (owner: worker:019cba95-98a5-7de2-afd9-2abf90ec89d9) (scope: crates/index/src/lib.rs) (depends: -)
  - Started_at: 2026-03-04T20:41:46Z
  - Completed_at: 2026-03-04T20:48:59Z
  - Completion note: Heuristic dedupe key now includes column and lexical token scanning preserves multiple same-line hits with deterministic order/tests.
  - Validation result: `cargo test -p indexer heuristic_references` passed.
