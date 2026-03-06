# Tasks — 28-storage-error-trace-diff-corrections

Meta:
- Spec: 28-storage-error-trace-diff-corrections — Storage Error and Trace Diff Corrections
- Depends on: 00-contracts-and-governance, 08-hybrid-retrieval-and-deep-search-harness
- Global scope:
  - crates/storage/, crates/mcp/, contracts/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Storage Error and Trace Diff Corrections and lock with regression coverage (owner: worker:019cbaed-7027-73e3-903e-09ae5f6282a1) (scope: crates/storage/, crates/mcp/, contracts/) (depends: -)
  - Started_at: 2026-03-04T22:17:34Z
  - Completed_at: 2026-03-04T22:23:39Z
  - Completion note: Zero vector dimensions now map to typed invalid input; deep-search trace diff now validates steps vector structural consistency before zip comparison with deterministic mismatch diagnostics.
  - Validation result: `cargo test -p storage` passed (18 tests) and `cargo test -p mcp playbook_suite` passed (unit+integration suites).
