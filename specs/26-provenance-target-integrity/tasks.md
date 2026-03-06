# Tasks — 26-provenance-target-integrity

Meta:
- Spec: 26-provenance-target-integrity — Provenance Target Integrity
- Depends on: 11-mcp-hotpath-caching-and-provenance
- Global scope:
  - crates/mcp/, crates/mcp/tests/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Provenance Target Integrity and lock with regression coverage (owner: worker:019cbad5-f0e2-79a3-8b7a-252038242169) (scope: crates/mcp/, crates/mcp/tests/) (depends: -)
  - Started_at: 2026-03-04T21:51:56Z
  - Completed_at: 2026-03-04T21:54:44Z
  - Completion note: Invalid repository hints no longer fall back to default provenance target; added regression asserting no misattributed event persistence.
  - Validation result: `cargo test -p mcp --test provenance` passed (3 tests) and `cargo test -p mcp --test tool_handlers` passed (10 tests).
