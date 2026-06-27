DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server/provenance.rs:258 | Slug: provenance-runtime-id-silent-skip

# Provenance persistence does not resolve runtime repository_id aliases

## Finding

MCP read/search/navigation tools resolve `repository_id` through
`workspace_by_any_repository_id`, accepting both stable hash ids and legacy
runtime ids (`repo-001`). `provenance_target_for_repository` looks up workspaces
by exact `repository_id` match only. When the provenance hint carries a legacy
runtime id, lookup fails and `record_provenance_with_outcome_internal` returns
`Ok(())` without persisting an event.

## Violated Invariant Or Contract

When a tool call succeeds with a valid `repository_id` alias, durable provenance
for that call should be recorded against the canonical workspace target.

## Oracle

Adoption gate (`workspace_registry.rs` ~55–65) matches stable or
`runtime_repository_id`. Provenance target (`provenance.rs` ~263–266) uses
`.find(|workspace| workspace.repository_id == repository_id)` only. Silent skip at
lines 363–367 on lookup failure.

## Counterexample

1. Client adopts workspace; MCP tools accept `repository_id=repo-001` (runtime id)
2. Client calls `search_hybrid` with `repository_id=repo-001`
3. Search executes successfully via alias resolution
4. Provenance hint resolves to `repo-001`
5. `provenance_target_for_repository(Some("repo-001"))` returns `None`
6. `record_provenance_with_outcome_internal` returns `Ok(())` — no event written

## Why It Might Matter

Workload export and provenance-backed diagnostics miss tool calls that used the
legacy runtime id, while the same calls succeed and return results to the client.

## Proof

**Dataflow trace:** runtime id hint → provenance exact-match lookup fails → silent
`Ok(())` skip → missing durable event.

**Cross-entry mismatch:** tool execution accepts alias; provenance target lookup
does not.

## Counterevidence Checked

- Stable hash ids in `repository_id` param work for provenance
- Multi-repo searches without hint use `default_provenance_target()` and can persist
- `dynamic-cli-reindex-repo-fork` (wontfix) is a broader identity partition issue;
  this is the provenance write path gap for an accepted runtime alias

## Suggested Next Step

Resolve provenance targets via `workspace_by_any_repository_id` (or map runtime id
to stable id before lookup).

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report from static inspection across
  `dataflow-boundaries` trail.
- 2026-06-27: fixed. Confirmed `provenance_target_for_repository` matched only
  `workspace.repository_id == repository_id` (stable hash) in both the
  attached-workspaces and known-workspaces lookups, while tool execution accepts the
  legacy runtime id (`repo-NNN`) via `workspace_by_any_repository_id`
  (matches `repository_id` OR `runtime_repository_id`). A call made with the runtime
  alias therefore resolved to `None` and silently skipped the provenance write. Fix:
  extended both find closures to also match `runtime_repository_id`, mirroring the
  adoption gate, while keeping the attached-first + known-bootstrap tiering (so the
  `known_workspace_can_bootstrap_provenance` gate is preserved rather than swapping
  to the registry-wide helper). The returned target still uses the canonical
  `workspace.repository_id`, so events are recorded against the stable workspace.
  Added regression test `provenance_persists_for_runtime_repository_id_alias`
  (asserts the runtime id differs from the stable id, then that a read_file via
  `repo-001` persists a provenance row). provenance suite green (9 tests).

DEVANA-KEY: crates/cli/src/mcp/server/provenance.rs:258 | P2 | provenance-runtime-id-silent-skip
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/mcp/server/provenance.rs:258 - Provenance target lookup matched only the stable repository_id while tools accept runtime id aliases, silently skipping provenance for alias calls; fixed by also matching runtime_repository_id in both lookup tiers plus a regression test.