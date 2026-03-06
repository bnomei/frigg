# Tasks — 15-heuristic-fallback-scale

Meta:
- Spec: 15-heuristic-fallback-scale — Heuristic Fallback Scale Fix
- Depends on: 04-symbol-graph-heuristic-nav, 11-mcp-hotpath-caching-and-provenance
- Global scope:
  - crates/index/, crates/mcp/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Heuristic Fallback Scale Fix and lock with regression coverage (owner: mayor) (scope: crates/index/, crates/mcp/) (depends: -)
  - Started_at: 2026-03-04T22:36:48Z
  - Completed_at: 2026-03-04T22:54:04Z
  - Completion note: Recovered interrupted worker changes and finalized scale fix via per-file/per-line heuristic reference resolver strategy with streaming-friendly source ingestion to avoid prior superlinear fallback behavior.
  - Validation result: `cargo test -p indexer heuristic_references` and `cargo test -p mcp --test tool_handlers` passed.
