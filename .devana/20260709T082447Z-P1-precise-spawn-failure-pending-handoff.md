DEVANA-FINDING: v1
DEVANA-STATE: fixed | P1 | high | security=no
DEVANA-KEY: crates/cli/src/mcp/server/precise_graph/generation.rs:1788 | precise-spawn-failure-pending-handoff

# Precise generation spawn failure abandons dirty-path handoff

## Finding

When `maybe_spawn_workspace_precise_generation` successfully admits a `PreciseGenerate` task but `spawn_precise_generation_thread` fails, the failure path finishes the task as `Failed` and returns `WorkspacePreciseGenerationAction::Failed` without draining or re-recording `precise_generation_pending_dirty_paths`. The successful-worker finish path **does** take pending dirty paths and may respawn. Concurrent callers that recorded pending paths while the failed task was active therefore strand those paths with no active generator; the failed call’s own `changed_paths`/`deleted_paths` are also dropped without pending re-queue.

## Violated Invariant Or Contract

Once a `PreciseGenerate` task is admitted, dirty-path handoff must either run generation for those paths or re-queue them into `precise_generation_pending_dirty_paths` for at-least-once drain on terminal finish — the same contract as the successful-worker path and the fixed pending lost-update handoff.

## Oracle

Successful finish path takes pending under the pending write lock and may call `maybe_spawn_workspace_precise_generation` again (~1750–1775). Prior finding `precise-spawn-failure-triggered` fixed status reporting only (`Failed` vs `Triggered`), not pending cleanup. Handoff tests cover successful finish races, not spawn-failure cleanup.

## Counterexample

**A — concurrent pending stranded**

1. Call A: `try_start` succeeds; registry has active `PreciseGenerate`.
2. Call B (watch/index dirty paths): `SkippedActiveTask` → `record_pending_precise_dirty_paths_locked`.
3. Call A: thread spawn fails → finish Failed → return without `take_pending` or re-queue.
4. Pending map still holds B’s paths; nothing drains them until an unrelated later generation finishes successfully or last detach clears the map.

**B — triggering paths dropped**

1. Spawn is called with non-empty dirty paths that match a generator.
2. Task starts; thread spawn fails.
3. Paths are not recorded as pending; generation never runs and is not automatically retried.

## Why It Might Matter

Silent precise-graph staleness after rare thread-pool/spawn failure: navigation remains heuristic or outdated SCIP until a later unrelated event re-supplies dirty paths. High-confidence lifecycle defect with correctness impact.

## Proof

**State transition mismatch:** admitted task → Failed finish without pending drain vs Succeeded/Failed worker finish that drains pending.

**Control-flow:** spawn `Err` branch (~1788–1813) only finishes task and returns; no `take_pending_precise_dirty_paths_locked`.

## Counterevidence Checked

- `precise-spawn-failure-triggered` is a different mechanism (status label) at nearby lines; already fixed.
- `precise-pending-lost-update-race` / `precise-pending-after-detach` cover successful finish races and detach teardown, not spawn-failure cleanup.
- Spawn failure is rare on multi-thread runtimes but reachable under thread-pool exhaustion / OS limits; the defect is still source-visible and unconditional on that branch.
- Strongest false-positive: “Failed status is enough; callers re-trigger.” Ruled out: concurrent pending is only stored in the pending map, and cold-start empty-path logic does not recover path-triggered dirty sets.

## Suggested Next Step

On spawn failure, under the pending dirty-paths lock: re-record the call’s own changed/deleted paths and take+respawn any pending set (or leave a single pending entry and schedule a best-effort retry), matching successful-finish handoff semantics.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-09: open by Devana. Initial report written from static source inspection during exhaustive `--all` hunt.
- 2026-07-09: fixed by re-queuing the call's dirty paths into precise_generation_pending_dirty_paths on thread spawn failure so later successful generations can drain them. Extended `precise_generation_spawn_failure_returns_failed_and_preserves_detail`.

DEVANA-KEY: crates/cli/src/mcp/server/precise_graph/generation.rs:1788 | precise-spawn-failure-pending-handoff
DEVANA-SUMMARY: fixed | P1 | high | Precise generation spawn-failure path finishes the task without draining or re-queuing pending dirty paths, stranding precise regeneration work.
