DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/workspace_registry.rs:34 | Slug: runtime-id-aliasing-concurrent-reindex

# Stable and runtime repository IDs split task-registry dedup, allowing concurrent reindexes on one DB

## Finding

Startup workspaces carry two IDs: a stable hash `repository_id` and a legacy `runtime_repository_id` (e.g. `repo-001`). MCP background tasks register under the stable id while watch registers under the runtime id. Only `repository_has_active_runtime_work` checks both aliases; watch supervisor and attach prewarm do not, so two full reindexes can run concurrently on the same SQLite file.

## Violated Invariant Or Contract

At most one in-flight index or semantic refresh per physical repository database. Active-task dedup must treat stable and runtime repository IDs as aliases for the same workspace.

## Oracle

`repository_has_active_runtime_work` (`workspace_session.rs:785-811`) explicitly resolves both `repository_id` and `runtime_repository_id` when checking the task registry. Watch and prewarm paths should observe the same union before spawning work.

## Counterexample

1. Server starts with configured roots; `from_startup_repositories` sets `repository_id = stable_hash` and `runtime_repository_id = "repo-001"`.
2. Watch acquires a lease; `WatchedRepository.repository_id == "repo-001"` (`watch/repository.rs:40-41`).
3. `workspace_attach` with defer triggers prewarm: `SemanticRefresh` registered under stable hash (`index_health.rs:869-871`).
4. Watch manifest-fast completes and queues semantic followup; watch starts another `SemanticRefresh` under `"repo-001"` (`supervisor.rs:352-354`).
5. Prewarm's `has_active_task_for_repository(..., &workspace.repository_id)` does not see the watch task; watch's guard at `supervisor.rs:334-337` checks `repository.repository_id` (runtime id) and does not see the prewarm task registered under the stable id.
6. Two `reindex_repository_with_runtime_config` calls run on the same `db_path`.

## Why It Might Matter

Concurrent full reindexes on one SQLite database can interleave manifest, projection, and semantic writes, producing transient inconsistency, duplicate embedding work, or hard-to-diagnose index corruption on default multi-repo startup configurations.

## Proof

**Contract mismatch**

- Producer: `WorkspaceRegistry::from_startup_repositories` stores distinct stable and runtime ids (`workspace_registry.rs:29-38`).
- Consumer (watch): `watched_repository_for_workspace` uses `runtime_repository_id`; tasks registered and checked with that id (`supervisor.rs:334-337`, `352-354`).
- Consumer (prewarm): registers and checks `SemanticRefresh` under `workspace.repository_id` (stable) (`index_health.rs:855-871`).
- `repository_has_active_runtime_work` resolves both ids but is used by `workspace_reindex` / attach gates, not by watch manifest-fast (no gate) or the watch semantic followup skip branch.

## Counterevidence Checked

`get_or_insert` sets both ids equal for dynamically attached repos, masking the bug in that path. Watch tests hardcode `repository_id == runtime_repository_id == "repo-001"`. Per-repo `active_class` in the watch scheduler serializes watch classes only, not MCP background tasks.

## Suggested Next Step

Centralize active-task checks through `repository_has_active_runtime_work` (or equivalent dual-id lookup) in watch supervisor before every spawn, and add a test with mismatched stable/runtime ids from `from_startup_repositories`.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed. Confirmed the divergence is real for startup repos: `config.repositories()` assigns `repository_id = legacy_repository_id_for_workspace_index` (`repo-001`, the runtime id), while `from_startup_repositories` recomputes `repository_id = stable_repository_id_for_root` (a hash). The watch SemanticFollowup guard (supervisor.rs:330) and prewarm guard (index_health.rs:849) each checked/registered `SemanticRefresh` under a single, different id, so neither saw the other → two reindexes on one db_path. Added `RuntimeTaskRegistry::has_active_task_for_any_repository(kind, &[ids])` and made both guards check the alias union: prewarm checks `[workspace.repository_id (stable), workspace.runtime_repository_id (legacy)]`; the watch guard checks `[repository.repository_id (runtime), stable_repository_id_for_root(root)]`. Unit test `has_active_task_for_any_repository_treats_stable_and_runtime_ids_as_aliases` covers the alias observation and confirms the old single-id lookup would have missed it. Existing scheduler/watch tests still pass. Note: a check→start_task TOCTOU window remains (pre-existing, spans two subsystems); this fix removes the systematic id-space blind spot that made the collision routine on default multi-repo startup.

DEVANA-KEY: crates/cli/src/mcp/workspace_registry.rs:34 | P1 | runtime-id-aliasing-concurrent-reindex
DEVANA-SUMMARY: Status=fixed | P1 high crates/cli/src/mcp/workspace_registry.rs:34 - Watch and prewarm SemanticRefresh dedup now check the stable+runtime id alias union via has_active_task_for_any_repository, so they observe each other and no longer run concurrent reindexes on one db (unit test added).