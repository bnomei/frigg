# Tasks — 03-text-search-engine

Meta:
- Spec: 03-text-search-engine — Deterministic Text Search
- Depends on: 02-ingestion-and-incremental-index
- Global scope:
  - crates/search/
  - crates/index/
  - benchmarks/

## In Progress

- (none)

## Blocked

- (none)

## Todo

## Done

- [x] T004: Add benchmark doc + harness for query latency budgets (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/search/, benchmarks/) (depends: T002, T003)
  - Started_at: 2026-03-04T19:15:10Z
  - Completed_at: 2026-03-04T19:21:07Z
  - Completion note: Added Criterion benchmark harness for literal/regex workloads and documented search latency methodology plus p50/p95/p99 budgets.
  - Validation result: `cargo bench -p searcher` passed (bench metrics emitted; regex workload currently above baseline in this run).
- [x] T003: Add ranking/filter pipeline and stable ordering guarantees (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/search/, crates/index/) (depends: T001, T002)
  - Started_at: 2026-03-04T19:12:02Z
  - Completed_at: 2026-03-04T19:15:10Z
  - Completion note: Added normalized filter pipeline and shared deterministic ordering guarantees for literal/regex search paths with repeated-run tests.
  - Validation result: `cargo test -p searcher ordering` passed (3 tests).
- [x] T002: Implement safe regex search path with bounded behavior (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/search/) (depends: T001)
  - Started_at: 2026-03-04T19:08:52Z
  - Completed_at: 2026-03-04T19:12:02Z
  - Completion note: Added bounded regex query mode with typed invalid/abusive-pattern errors, deterministic ordering, and filter-aware regex tests.
  - Validation result: `cargo test -p searcher regex_search` passed (4 tests).
- [x] T001: Implement deterministic literal search engine over indexed candidates (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/search/) (depends: -)
  - Started_at: 2026-03-04T19:04:22Z
  - Completed_at: 2026-03-04T19:08:52Z
  - Completion note: Added deterministic literal search with stable ordering and repository/path filtering plus focused determinism tests.
  - Validation result: `cargo test -p searcher literal_search` passed (3 tests).
