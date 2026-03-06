# Tasks — 11-mcp-hotpath-caching-and-provenance

Meta:
- Spec: 11-mcp-hotpath-caching-and-provenance — MCP Hot Path Caching and Provenance Throughput
- Depends on: 10-mcp-surface-hardening
- Global scope:
  - crates/mcp/
  - crates/storage/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add repository-scoped symbol corpus cache with deterministic invalidation signatures (owner: worker:019cbaa3-f158-7480-a6d8-fe11b402695b) (scope: crates/mcp/src/mcp/server.rs) (depends: -)
  - Started_at: 2026-03-04T20:57:17Z
  - Completed_at: 2026-03-04T21:08:02Z
  - Completion note: Added repository-scoped symbol corpus cache keyed by deterministic `root_signature` to reuse corpus builds across unchanged requests.
  - Validation result: `cargo test -p mcp --test tool_handlers` passed (10 tests), `cargo bench -p mcp --bench tool_latency -- --noplot` passed.
- [x] T002: Add precise SCIP graph cache keyed by artifact signature for `find_references` (owner: worker:019cbaa3-f158-7480-a6d8-fe11b402695b) (scope: crates/mcp/src/mcp/server.rs, crates/graph/src/lib.rs) (depends: T001)
  - Started_at: 2026-03-04T20:57:17Z
  - Completed_at: 2026-03-04T21:08:02Z
  - Completion note: Added cached precise graph reuse keyed by `(repository_id, scip_signature, corpus_signature)` for deterministic repeat `find_references`.
  - Validation result: `cargo test -p graph` passed (13 tests), `cargo test -p mcp --test tool_handlers` passed (10 tests).
- [x] T003: Reuse initialized provenance storage and monotonic unique trace IDs (owner: worker:019cbaa3-f158-7480-a6d8-fe11b402695b) (scope: crates/mcp/src/mcp/server.rs, crates/storage/src/lib.rs) (depends: -)
  - Started_at: 2026-03-04T20:57:17Z
  - Completed_at: 2026-03-04T21:08:02Z
  - Completion note: Implemented reused provenance storage handles and switched trace IDs to UUIDv7 generation.
  - Validation result: `cargo test -p mcp --test provenance` passed (2 tests), `cargo test -p storage` passed (12 tests).
- [x] T004: Add/verify storage indexes for latest snapshot and provenance tool queries (owner: worker:019cbaa3-f158-7480-a6d8-fe11b402695b) (scope: crates/storage/src/lib.rs, crates/storage/tests if present) (depends: T003)
  - Started_at: 2026-03-04T20:57:17Z
  - Completed_at: 2026-03-04T21:08:02Z
  - Completion note: Added migration index DDL and tests that verify both index existence and query-plan usage for hot-path snapshot/provenance lookups.
  - Validation result: `cargo test -p storage` passed (12 tests).
- (none)
