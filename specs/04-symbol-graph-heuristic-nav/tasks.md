# Tasks — 04-symbol-graph-heuristic-nav

Meta:
- Spec: 04-symbol-graph-heuristic-nav — Symbol Graph and Heuristic Navigation
- Depends on: 02-ingestion-and-incremental-index
- Global scope:
  - crates/index/
  - crates/graph/
  - crates/mcp/

## In Progress

- (none)

## Blocked

- (none)

## Todo

## Done

- [x] T004: Wire `search_symbol` and heuristic `find_references` in MCP server (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: crates/mcp/) (depends: T001, T003)
  - Started_at: 2026-03-04T19:21:56Z
  - Completed_at: 2026-03-04T19:28:13Z
  - Completion note: Replaced MCP placeholders with real symbol/reference query paths and deterministic heuristic metadata in tool responses.
  - Validation result: `cargo test -p mcp` passed (unit + integration + doctest).
- [x] T003: Implement heuristic reference resolver (owner: worker:019cb9aa-fae5-7d13-a6e5-6b73c3f68600) (scope: crates/index/, crates/graph/) (depends: T001, T002)
  - Started_at: 2026-03-04T19:13:11Z
  - Completed_at: 2026-03-04T19:21:56Z
  - Completion note: Added deterministic heuristic reference resolver combining graph hints and lexical fallback with confidence/evidence metadata and false-positive bound tests.
  - Validation result: `cargo test -p indexer heuristic_references` passed.
- [x] T002: Implement graph storage model for symbol and reference edges (owner: worker:019cb9aa-fae5-7d13-a6e5-6b73c3f68600) (scope: crates/graph/) (depends: T001)
  - Started_at: 2026-03-04T19:09:26Z
  - Completed_at: 2026-03-04T19:13:11Z
  - Completion note: Added deterministic in-memory symbol graph model with typed relations, adjacency queries, and traversal/upsert tests.
  - Validation result: `cargo test -p graph` passed.
- [x] T001: Implement Rust/PHP symbol extraction pipeline (owner: worker:019cb9aa-fae5-7d13-a6e5-6b73c3f68600) (scope: crates/index/) (depends: -)
  - Started_at: 2026-03-04T19:04:22Z
  - Completed_at: 2026-03-04T19:09:26Z
  - Completion note: Added deterministic Rust/PHP tree-sitter symbol extraction with stable IDs/spans, diagnostics-on-failure behavior, and focused fixture tests.
  - Validation result: `cargo test -p indexer symbols_rust_php` passed (4 tests).
