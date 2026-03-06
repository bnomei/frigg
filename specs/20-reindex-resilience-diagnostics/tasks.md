# Tasks — 20-reindex-resilience-diagnostics

Meta:
- Spec: 20-reindex-resilience-diagnostics — Reindex Resilience Diagnostics
- Depends on: 02-ingestion-and-incremental-index
- Global scope:
  - crates/index/, crates/cli/, contracts/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Reindex Resilience Diagnostics and lock with regression coverage (owner: worker:019cbace-eeaf-77c1-998f-b00634c45e79) (scope: crates/index/, crates/cli/, contracts/) (depends: -)
  - Started_at: 2026-03-04T21:44:20Z
  - Completed_at: 2026-03-04T21:52:42Z
  - Completion note: Reindex now uses manifest diagnostics path (`build_with_diagnostics`) to continue on unreadable files with deterministic typed summary counts and CLI output.
  - Validation result: `cargo test -p indexer` passed (16 tests) and `cargo test -p frigg` passed (10 tests).
