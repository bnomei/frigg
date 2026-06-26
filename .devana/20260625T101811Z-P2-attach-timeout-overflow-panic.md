DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server.rs:666 | Slug: attach-timeout-overflow-panic

# Oversized Attach Timeout Can Panic

## Finding

`workspace_attach` accepts an unbounded `index_timeout_ms`, converts it to `Duration`, and passes it to a hand-rolled wait loop that adds it to `tokio::time::Instant::now()`.

## Violated Invariant Or Contract

User-supplied timeout values should be validated or capped before timer arithmetic. Bad timeout values should return a typed error or timeout, not panic.

## Oracle

The attach params expose raw `u64` milliseconds with no schema cap. Rust instant addition can panic when the resulting instant is out of range.

## Counterexample

Call `workspace_attach` with `index_mode=ensure`, `wait_for_index=true`, and an extremely large timeout while the repository already has active index work. The active-work wait path computes an overflowing deadline.

## Why It Might Matter

A malformed MCP request can crash the server process instead of returning an invalid-params response.

## Proof

Dataflow trace: `workspace_attach` builds `Duration::from_millis(params.index_timeout_ms.unwrap_or(30_000))` at `crates/cli/src/mcp/server.rs:666`. If active index work exists, `ensure_workspace_index_for_attach` calls `wait_for_repository_index_work`, which computes `tokio::time::Instant::now() + timeout` at `crates/cli/src/mcp/server/workspace_session.rs:334`.

## Counterevidence Checked

The spawned-refresh branch uses `tokio::time::timeout`, which handles large deadlines differently. The precise-generation wait loop uses fixed 30-second values in current callers.

## Suggested Next Step

Cap `index_timeout_ms` to a sane maximum or use checked deadline arithmetic and return an invalid-params error for unrepresentable durations.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed. Confirmed `workspace_attach` built `Duration::from_millis(index_timeout_ms)` from an unbounded `u64` (types/workspace.rs:248) and `wait_for_repository_index_work` computed `tokio::time::Instant::now() + timeout`, which panics for absurd durations. Two-layer fix: (1) at the param boundary (server.rs:666) clamp `index_timeout_ms` to `MAX_INDEX_TIMEOUT_MS = 1 hour` before building the Duration — far beyond any legitimate attach-time index wait; (2) defense in depth in the wait loop (workspace_session.rs:335) use `Instant::checked_add`, falling back to a representable `now + 1h` bound if it ever overflows, so the loop can never panic regardless of caller. Compiles; existing `workspace_attach` tests pass. No bespoke test added: the private async wait loop returns immediately when no index work is active, so exercising the deadline overflow path deterministically would need a contrived active-task fixture; the clamp + checked_add make the panic unreachable by construction.

DEVANA-KEY: crates/cli/src/mcp/server.rs:666 | P2 | attach-timeout-overflow-panic
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/mcp/server.rs:666 - Attach index timeout is now clamped to 1h at the param boundary and the wait loop uses checked Instant arithmetic, so an oversized timeout can no longer panic.
