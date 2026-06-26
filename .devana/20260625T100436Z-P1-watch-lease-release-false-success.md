DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/watch/supervisor.rs:326 | Slug: watch-lease-release-false-success

# Watch supervisor marks refresh succeeded when repository Arc entry is missing after lease release

## Finding

`release_lease` removes the repository from the shared `repositories` Arc synchronously before the supervisor handles `LeaseReleased`. If a supervisor tick runs between removal and scheduler cleanup, it can call `mark_started` (draining dirty paths), find no Arc entry, and call `mark_succeeded` without spawning reindex work.

## Violated Invariant Or Contract

A scheduled manifest or semantic refresh must either run to completion or remain pending until it can run. `mark_started` must not consume debounce state and dirty-path samples without executing work.

## Oracle

After a filesystem change, manifest snapshot id or file digests should advance. The scheduler must not report success without a reindex task having run.

## Counterexample

1. Client holds a watch lease; `record_path_change` queues `ManifestFast` and records paths in `recent_paths`.
2. Client calls `release_lease()` while refresh is due on the next tick.
3. `release_lease` removes the repo from `repositories` synchronously (`supervisor.rs:133-137`), then sends `LeaseReleased` asynchronously.
4. Supervisor tick runs before `LeaseReleased` is handled: `next_ready_refresh` returns the job, `mark_started` drains `recent_paths`.
5. `repositories.get()` returns `None`; branch at `supervisor.rs:326-328` calls `mark_succeeded` and continues with no `spawn_blocking`.

## Why It Might Matter

Filesystem changes can be dropped with a false success signal, leaving the index stale until another watch event arrives. Distinct from the known detach completion race at line 485 (that path returns early without `mark_succeeded`).

## Proof

**Control-flow trace**

`release_lease` Arc remove (sync) → tick: `mark_started` → `repositories.get() == None` → `mark_succeeded` → no reindex.

**Counterexample value**

Interleave lease release with supervisor tick between Arc removal and `LeaseReleased` handler.

## Counterevidence Checked

`queue_startup_refresh_if_needed` on re-lease may eventually recover if freshness still reports stale, but narrow changes that leave manifest "valid" can remain unindexed. `handle_reindex_completed` detach race (existing P3 report) is a different mechanism.

## Suggested Next Step

Treat missing Arc entry as defer/re-enqueue (`mark_failed` with retry) rather than success, or make lease release and scheduler cleanup atomic.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed. Confirmed the supervisor tick called `scheduler.mark_started` (which for ManifestFast drains `recent_paths` and clears the pending/debounce state) *before* fetching the repository Arc, then on a missing Arc called `mark_succeeded` — a false success that discarded the dirty-path samples. `next_ready_refresh` takes `&self` (read-only), so the fix reorders: resolve the live repository Arc first; if it is absent (lease released, `LeaseReleased` command not yet handled) `continue` without draining or marking anything, so the pending refresh survives for a re-lease and `LeaseReleased` → `remove_repository` cleans up the scheduler entry shortly after. The SemanticFollowup alias-dedup guard was also moved ahead of `mark_started`; for SemanticFollowup `mark_started` returns no paths, so skipping straight to `mark_succeeded` leaves equivalent scheduler state. No deterministic test added: the race lives inside the spawned async supervisor select-loop driven by real timers, with no pure seam; verified the full `watch::` suite (20 tests) still passes.

DEVANA-KEY: crates/cli/src/watch/supervisor.rs:326 | P1 | watch-lease-release-false-success
DEVANA-SUMMARY: Status=fixed | P1 high crates/cli/src/watch/supervisor.rs:326 - Supervisor now resolves the live repository before consuming scheduler state and skips (no drain, no false mark_succeeded) when the lease was released, preserving pending refreshes.