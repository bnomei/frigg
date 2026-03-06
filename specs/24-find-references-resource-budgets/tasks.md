# Tasks — 24-find-references-resource-budgets

Meta:
- Spec: 24-find-references-resource-budgets — Find References Resource Budgets
- Depends on: 10-mcp-surface-hardening, 11-mcp-hotpath-caching-and-provenance
- Global scope:
  - crates/mcp/, crates/graph/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Find References Resource Budgets and lock with regression coverage (owner: worker:019cbaf3-be4c-7292-9142-e81e3beec33a) (scope: crates/mcp/, crates/graph/) (depends: -)
  - Started_at: 2026-03-04T22:24:38Z
  - Completed_at: 2026-03-04T22:36:04Z
  - Completion note: Added deterministic SCIP/source files resource budgets (bytes/files/time) for `find_references`, mapped budget violations to typed timeout metadata, and added graph+tool handler regressions.
  - Validation result: `cargo test -p mcp --test tool_handlers` passed (14 tests) and `cargo test -p graph` passed (16 tests).
