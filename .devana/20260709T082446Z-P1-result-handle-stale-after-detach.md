DEVANA-FINDING: v1
DEVANA-STATE: fixed | P1 | high | security=no
DEVANA-KEY: crates/cli/src/mcp/server.rs:1398 | result-handle-stale-after-detach

# workspace_detach leaves session result_handles for the detached repository

## Finding

`workspace_detach` invalidates symbol-corpus, freshness, precise-generation, and precise-graph caches for the detached workspace but does **not** call `invalidate_session_result_handles_for_repository_ids`. Index-refresh paths do invalidate session handles. After detach, `session_result_handle_lookup` can still return `Found` for anchors whose `repository_id` is no longer adopted; `read_match` then fails with “repository_id is not adopted for this session” instead of the structured `STALE_HANDLE` recovery contract.

## Violated Invariant Or Contract

Session `result_handle` lifetime is documented as session-scoped and must not outlive adoption for its repository. Detach and index-refresh should both drop handles that can no longer be proof-read under the current session attachments.

## Oracle

`invalidate_workspace_index_runtime_caches` in `workspace_session.rs` calls `invalidate_session_result_handles_for_repository_ids`. Index refresh gate test `read_match_result_handle_is_invalidated_after_workspace_index_refresh` encodes the same expectation for refresh. Detach path in `server.rs` omits the equivalent call. `handle_expires: "session"` metadata promises session-scoped validity.

## Counterexample

1. Adopt repo R; run `search_text` and retain `result_handle` + `match_id`.
2. `workspace_detach` R (still within handle TTL and entry cap).
3. Call `read_match` with the retained handle.
4. Lookup returns `Found` (entry still present); subsequent adoption check fails with non-stale not-adopted error instead of `STALE_HANDLE` recovery.

## Why It Might Matter

Agents that follow recovery fields retry the wrong remediation (re-adopt vs re-search). Handles remain until TTL (300s) or FIFO eviction (64), prolonging mixed-handle diagnostics and protocol inconsistency after explicit detach.

## Proof

**Cross-entry mismatch:** index path clears handles (`workspace_session.rs` ~428–430); detach path does not (`server.rs` ~1398–1402).

**State transition mismatch:** session still holds anchors for a repository_id that is no longer adopted.

**Control-flow:** `session_result_handle_lookup` only prunes by TTL/missing entry, not by current adoption set.

## Counterevidence Checked

- Distinct from `result-handle-stale-after-index` (presentation/index invalidation path).
- Detach does clear pending precise dirty paths when last session/lease leaves; that is a different map.
- `read_match` re-checks adoption, so content is not served after detach — failure mode is wrong error/recovery class, not open exfiltration via handle alone.
- Strongest false-positive: “not-adopted is good enough.” Ruled out because tool contracts distinguish `STALE_HANDLE` vs missing adoption and attach `suggested_next` accordingly; agents branch on `error_code`.

## Suggested Next Step

Call `invalidate_session_result_handles_for_repository_ids(runtime_task_repository_aliases(&workspace))` on the detach success path (mirror index invalidation).

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-09: open by Devana. Initial report written from static source inspection during exhaustive `--all` hunt.
- 2026-07-09: fixed by invalidating session result_handles inside `detach_workspace` (mirrors index-refresh invalidation). Regression: `read_match_result_handle_is_invalidated_after_workspace_detach`.

DEVANA-KEY: crates/cli/src/mcp/server.rs:1398 | result-handle-stale-after-detach
DEVANA-SUMMARY: fixed | P1 | high | workspace_detach does not invalidate session result_handles, so read_match fails with not-adopted instead of STALE_HANDLE after detach.
