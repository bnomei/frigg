# Tasks — 19-mcp-async-blocking-isolation

Meta:
- Spec: 19-mcp-async-blocking-isolation — MCP Async Blocking Isolation
- Depends on: 07-mcp-server-and-tool-contracts, 11-mcp-hotpath-caching-and-provenance
- Global scope:
  - crates/mcp/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement MCP Async Blocking Isolation and lock with regression coverage (owner: mayor) (scope: crates/mcp/) (depends: -)
  - Started_at: 2026-03-04T22:36:48Z
  - Completed_at: 2026-03-04T22:54:04Z
  - Completion note: Recovered interrupted worker changes and finalized blocking isolation helpers (`run_blocking_task`/provenance-blocking path) with handler migration for MCP core tools.
  - Validation result: `cargo test -p mcp --test provenance`, `cargo test -p mcp --test tool_handlers`, and `cargo bench -p mcp --bench tool_latency -- --noplot` passed.
