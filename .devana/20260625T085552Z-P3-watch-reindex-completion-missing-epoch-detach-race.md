DEVANA-FINDING: v1
Priority: P3 | Confidence: medium | Security-sensitive: no | Status: fixed
Location: crates/cli/src/watch/supervisor.rs:485 | Slug: watch-reindex-completion-missing-epoch-detach-race

# Watch reindex completion keyed only on repository_id; detach/re-attach race drops a freshly queued refresh

## Finding

`SupervisorCommand::ReindexCompleted` carries no run/epoch token, only the
`repository_id`. `handle_reindex_completed` looks the repository up by id and, on
success, calls `scheduler.mark_succeeded(repository_id, class, now)`, which clears
the refresh's `pending`/`debounce_deadline`. Because `repository_id` is reused
across a detach and a subsequent re-attach, a completion produced by a reindex that
was in flight before the detach can be applied to the freshly re-attached
lifecycle of the same id — clearing the startup refresh that the re-attach just
queued. The same missing-epoch gap also allows a second reindex to be spawned for
the id while the original blocking task is still running.

## Violated Invariant Or Contract

A completion event must only affect the scheduler state of the run that produced
it. The scheduler design intends at most one in-flight manifest_fast run per repo
and exactly one matching completion (`next_ready_refresh` gates on
`in_flight_manifest_fast.is_empty()`; `ready_at` returns `None` while a class is
active). Routing completions by `repository_id` alone violates this across a
detach/re-attach boundary.

## Oracle

- Scheduler concurrency contract: `crates/cli/src/watch/scheduler.rs` —
  `next_ready_refresh` / `ready_at` enforce one active manifest_fast run per repo;
  tests `scheduler_failure_schedules_retry_without_parallel_restart` and
  `scheduler_debounces_roots_and_serializes_execution` codify "no parallel restart".
- `remove_repository` clears `in_flight_manifest_fast` and the repo's watch state,
  re-opening the in-flight gate after a detach.

## Counterexample

Event order (single-threaded supervisor; the blocking reindex task outlives the
detach):
1. Repo R manifest_fast reindex is in flight (`spawn_blocking` running; R is in
   `in_flight_manifest_fast`).
2. Client detaches R → `LeaseReleased` → `scheduler.remove_repository(R)` clears
   in-flight and R's state.
3. Client re-attaches R → `LeaseAcquired` → `add_repository(R)` +
   `queue_startup_refresh_if_needed` sets `manifest_fast.pending = true`,
   `debounce_deadline = Some(now)`.
4. The original (stale) task finishes → `ReindexCompleted{R, ManifestFast, Ok}`.
   `handle_reindex_completed` finds R present again and calls
   `scheduler.mark_succeeded(R, ManifestFast)`, which (with `rerun_requested` false)
   sets `pending = false, debounce_deadline = None`, discarding the freshly queued
   startup refresh. R is left un-refreshed until the next file event.
   (Alternatively, between 3 and 4 the now-empty in-flight gate lets a second
   reindex spawn for R against the same db.)

## Why It Might Matter

A re-attached workspace can be left with a stale index until an unrelated file
event triggers another refresh — indefinitely for a quiet repository. The
alternative branch spawns a redundant concurrent reindex (which, under WAL with no
busy_timeout, fails with SQLITE_BUSY and schedules a spurious retry). Correctness
of the watch refresh lifecycle, narrow but reachable timing window.

## Proof

State-transition + control-flow mismatch:
- `crates/cli/src/watch/supervisor.rs:42-46` — `ReindexCompleted` has no epoch.
- `:497-501` — completion handler resolves the repository by `repository_id` only.
- `:508` — on `Ok`, `scheduler.mark_succeeded(repository_id, class, now)` with no
  run check; `mark_succeeded` clears `pending`/`debounce_deadline` when no rerun is
  requested (`scheduler.rs` `RefreshQueueState::mark_succeeded`).
- `remove_repository` (detach) clears `in_flight_manifest_fast`, and
  `add_repository` + `queue_startup_refresh_if_needed` (re-attach) create a fresh
  pending refresh — both keyed on the reused `repository_id`.

## Counterevidence Checked

- `remove_repository` clearing in-flight prevents a permanent stall in the
  detach-only case, but it is exactly what re-opens the gate for the re-attach race.
- SQLite WAL rejects a truly concurrent writer, so silent corruption is unlikely;
  realized harm is a dropped startup refresh and/or a spurious failed reindex.
- Single-threaded command loop serializes handling, but the `spawn_blocking`
  reindex outlives the detach command, so the stale completion is delivered after
  re-attach — the race is real, not prevented by serialization.

## Suggested Next Step

Attach a monotonically increasing per-repository run/epoch token to the dispatched
reindex and echo it in `ReindexCompleted`; in `handle_reindex_completed`, ignore a
completion whose epoch does not match the repository's current epoch.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-27: fixed. Confirmed `SupervisorCommand::ReindexCompleted` carried only `repository_id`, and `handle_reindex_completed` called `scheduler.mark_succeeded` purely by id — so a completion from a reindex dispatched before a detach could clear the refresh a re-attach of the same id had just queued. Implemented the suggested epoch token: added a `RepositoryEpochs` map in `run_supervisor` keyed by `repository_id`; `LeaseAcquired` calls `ensure` (preserves any carried-over epoch), `LeaseReleased` calls `bump` (so any in-flight run from the released lifecycle becomes stale), each dispatched reindex captures `repository_epochs.current(..)` and echoes it in `ReindexCompleted { epoch, .. }`, and the completion arm ignores (with a warn) any completion whose epoch != the repository's current epoch before invoking `handle_reindex_completed`. The boundary that previously dropped the refresh now survives. (The narrower duplicate-concurrent-reindex window the report also notes is left to SQLite WAL's single-writer rejection + the existing retry path, which is self-healing; the epoch token addresses the dropped-refresh correctness issue.) Regression tests `repository_epochs_invalidate_stale_completion_across_detach_reattach`, `repository_epochs_match_for_undisturbed_lifecycle`, and `repository_epochs_default_to_zero_for_unknown_repository` (supervisor.rs) codify the lifecycle. `cargo test watch::` green (23).

DEVANA-KEY: crates/cli/src/watch/supervisor.rs:485 | P3 | watch-reindex-completion-missing-epoch-detach-race
DEVANA-SUMMARY: Status=fixed | P3 medium crates/cli/src/watch/supervisor.rs:485 - Watch reindex completions are routed by repository_id with no run/epoch token, so a stale completion from a pre-detach reindex can clear a re-attached repository's freshly queued refresh (or allow a duplicate concurrent reindex), leaving the index stale.
