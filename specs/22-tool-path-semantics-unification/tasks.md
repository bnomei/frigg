# Tasks — 22-tool-path-semantics-unification

Meta:
- Spec: 22-tool-path-semantics-unification — Tool Path Semantics Unification
- Depends on: 07-mcp-server-and-tool-contracts, 13-contract-and-doc-drift-closure
- Global scope:
  - crates/mcp/, crates/search/, contracts/, crates/mcp/tests/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Tool Path Semantics Unification and lock with regression coverage (owner: worker:019cbae5-97c0-7af1-b333-8412cc707a6f) (scope: crates/mcp/, crates/search/, contracts/, crates/mcp/tests/) (depends: -)
  - Started_at: 2026-03-04T22:09:01Z
  - Completed_at: 2026-03-04T22:16:50Z
  - Completion note: Unified core tool response paths to canonical repository-relative format; `read_file` retains absolute-input compatibility while emitting canonical relative paths plus compatibility provenance metadata.
  - Validation result: `cargo test -p mcp --test tool_handlers` passed (12 tests), `cargo test -p mcp citation_payloads` passed (4 tests), and targeted security regression passed.
