DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server/workspace.rs:114 | Slug: adopt-workspace-orphan-on-lease-failure

# `adopt_workspace` records session adoption before watch lease acquisition with no rollback

## Finding

`adopt_workspace` inserts the repository into `adopted_repository_ids` and calls `mark_session_adopted` before `watch_runtime.acquire_lease`. If lease acquisition fails, the session remains adopted but the tool returns an error. A subsequent `workspace_attach` sees `newly_adopted = false` and skips the lease retry path.

## Violated Invariant Or Contract

Session adoption and `workspace_attach` success/failure must be atomic: an error response must leave the session without adoption and without bumping global session counts.

## Oracle

After a failed `workspace_attach`, `attached_workspaces()` should be empty (or not include the failed repo), `list_repositories[].session.adopted` should be false for that repo, and retrying attach should re-attempt lease acquisition.

## Counterexample

1. MCP server with watch enabled.
2. `workspace_attach` for a repo where `WatchRuntime::acquire_lease` fails (watcher limit, permissions, invalid root).
3. Tool returns error, but `adopted_repository_ids` already contains the repo and `mark_session_adopted` ran.
4. Retry `workspace_attach` for the same repo: `newly_adopted = false`, lease acquisition block is skipped entirely.
5. Session believes the repo is adopted; watch lease is never acquired; incremental freshness is absent.

## Why It Might Matter

Operators see a failed attach but the session is left in a half-adopted state with no automatic recovery. `workspace_reindex` can also report failure after a successful reindex when adopt fails afterward (`server.rs:1171-1230`), leaving disk state ahead of the reported tool outcome.

## Proof

**Control-flow trace**

`adopt_workspace`: `adopted.insert()` → `mark_session_adopted()` → `acquire_lease()?` with no rollback on `Err` (`workspace.rs:114-141`).

**State transition mismatch**

Allowed: adopt succeeds iff lease succeeds (or watch disabled). Actual: adopt state committed before lease attempt; retry cannot re-enter lease path.

## Counterevidence Checked

Watch tests cover successful adopt/detach lease counting but not lease failure leaving session adopted. `acquire_lease` partial cleanup in `watch/supervisor.rs` only affects its own `lease_counts`, not session adoption.

## Suggested Next Step

Move lease acquisition before adoption commit, or roll back `adopted_repository_ids` and `mark_session_adopted` on lease failure; retry attach when adopted but lease is missing.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed. Confirmed `adopt_workspace` committed `adopted_repository_ids.insert` + `mark_session_adopted` before `acquire_lease`, and propagated a lease error with `?` and no rollback, so a failed attach left the session half-adopted (`newly_adopted == false` on retry → lease path skipped). Kept the atomic `insert()` claim (so concurrent same-repo adopts on one session still single-count), and on `acquire_lease` error now roll back symmetrically: remove the repo from `adopted_repository_ids` and call `mark_session_released` (inverse of `mark_session_adopted`) before returning the error. `release_lease` is intentionally NOT called because the lease was never acquired — `acquire_lease` already cleans up its own partial `lease_counts` on the watch failure path. A retry now re-enters the lease path with `newly_adopted == true`. No deterministic test added: the only failure point is `RecommendedWatcher::watch`, whose behavior on a bad/non-existent root is platform-dependent (macOS FSEvents commonly accepts non-existent paths), and `acquire_lease` is a concrete type with no injection seam, so a forced-failure test would be flaky. Verified the success path is unaffected (`workspace_attach` adopt/detach + watch lease-counting tests pass).

DEVANA-KEY: crates/cli/src/mcp/server/workspace.rs:114 | P1 | adopt-workspace-orphan-on-lease-failure
DEVANA-SUMMARY: Status=fixed | P1 high crates/cli/src/mcp/server/workspace.rs:114 - adopt_workspace now rolls back the session adoption and registry count when watch lease acquisition fails, so a failed attach is atomic and retryable.