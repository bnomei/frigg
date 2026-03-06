# Tasks — 17-symlink-safe-provenance-paths

Meta:
- Spec: 17-symlink-safe-provenance-paths — Symlink-Safe Provenance Paths
- Depends on: 10-mcp-surface-hardening, 11-mcp-hotpath-caching-and-provenance
- Global scope:
  - crates/mcp/, crates/storage/, crates/cli/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Symlink-Safe Provenance Paths and lock with regression coverage (owner: worker:019cbad9-76f1-7f03-97f0-2d5b6e227762) (scope: crates/mcp/, crates/storage/, crates/cli/) (depends: -)
  - Started_at: 2026-03-04T21:55:48Z
  - Completed_at: 2026-03-04T22:03:46Z
  - Completion note: Added canonical/symlink-safe provenance DB resolution helpers and enforced them in MCP provenance writes and CLI storage paths, with new symlink-escape regressions.
  - Validation result: `cargo test -p mcp security` passed (5 tests) and `cargo test -p storage` passed (16 tests).
