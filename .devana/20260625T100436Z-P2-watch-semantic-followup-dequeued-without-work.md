DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/watch/supervisor.rs:339 | Slug: watch-semantic-followup-dequeued-without-work

# Watch semantic followup marked succeeded when another SemanticRefresh is active

## Finding

When a scheduled `SemanticFollowup` is ready but `has_active_task_for_repository(SemanticRefresh, …)` is true, the supervisor calls `mark_succeeded` without spawning work or re-enqueueing. `mark_started` has already cleared `semantic_followup.pending`, so the followup is permanently dropped if the other task fails or targeted a stale plan.

## Violated Invariant Or Contract

After `ManifestFast` commits a newer manifest snapshot, semantic head must cover that snapshot before freshness is satisfied. A dequeued `SemanticFollowup` must run, fail with retry, or be re-queued—not succeed without work.

## Oracle

`startup_refresh_status` / `repository_freshness_status`: manifest `Ready` with semantic `MissingForActiveModel` or stale `covered_snapshot_id` implies followup work is still required.

## Counterexample

1. Manifest-fast watch refresh completes; `queue_semantic_followup_if_needed` enqueues `SemanticFollowup`.
2. MCP attach prewarm already runs `SemanticRefresh` for the same repository (possibly under a different id alias; see separate runtime-id report).
3. Scheduler picks followup: `mark_started` clears pending.
4. Active-task guard at `supervisor.rs:330-340` fires → `mark_succeeded` without spawn.
5. In-flight refresh completes against an older plan or fails; semantic head lags manifest with no pending followup.

## Why It Might Matter

Semantic search and hybrid retrieval silently miss files from the latest manifest until another manifest-changing event re-triggers followup scheduling.

## Proof

**State transition mismatch**

`pending → mark_started (pending=false) → mark_succeeded` on skip branch without spawn or `enqueue_semantic_followup`.

**Control-flow trace**

`enqueue_semantic_followup` refuses re-entry while pending (`scheduler.rs:156-162`); skipped followup clears pending via `mark_succeeded`; no hook re-queues after external task completion.

## Counterevidence Checked

If the external `SemanticRefresh` succeeds for the same snapshot, freshness may be OK by accident. `queue_semantic_followup_if_needed` runs on `ManifestFast` completion only, not after skipped followup. Distinct from epoch/detach completion race at `supervisor.rs:485` (existing P3 report).

## Suggested Next Step

Defer followup (`mark_failed` with backoff) or await external task completion and re-check freshness before marking succeeded.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed. Confirmed the SemanticFollowup active-task guard called `mark_succeeded`, which clears `semantic_followup.pending`, so a followup deferred because another SemanticRefresh was in flight was dropped permanently — if that refresh failed or committed an older plan, the semantic head lagged the manifest with nothing re-queued (`enqueue_semantic_followup` runs only on ManifestFast completion). Changed the skip branch to `scheduler.mark_failed(&repository_id, class, now, retry)` (using the loop's `watch_config.retry_ms`-derived `retry`), which keeps the followup pending with a backoff deadline so it re-checks after the in-flight refresh finishes and runs if freshness is still unsatisfied. This composes with the earlier reorder (guard now runs before `mark_started`, so no `recent_paths`/pending state is consumed on defer) and the alias-aware dedup. Full `watch::` suite (20 tests) passes. (No new deterministic test: the path lives in the spawned supervisor select-loop with real timers; behavior verified by the existing suite + the scheduler `mark_failed` semantics that keep `pending=true`.)

DEVANA-KEY: crates/cli/src/watch/supervisor.rs:339 | P2 | watch-semantic-followup-dequeued-without-work
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/watch/supervisor.rs:339 - Deferred SemanticFollowup (another SemanticRefresh active) now backs off via mark_failed instead of being dropped by mark_succeeded, so it retries until the semantic head catches up to the manifest.