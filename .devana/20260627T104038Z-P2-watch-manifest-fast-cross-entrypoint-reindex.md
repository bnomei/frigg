DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/watch/supervisor.rs:381 | Slug: watch-manifest-fast-cross-entrypoint-reindex

# Watch ManifestFast dispatch ignores MCP-active reindex tasks

## Finding

MCP attach-ensure, `workspace_prepare`, and `workspace_reindex` call
`repository_has_active_runtime_work` before spawning background reindexes, checking
`ChangedReindex`, `WorkspaceReindex`, `WorkspacePrepare`, and `SemanticRefresh`
under stable and runtime repository id aliases. Watch `ManifestFast` dispatch does
not call this guard; it only dedupes scheduler `in_flight_manifest_fast` and guards
`SemanticFollowup` against active `SemanticRefresh`. A watch manifest-fast reindex
can therefore start while an MCP `WorkspaceReindex` is already writing the same
`db_path`, and vice versa is only partially protected (MCP blocks on watch
`ChangedReindex` but not the reverse).

## Violated Invariant Or Contract

At most one in-flight manifest/index refresh per physical repository database
across watch supervisor and MCP background tasks.

## Oracle

`repository_has_active_runtime_work` (`workspace_session.rs` ~791–817) is the
MCP-side cross-task dedup gate. Watch `SemanticFollowup` uses
`has_active_task_for_any_repository` for semantic dedup (~401–425) but `ManifestFast`
has no analogous registry check before `spawn_blocking` (~457–504).

## Counterexample

1. MCP client calls `workspace_reindex` → `RuntimeTaskKind::WorkspaceReindex`
   registered; `reindex_repository_with_runtime_config` runs on `db_path`.
2. Filesystem notify fires → watch supervisor selects `WatchRefreshClass::ManifestFast`.
3. Scheduler `in_flight_manifest_fast` is empty; no registry check for
   `WorkspaceReindex`.
4. Watch starts `RuntimeTaskKind::ChangedReindex` on the same `db_path` while step
   1 is still in `spawn_blocking`.

## Why It Might Matter

Duplicate reindex work, `SQLITE_BUSY` failures, or interleaved manifest/projection
phases can leave transient inconsistency or failed refreshes without a clear owner.

## Proof

**Cross-entry mismatch:** MCP paths consult `repository_has_active_runtime_work`;
watch `ManifestFast` does not.

**Control-flow trace:** `next_ready_refresh` → `mark_started` → `spawn_blocking`
without registry guard for `WorkspaceReindex` / `WorkspacePrepare`.

Distinct from the open detach-race report (scheduler in-flight cleared on detach);
this path does not require detach.

## Counterevidence Checked

- SQLite WAL single-writer may surface `SQLITE_BUSY` instead of silent corruption;
  harm is duplicate work and refresh failure rather than proven data loss.
- `SemanticFollowup` dedup against `SemanticRefresh` does not cover manifest-fast
  vs workspace-reindex interaction.
- Scheduler `active_class` serializes consecutive watch jobs per repo until
  completion but does not observe MCP tasks.
- `dynamic-cli-reindex-repo-fork` (wontfix) is CLI-only; this report is MCP↔watch
  coordination.

## Suggested Next Step

Before watch `ManifestFast` dispatch, call `has_active_task_for_any_repository`
for `{ChangedReindex, WorkspaceReindex, WorkspacePrepare}` using stable+runtime
id aliases, mirroring the semantic followup guard.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report written from static source inspection
  across `outside-in-entrypoints`, `state-lifecycle`, and `cache-persistence` trails.
- 2026-06-27: fixed. Confirmed `active_refresh_task_running_for_repository` only
  consulted `watch_task_kind_for_class(class)` — a single kind (ChangedReindex for
  ManifestFast, SemanticRefresh for SemanticFollowup) — so the watch dispatch never
  saw the MCP-owned `WorkspaceReindex`/`WorkspacePrepare` tasks that write the same
  `db_path`. Added `conflicting_task_kinds_for_class`: ManifestFast now defers to
  {ChangedReindex, WorkspaceReindex, WorkspacePrepare} and SemanticFollowup to
  {SemanticRefresh, WorkspaceReindex, WorkspacePrepare}, checked under both the
  stable and runtime id aliases via `has_active_task_for_any_repository` (mirroring
  the MCP-side `repository_has_active_runtime_work` gate). When the guard trips, the
  existing dispatch path keeps the refresh pending with backoff
  (`scheduler.mark_failed` + `continue`), so the work retries once the MCP task
  finishes rather than starting a concurrent writer. `watch_task_kind_for_class`
  retained for the `start_task` label of the task being dispatched. Existing alias
  tests still pass; added `manifest_fast_defers_to_active_mcp_workspace_reindex` and
  `manifest_fast_defers_to_active_mcp_workspace_prepare`. Full `watch::supervisor`
  suite green (9 tests).

DEVANA-KEY: crates/cli/src/watch/supervisor.rs:381 | P2 | watch-manifest-fast-cross-entrypoint-reindex
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/watch/supervisor.rs:381 - Watch ManifestFast/SemanticFollowup dispatch only deduped against its own watch task kind and missed MCP WorkspaceReindex/WorkspacePrepare writers on the same db_path; fixed by deferring both watch classes to those cross-entrypoint kinds (with backoff retry) plus regression tests.