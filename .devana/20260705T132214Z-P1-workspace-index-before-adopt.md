DEVANA-FINDING: v1
DEVANA-STATE: wontfix | P1 | high | security=no
DEVANA-KEY: crates/cli/src/mcp/server.rs:1423 | workspace-index-before-adopt

# workspace_index mutates storage before session adoption

## Finding

The MCP `workspace_index` tool runs a full `index_repository_with_runtime_config` pass and invalidates caches before calling `adopt_workspace`. If adoption fails afterward (watch lease failure, registry conflict, etc.), `.frigg/storage.sqlite3` and manifest state are already mutated while the session remains non-adopted.

## Violated Invariant Or Contract

Session adoption should gate or follow index side effects so on-disk index state and MCP session ownership stay consistent. `workspace_attach` and `workspace_prepare` adopt before or alongside activation; `workspace_index` inverts that order.

## Oracle

`workspace_attach` path in `workspace_session.rs` adopts then indexes. `workspace_prepare` initializes storage then adopts (`server.rs:1265`). `runtime_gate_tests/workspace.rs` exercises adopt failure after attach; `workspace_index` lacks the symmetric ordering guard.

## Counterexample

1. HTTP MCP session calls `workspace_index` on a valid authorized root with `confirm: true`.
2. Blocking index completes successfully, writing manifest/semantic rows and calling `invalidate_workspace_index_runtime_caches` (`server.rs:1448`).
3. `adopt_workspace` fails (e.g. watch lease cannot be acquired).
4. Session has no adopted repository for that root, but disk index reflects the completed refresh.

## Why It Might Matter

Agents can believe indexing failed when storage already changed, or operate on stale session state while SQLite holds a new snapshot. Follow-up tools scoped to adopted repos will not see the indexed repository even though `.frigg/` changed.

## Proof

Control-flow trace: `workspace_index` handler (`server.rs:1423-1467`) → `index_repository_with_runtime_config` + cache invalidation → `adopt_workspace` only at line 1467 on success path.

## Counterevidence Checked

- Ephemeral registry entries are pruned on guard drop after failure, but SQLite writes are not rolled back.
- `workspace_attach` and `workspace_prepare` use adopt-first or adopt-before-finalize ordering.

## Suggested Next Step

Reorder `workspace_index` to adopt (or acquire lease) before indexing, or roll back / mark index output tentative when `adopt_workspace` fails.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-05: open by Devana. Initial report written from static source inspection.
- 2026-07-05: wontfix by user decision; no code change made. The report remains as a durable record of the accepted workspace_index lifecycle consistency risk.

DEVANA-KEY: crates/cli/src/mcp/server.rs:1423 | workspace-index-before-adopt
DEVANA-SUMMARY: wontfix | P1 | high | workspace_index writes and invalidates the on-disk index before adopt_workspace; this lifecycle consistency risk is accepted.
