DEVANA-FINDING: v1
DEVANA-STATE: fixed | P1 | high | security=no
DEVANA-KEY: crates/cli/src/mcp/server/search_tools/batch.rs:298 | search-batch-nested-handle-pollution

# search_batch nested full search tools mint phantom result_handles

## Finding

Each `search_batch` probe calls the full public `search_text_impl` / `search_symbol_impl` / `search_hybrid_impl` path. Those implementations always run presentation that allocates session `result_handle`s. Batch discards nested `match_id`s (`match_id: None` when mapping rows) and only returns a final `batch:*` handle, but intermediate probe handles remain in the session cache (up to N per batch + 1 final). With `SESSION_RESULT_HANDLE_MAX_ENTRIES = 64`, a few multi-probe batches silently evict earlier client-held handles, causing false `STALE_HANDLE` on `read_match`. Nested tools also finalize under child tool names, so routing stats attribute fan-out to `search_text`/`search_symbol`/`search_hybrid` while `search_batch` may never appear.

## Violated Invariant Or Contract

Session handle entries must correspond to handles returned to the client (or be cleaned up). A composed tool must not thrash the shared handle budget with invisible intermediates. Tool-call accounting for Futura routing should attribute the agent-visible tool (`search_batch`), not only nested internals.

## Oracle

Sibling composer `impact_bundle` surfaces nested handles and finalizes as itself. Handle store is session-global with hard cap 64 and TTL 300s. Batch design discards nested match_ids, proving intermediates are not part of the batch contract.

## Counterexample

1. Agent holds `result_handle` from prior `search_text` (within TTL).
2. Run several 8-probe `search_batch` calls (each inserts ~8 intermediate handles + 1 batch handle under concurrent join).
3. FIFO prune drops the earlier handle.
4. `read_match` on the retained client handle returns `STALE_HANDLE` despite no detach/index and within claimed session expiry.

## Why It Might Matter

Breaks proof-read workflows after legitimate batch use; corrupts routing stats used for Futura posture. High-confidence correctness/availability defect on the new concurrent batch path.

## Proof

**Dataflow:** batch probe → full `search_*_impl` → `assign_result_handle_for_*` → session cache insert → batch maps matches with `match_id: None` → second `assign_result_handle_for_batch_matches` → client only sees final handle.

**Control-flow:** concurrent `tokio::join!` tree still fully runs both halves; one probe `Err` aborts batch after siblings already stored handles.

## Counterevidence Checked

- Distinct from `result-handle-stale-after-index` (index invalidation).
- Distinct from detach missing invalidation (separate report).
- Strongest false-positive: “orphan handles are harmless until TTL.” Ruled out by hard max 64 with insertion-order eviction of live client handles.
- Impact_bundle intentionally returns nested handles; batch does not.

## Suggested Next Step

Internal search cores that skip present/handle mint for batch, or strip nested handles immediately after each probe; finalize once as `search_batch`. Optionally record one zero-hit/recovery at batch level only.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-09: open by Devana. Initial report written from static source inspection during exhaustive `--all` hunt.
- 2026-07-09: fixed by dropping nested probe result_handles after each search_batch probe completes, so only the final batch handle remains in the session cache. Test: `drop_session_result_handle_removes_entry`.

DEVANA-KEY: crates/cli/src/mcp/server/search_tools/batch.rs:298 | search-batch-nested-handle-pollution
DEVANA-SUMMARY: fixed | P1 | high | search_batch nested full search tools mint invisible session result_handles that thrash the 64-entry cache and mis-attribute routing stats.
