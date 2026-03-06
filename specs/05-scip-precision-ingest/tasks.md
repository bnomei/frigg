# Tasks — 05-scip-precision-ingest

Meta:
- Spec: 05-scip-precision-ingest — SCIP Precision Path
- Depends on: 04-symbol-graph-heuristic-nav
- Global scope:
  - crates/graph/
  - crates/mcp/
  - fixtures/scip/

## In Progress

- (none)

## Blocked

- (none)

## Todo

## Done

- [x] T002: Implement precision-aware reference query resolver (owner: worker:019cb9aa-fae5-7d13-a6e5-6b73c3f68600) (scope: crates/graph/, crates/mcp/) (depends: T001)
  - Started_at: 2026-03-04T19:47:48Z
  - Completed_at: 2026-03-04T19:52:57Z
  - Completion note: Wired precise-first `find_references` precedence through MCP with deterministic fallback and precision metadata notes.
  - Validation result: `cargo test -p mcp precision_precedence` passed.
- [x] T004: Create SCIP fixture pack and regression matrix (owner: worker:019cb9aa-fae5-7d13-a6e5-6b73c3f68600) (scope: fixtures/scip/, crates/graph/) (depends: T001)
  - Started_at: 2026-03-04T19:43:13Z
  - Completed_at: 2026-03-04T19:46:14Z
  - Completion note: Added synthetic SCIP fixture pack with regression matrix coverage for definitions/references, relationships, role-bits, and invalid-range diagnostics.
  - Validation result: `cargo test -p graph scip_fixture_matrix` and `cargo test -p graph` passed.
- [x] T003: Add incremental SCIP update and replace semantics (owner: worker:019cb9aa-fae5-7d13-a6e5-6b73c3f68600) (scope: crates/graph/) (depends: T001)
  - Started_at: 2026-03-04T19:36:20Z
  - Completed_at: 2026-03-04T19:43:13Z
  - Completion note: Added deterministic file-level SCIP incremental replacement semantics preserving unaffected precise data.
  - Validation result: `cargo test -p graph scip_incremental_update` and `cargo test -p graph` passed.
- [x] T001: Build SCIP ingestion parser and mapping layer (owner: worker:019cb9aa-fae5-7d13-a6e5-6b73c3f68600) (scope: crates/graph/) (depends: -)
  - Started_at: 2026-03-04T19:28:46Z
  - Completed_at: 2026-03-04T19:36:20Z
  - Completion note: Added typed SCIP ingest pipeline (parse/map/apply), normalized precise storage, deterministic precise query APIs, and invalid-input diagnostics.
  - Validation result: `cargo test -p graph scip_ingest` and `cargo test -p graph` passed.
