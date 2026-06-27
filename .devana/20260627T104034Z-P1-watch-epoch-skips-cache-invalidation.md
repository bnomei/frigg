DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/watch/supervisor.rs:346 | Slug: watch-epoch-skips-cache-invalidation

# Stale watch reindex completions skip MCP cache invalidation after detach

## Finding

When a watch reindex finishes after `workspace_detach` bumps the repository epoch,
the supervisor correctly ignores the stale `ReindexCompleted` command so scheduler
state is not falsely advanced. However, the `spawn_blocking` task has already
committed manifest and projection updates to SQLite and only invalidates
`validated_manifest_candidate_cache`. The full
`repository_cache_invalidation_callback` (projection store, search/navigation
response caches, symbol corpus, file content caches) runs only inside
`handle_reindex_completed`, which the epoch gate skips. Re-attach can then see a
fresh DB snapshot while MCP search and navigation caches still serve pre-reindex
data.

## Violated Invariant Or Contract

A successful watch reindex must invalidate all repository-scoped MCP/search caches,
not only the validated-manifest candidate cache, regardless of whether the
completion epoch matches the current lifecycle epoch.

## Oracle

`repository_cache_invalidation_callback` (`workspace_session.rs` ~19–54) is the
documented invalidation path for watch-success completions
(`handle_reindex_completed` ~599–600). `workspace_attach` does not flush search or
navigation caches. `queue_startup_refresh_if_needed` can skip a new refresh when
`startup_refresh_status` reports manifest `Ready` after the zombie run already
wrote matching index state.

## Counterexample

1. Session adopts repo; watch dispatches `ManifestFast` at epoch N.
2. Client calls `workspace_detach` → `LeaseReleased` bumps epoch to N+1.
3. Pre-detach `spawn_blocking` reindex completes successfully, writes SQLite,
   invalidates only `validated_manifest_candidate_cache`, sends
   `ReindexCompleted { epoch: N, result: Ok(...) }`.
4. Supervisor sees `epoch != current_epoch` and skips `handle_reindex_completed`.
5. Client re-attaches; startup refresh sees manifest already `Ready` → no new
   refresh, no cache invalidation.
6. `search_hybrid` / `search_text` return stale ranked results from warm caches.

## Why It Might Matter

Users can see search and navigation answers that disagree with the on-disk index
after detach/re-attach during an in-flight watch refresh, without any error signal.

## Proof

**Control-flow trace:** `spawn_blocking` success (~488–492) → partial cache
invalidation only → `ReindexCompleted` → epoch gate (~347–359) → skip
`handle_reindex_completed` → skip `repository_cache_invalidation_callback`.

**State transition mismatch:** DB/index state advances to post-reindex; MCP cache
state remains pre-reindex.

Distinct from the open detach-race report (concurrent reindex dispatch); this
finding is the stale-success cache coherency gap left by epoch gating.

## Counterevidence Checked

- Epoch gating intentionally prevents false `mark_succeeded` on stale completions;
  that does not invalidate the separate cache-coherency gap.
- `workspace_detach` invalidates a subset of caches (`server.rs` ~888–894) but not
  projection store or search/navigation response caches.
- Watch tests cover epoch stale-completion logging but not post-detach cache
  staleness after zombie success.

## Suggested Next Step

On stale epoch completions where `result` is `Ok`, still run cache invalidation
(or invalidate inside `spawn_blocking` success path unconditionally). Add a
detach-during-reindex regression asserting search caches refresh after re-attach.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report written from static source inspection
  across `inside-out-paths`, `state-lifecycle`, and `cache-persistence` trails.
- 2026-06-27: fixed. Confirmed the epoch gate (`ReindexCompleted` handler) skipped
  `handle_reindex_completed` — and thus `repository_cache_invalidation_callback` —
  for stale completions, while the `spawn_blocking` success path had already
  committed SQLite and only flushed `validated_manifest_candidate_cache`. Added a
  testable helper `invalidate_caches_for_stale_completion` that runs the full
  invalidation callback for stale completions when `result.is_ok()` (the run that
  actually mutated the index), while deliberately leaving scheduler state untouched
  (no false `mark_succeeded` on the superseded lifecycle). The callback is keyed by
  `repository_id` and resolves the canonical id from the registry, so invoking it
  after detach is safe. Added unit tests
  `stale_successful_completion_invalidates_repository_caches` and
  `stale_failed_completion_does_not_invalidate_repository_caches`. Full
  `watch::supervisor` suite green.

DEVANA-KEY: crates/cli/src/watch/supervisor.rs:346 | P1 | watch-epoch-skips-cache-invalidation
DEVANA-SUMMARY: Status=fixed | P1 high crates/cli/src/watch/supervisor.rs:346 - Epoch-gated stale watch completions skipped repository_cache_invalidation_callback while zombie reindexes still mutated SQLite; fixed by invalidating repository caches on stale Ok completions (scheduler state untouched) plus regression tests.