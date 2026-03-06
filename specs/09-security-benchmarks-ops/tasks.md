# Tasks — 09-security-benchmarks-ops

Meta:
- Spec: 09-security-benchmarks-ops — Security, Performance, and Operations
- Depends on: 03-text-search-engine, 04-symbol-graph-heuristic-nav, 06-embeddings-and-vector-store, 07-mcp-server-and-tool-contracts
- Global scope:
  - docs/security/
  - benchmarks/
  - scripts/
  - crates/cli/
  - crates/mcp/
  - crates/search/

## In Progress

- (none)

## Blocked

- (none)

## Todo

## Done

- [x] T004: Add release-readiness checklist gate (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: docs/security/, benchmarks/, scripts/) (depends: T001, T002, T003)
  - Started_at: 2026-03-04T19:47:48Z
  - Completed_at: 2026-03-04T19:52:57Z
  - Completion note: Added release-readiness checklist and deterministic gate script with controlled fail-path drill support.
  - Validation result: `bash scripts/check-release-readiness.sh` pass/fail/pass sequence verified.
- [x] T002: Implement benchmark harness and budget files (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: benchmarks/, crates/search/, crates/mcp/) (depends: -)
  - Started_at: 2026-03-04T19:36:20Z
  - Completed_at: 2026-03-04T19:47:48Z
  - Completion note: Added MCP benchmark harness, machine-readable budget contract, report generator, and refreshed benchmark docs/report artifacts.
  - Validation result: `cargo bench -p searcher` and `python3 benchmarks/generate_latency_report.py` passed.
- [x] T001: Build security regression suite for path, regex, and transport abuse cases (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: crates/mcp/, crates/search/, docs/security/) (depends: -)
  - Started_at: 2026-03-04T19:28:46Z
  - Completed_at: 2026-03-04T19:36:20Z
  - Completion note: Added path traversal/workspace-boundary and regex-abuse security regressions plus mapped threat model documentation.
  - Validation result: `cargo test -p mcp security` and `cargo test -p searcher security` passed.
- [x] T003: Finalize operability command suite and deterministic outputs (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/cli/, scripts/) (depends: -)
  - Started_at: 2026-03-04T19:28:46Z
  - Completed_at: 2026-03-04T19:36:20Z
  - Completion note: Added deterministic end-to-end `scripts/smoke-ops.sh` contract checks for init/reindex/reindex --changed/verify.
  - Validation result: `bash scripts/smoke-ops.sh` passed.
