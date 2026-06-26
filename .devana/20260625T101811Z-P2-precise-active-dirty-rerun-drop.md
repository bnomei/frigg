DEVANA-FINDING: v1
Priority: P2 | Confidence: medium | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server/precise_graph/generation.rs:1415 | Slug: precise-active-dirty-rerun-drop

# Active Precise Generation Drops Dirty Reruns

## Finding

If a precise-generation task is already active, a later request with new changed or deleted paths returns `SkippedActiveTask` without recording that those paths need another generation pass.

## Violated Invariant Or Contract

The active-task guard may suppress duplicate execution only if the active task covers the new dirty work or the new work is queued for replay after completion.

## Oracle

Generator selection is based on `changed_paths` and `deleted_paths`, and runtime task tracking is supposed to coordinate background work without losing required repository work.

## Counterexample

A precise generation starts because an artifact is missing. While it is running, a reindex sees a source-file change and calls precise generation with that changed path. The active-task branch returns `SkippedActiveTask`. If the first generator read the old file before the change, the new change is never regenerated.

## Why It Might Matter

Precise graph artifacts can remain stale after a valid reindex reports that precise generation was merely active and later completed.

## Proof

Control-flow trace: `maybe_spawn_workspace_precise_generation` selects generators for the new path set, then checks active tasks at lines 1415-1425. If active, it logs the path counts and returns at line 1439. The completion thread caches only the original generation summaries and never consumes skipped dirty paths.

## Counterevidence Checked

The explicit reindex path invalidates caches before calling generation, but it does not queue the skipped precise request. Waiting for precise completion only waits for the already-running task.

## Suggested Next Step

Record pending dirty paths per repository when `SkippedActiveTask` is returned, and schedule a follow-up generation after the active task completes.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed. Confirmed the `SkippedActiveTask` branch dropped the new dirty paths, and (verified) `workspace_precise_generation_needed` with empty paths only regenerates when no artifact is cached — so a re-check after completion would NOT pick up a content change. Implemented the report's recommended approach: added a per-repository pending-dirty-paths store `precise_generation_pending_dirty_paths: Arc<RwLock<BTreeMap<repository_id, (changed, deleted)>>>` on `FriggMcpRuntimeState` (Arc-shared across sessions, single init site). On `SkippedActiveTask`, `record_pending_precise_dirty_paths` merges the new changed/deleted sets. The active task's completion thread, AFTER `finish_task` (so the active-task guard no longer sees it), calls `take_pending_precise_dirty_paths` and replays them through `maybe_spawn_workspace_precise_generation`. Ordering (drain after finish) ensures: paths arriving during the run are queued and replayed; paths arriving after finish spawn directly; if a newer task is already active at replay time, the paths are re-queued for it — no loss, no infinite loop (replay only happens when work is pending). Added regression test `precise_generation_records_dirty_paths_when_skipped_for_active_task` (registers an active PreciseGenerate task, asserts SkippedActiveTask records the changed+deleted paths and that the drain consumes them). New test + 20 precise_generation tests pass.

DEVANA-KEY: crates/cli/src/mcp/server/precise_graph/generation.rs:1415 | P2 | precise-active-dirty-rerun-drop
DEVANA-SUMMARY: Status=fixed | P2 medium crates/cli/src/mcp/server/precise_graph/generation.rs:1415 - Dirty paths skipped during an active precise generation are now recorded per repo and replayed by the active task's completion thread instead of being dropped (regression test added).
