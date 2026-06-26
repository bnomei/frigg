DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server/workspace_session.rs:720 | Slug: defer-semantic-recovery-drop

# Defer Attach Drops Missing-Model Semantic Recovery

## Finding

`workspace_attach(index_mode=defer)` reports that recovery work was queued, but the prewarm spawner ignores semantic refresh plans whose reason is `semantic_snapshot_missing_for_active_model`.

## Violated Invariant Or Contract

Defer mode should start available recovery work for stale or missing semantic state, not only report a queued action.

## Oracle

`workspace_semantic_refresh_plan` explicitly returns a plan for `MissingForActiveModel`, and freshness tests encode that state as refresh-needed.

## Counterexample

A repository has a valid manifest snapshot, semantic runtime enabled, and no semantic rows for the active provider/model after enabling semantics or changing models. In stdio/default watch-off mode, `workspace_attach` with `index_mode=defer` returns `Queued`, but no semantic refresh task is started.

## Why It Might Matter

Clients can be told that semantic recovery is queued while semantic search remains unavailable until a stronger ensure/reindex path is used.

## Proof

Caller/callee mismatch: defer mode calls `maybe_spawn_workspace_runtime_prewarm` and records `WorkspaceIndexAction::Queued` at `crates/cli/src/mcp/server/workspace_session.rs:720`. The plan builder returns `semantic_snapshot_missing_for_active_model` at `crates/cli/src/mcp/server/runtime_status/index_health.rs:760`. The spawner sets `should_refresh_semantic` only when the reason is `stale_manifest_snapshot` at lines 838-841, so it returns without starting work.

## Counterevidence Checked

`index_mode=ensure` runs full attach refresh, and `index_mode=skip` intentionally does no work. Watch startup can queue semantic follow-up when watch is active. This issue is the attach defer prewarm path, distinct from the existing watch semantic followup drop.

## Suggested Next Step

Treat any concrete `WorkspaceSemanticRefreshPlan` as refresh work, or explicitly map each refresh reason to a task and response action.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed. Confirmed `workspace_semantic_refresh_plan` returns plans for two reasons (`stale_manifest_snapshot` and `semantic_snapshot_missing_for_active_model`) and `refresh_workspace_semantic_snapshot_with_plan` runs a `ReindexMode::Full` reindex identically for both, yet `maybe_spawn_workspace_runtime_prewarm` gated `should_refresh_semantic` on `reason == "stale_manifest_snapshot"`, so a missing-active-model plan was built but never spawned while defer attach still reported `Queued`. Changed the gate to `semantic_plan.is_some()` (any concrete plan is actionable). Extended the existing `semantic_refresh_plan_detects_latest_snapshot_missing_active_model` test to call `maybe_spawn_workspace_runtime_prewarm` and assert a `SemanticRefresh` task with phase `semantic_attach_refresh` is registered (start_task runs synchronously before the worker thread). Passes; would fail pre-fix.

DEVANA-KEY: crates/cli/src/mcp/server/workspace_session.rs:720 | P2 | defer-semantic-recovery-drop
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/mcp/server/workspace_session.rs:720 - Defer attach prewarm now spawns semantic recovery for any concrete refresh plan, including semantic_snapshot_missing_for_active_model, so the reported Queued action actually starts work (regression test added).
