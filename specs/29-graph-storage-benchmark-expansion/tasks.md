# Tasks — 29-graph-storage-benchmark-expansion

Meta:
- Spec: 29-graph-storage-benchmark-expansion — Graph and Storage Benchmark Expansion
- Depends on: 14-benchmark-coverage-expansion
- Global scope:
  - crates/graph/benches/, crates/storage/benches/, benchmarks/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Graph and Storage Benchmark Expansion and lock with regression coverage (owner: worker:019cbad9-7c48-7130-b2c3-bbd482130f6d) (scope: crates/graph/benches/, crates/storage/benches/, benchmarks/) (depends: -)
  - Started_at: 2026-03-04T21:55:48Z
  - Completed_at: 2026-03-04T22:11:06Z
  - Completion note: Added graph/storage hot-path benches, benchmark docs, and budget entries; mayor unblocked bench execution by wiring bench targets in `crates/graph/Cargo.toml` and `crates/storage/Cargo.toml`.
  - Validation result: `cargo bench -p graph --bench graph_hot_paths -- --noplot`, `cargo bench -p storage --bench storage_hot_paths -- --noplot`, and `python3 benchmarks/generate_latency_report.py` passed with `summary pass=21 fail=0 missing=0`.
