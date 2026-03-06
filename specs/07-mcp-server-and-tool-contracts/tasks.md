# Tasks — 07-mcp-server-and-tool-contracts

Meta:
- Spec: 07-mcp-server-and-tool-contracts — MCP Surface and Runtime
- Depends on: 00-contracts-and-governance, 01-storage-and-repo-state
- Global scope:
  - crates/mcp/
  - crates/cli/
  - contracts/tools/

## In Progress

- (none)

## Blocked

- (none)

## Todo

## Done

- [x] T004: Add provenance emission per tool invocation (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: crates/mcp/, crates/storage/) (depends: T002)
  - Started_at: 2026-03-04T18:59:00Z
  - Completed_at: 2026-03-04T19:07:39Z
  - Completion note: Added persisted, bounded, best-effort provenance emission for all core tools with integration tests and storage assertions.
  - Validation result: `cargo test -p mcp provenance` and `cargo test -p storage provenance_append_and_load_for_tool` passed.
- [x] T003: Harden transport runtime (stdio + HTTP) with security checks (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: crates/cli/, crates/mcp/) (depends: T001)
  - Started_at: 2026-03-04T17:23:30Z
  - Completed_at: 2026-03-04T18:59:00Z
  - Completion note: Added localhost-safe HTTP defaults, remote-bind override/auth guardrails, optional bearer middleware, and transport-focused runtime tests.
  - Validation result: `cargo test -p frigg` passed (6 tests).
- [x] T002: Implement full tool handler logic integration (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: crates/mcp/) (depends: T001)
  - Started_at: 2026-03-04T17:18:37Z
  - Completed_at: 2026-03-04T17:23:30Z
  - Completion note: Replaced synthetic placeholder tool behavior with deterministic typed handling and integration tests for success/error paths.
  - Validation result: `cargo test -p mcp` passed (unit + integration + doctest).
- [x] T001: Finalize MCP tool schemas and input/output wrappers (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: crates/mcp/, contracts/tools/) (depends: -)
  - Started_at: 2026-03-04T17:09:38Z
  - Completed_at: 2026-03-04T17:18:37Z
  - Completion note: Replaced generic list params with dedicated wrappers, added schema-contract tests, and published per-core-tool v1 schema docs under contracts.
  - Validation result: `cargo test -p mcp schema` passed (6 tests) and docs presence checks passed.
