# Tasks — 18-provenance-strict-persistence

Meta:
- Spec: 18-provenance-strict-persistence — Provenance Strict Persistence
- Depends on: 11-mcp-hotpath-caching-and-provenance
- Global scope:
  - crates/mcp/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Provenance Strict Persistence and lock with regression coverage (owner: worker:019cbae0-2160-7ee0-a8e0-5e33b2c81d4b) (scope: crates/mcp/) (depends: -)
  - Started_at: 2026-03-04T22:03:05Z
  - Completed_at: 2026-03-04T22:08:35Z
  - Completion note: Provenance persistence is strict-by-default with typed `provenance_persistence_failed` metadata; best-effort is now explicit opt-in (`FRIGG_MCP_PROVENANCE_BEST_EFFORT` / constructor flag).
  - Validation result: `cargo test -p mcp --test provenance` passed (5 tests), `cargo test -p mcp --test tool_handlers` passed (11 tests), and `cargo test -p mcp --test security` passed (5 tests).
