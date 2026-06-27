DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/watch/scheduler.rs:144 | Slug: watch-semantic-retry-cleared-by-path-event

# Filesystem events clear scheduled semantic-followup retries

## Finding

When `SemanticFollowup` fails, `mark_failed` schedules an independent retry via
`retry_deadline`. Any subsequent filesystem `record_event` resets
`semantic_followup` to default whenever `active_class !=
SemanticFollowup`, wiping the pending retry. The same event arms manifest-fast
debounce. If the subsequent manifest-fast refresh fails, semantic follow-up is
never re-queued because `queue_semantic_followup_if_needed` runs only on manifest
success.

## Violated Invariant Or Contract

A failed semantic-followup retry scheduled by `mark_failed` must survive unrelated
path-change notifications until its retry deadline or until semantic work is
explicitly re-queued through a valid path.

## Oracle

`mark_failed` sets `pending=true` and `retry_deadline=now+retry`
(`scheduler.rs` ~95–100). `record_event` line 144–146 assigns
`RefreshQueueState::default()` to `semantic_followup` when not actively running
semantic follow-up.

## Counterexample

1. T0: `SemanticFollowup` fails (embedding API error) → `mark_failed` →
   `retry_deadline=T0+5s`
2. T0+ε: filesystem notify → `record_event` → semantic retry wiped, manifest-fast
   armed
3. T0+5s: no semantic work scheduled (retry was cleared at step 2)
4. T0+ε+750ms: manifest-fast runs but fails (e.g. `SQLITE_BUSY` from concurrent
   reindex)
5. Steady state: semantic head stays stale; no independent semantic retry until
   manifest eventually succeeds and re-runs `queue_semantic_followup_if_needed`

## Why It Might Matter

Semantic recovery can be indefinitely delayed when manifest refreshes fail after a
path event clears the semantic retry, even when the original semantic failure was
unrelated to manifest state.

## Proof

**State transition trace:** failed semantic → retry scheduled → `record_event` →
`semantic_followup=default()` → retry lost → manifest failure blocks re-queue.

Distinct from fixed semantic-followup deferral (`mark_succeeded` on active
SemanticRefresh) and from open detach-race reports.

## Counterevidence Checked

- `record_event` preserves manifest-fast retry when `retry_deadline` is set and
  class is not ManifestFast (lines 135–138); semantic follow-up has no equivalent
  guard
- Successful manifest-fast eventually calls `queue_semantic_followup_if_needed`,
  mitigating when manifest succeeds promptly
- Startup `enqueue_initial_sync` only runs on lease acquire, not on this path

## Suggested Next Step

Do not reset `semantic_followup` in `record_event` when `retry_deadline.is_some()`,
or re-enqueue semantic follow-up on manifest failure when freshness still requires it.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report from static inspection across
  `state-lifecycle` trail.
- 2026-06-27: fixed. Confirmed `RepositoryWatchState::record_event` (scheduler.rs
  ~144) unconditionally reset `semantic_followup` to `RefreshQueueState::default()`
  whenever `active_class != SemanticFollowup`, wiping a retry that `mark_failed` had
  scheduled (pending + retry_deadline) — while `manifest_fast` has a retry-preserving
  guard just above (lines 135-138). Fix mirrors that guard: only reset
  `semantic_followup` when `retry_deadline.is_none()`. A pending-but-not-failed
  follow-up is still reset (a fresh manifest-fast supersedes it with re-queued
  semantic work on success), but a failed follow-up's independent retry — usually
  recovering from an embedding error unrelated to the path change — now survives.
  Added tests `scheduler_path_change_preserves_failed_semantic_followup_retry`
  (failed retry survives an unrelated path event) and
  `scheduler_path_change_supersedes_non_failed_semantic_followup` (boundary: a
  non-failed pending follow-up is still reset). watch lib suite green (32 tests).

DEVANA-KEY: crates/cli/src/watch/scheduler.rs:144 | P2 | watch-semantic-retry-cleared-by-path-event
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/watch/scheduler.rs:144 - record_event reset semantic_followup on every path change, dropping a mark_failed retry; fixed by preserving semantic_followup when retry_deadline is set (mirroring the manifest-fast guard) plus regression tests.