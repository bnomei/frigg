DEVANA-FINDING: v1
DEVANA-STATE: fixed | P1 | high | security=no
DEVANA-KEY: crates/cli/src/mcp/server/search_tools/batch.rs:409 | search-batch-unapplied-scope-echo

# search_batch echoes unapplied probe filters as zero-hit applied scope

## Finding

Hybrid probes accept `path_regex` / `glob` / `path_class` on `SearchBatchProbe`, but the hybrid arm builds `SearchHybridParams` without those filters (hybrid API has no path filters). On zero hits, probe summary sets `scope: body.recovery.scope.or_else(|| probe_scope(probe))`, and `probe_scope` copies the unused request filters into `ZeroHitScope`. Agents therefore see `scope.path_regex` (etc.) as applied when the search was whole-repo.

Sibling: text probes map path_regex/glob but not `path_class` (`..Default::default()` on `SearchTextParams`), yet `probe_scope` still echoes `path_class` as applied scope.

## Violated Invariant Or Contract

FUT-006 zero-hit scope echo must report **applied** search scope, not raw request fields that the execution path ignored. Misreported scope causes false `ScopeExcludedAllCandidates` / “broaden path_regex” remediation.

## Oracle

`SearchHybridParams` has no path_regex/glob/path_class fields. Text/symbol arms wire filters into their params; hybrid does not. Recovery/docs treat `scope` as applied filters for zero-hit diagnostics.

## Counterexample

1. `search_batch` with a hybrid probe: `query="foo"`, `path_regex="^src/"`.
2. Hybrid runs unscoped; repo has matches only outside `src/` → zero or misleading miss depending on content; when zero, summary scope still shows `path_regex: "^src/"`.
3. Agent broadens path_regex while the real miss was unscoped hybrid discovery.

## Why It Might Matter

Structured zero-hit contract lies; multi-hypothesis agents waste turns and may abandon correct unscoped results. High-confidence dataflow/contract bug on a new Futura surface.

## Proof

**Dataflow:** probe filters → hybrid params omit filters → unscoped search → `probe_scope` reattaches filters as `scope` → MCP zero-hit diagnostics.

**Contract mismatch:** request-echo presented as applied scope.

## Counterevidence Checked

- Not the same as hybrid witness/guardrail ranking issues already reported.
- Text+path_regex is applied; the lie is hybrid (and text path_class-only).
- Strongest false-positive: “schema allows filters so hybrid should ignore them silently without scope.” Scope echo is the bug even if silent ignore of unsupported filters is intentional — either validate+reject unsupported filters or kind-aware `probe_scope`.

## Suggested Next Step

Kind-aware `probe_scope` (hybrid: repository only; text: path_regex/glob/repo; symbol: path_regex/path_class/repo), or reject unsupported filters at batch validation. Prefer body-applied scope only.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-09: open by Devana. Initial report written from static source inspection during exhaustive `--all` hunt.

DEVANA-KEY: crates/cli/src/mcp/server/search_tools/batch.rs:409 | search-batch-unapplied-scope-echo
DEVANA-SUMMARY: fixed | P1 | high | search_batch hybrid (and text path_class-only) probes echo unapplied filters as zero-hit applied scope, lying about search constraints.
