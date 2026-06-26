DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server/precise_graph/generation.rs:943 | Slug: precise-failed-summary-skips-retry

# Failed Precise Generation Summaries Suppress Retries

## Finding

When there are no changed or deleted paths, precise-generation work is considered unnecessary if any cached generation summary exists, even when that summary is `Failed` or `MissingTool`.

## Violated Invariant Or Contract

`SkippedNoWork` should mean the precise artifact is already usable or no generator applies. A previous failed generation must not satisfy future generation work.

## Oracle

Generation summaries carry explicit success and failure states, and lifecycle reporting uses failures to recommend remediation.

## Counterexample

A Python workspace triggers precise generation while the generator tool is missing. The missing-tool summary is cached. The user installs the tool and calls `workspace_attach` or `workspace_prepare` again with no file changes. The generator is skipped because the failed summary exists.

## Why It Might Matter

Precise navigation can remain unavailable after the user fixes the environment, unless they know to force a path-changing reindex, detach, or restart.

## Proof

State transition mismatch: `workspace_precise_generation_needed` returns `scip_cached_workspace_precise_generation(...).is_none()` for empty `changed_paths` and `deleted_paths` at lines 943-949. It does not inspect the cached summary status or the expected artifact path.

## Counterevidence Checked

`workspace_reindex` and `workspace_detach` clear the generation cache, and changed paths can trigger generation. Normal attach and prepare retry paths do not clear failed generation summaries.

## Suggested Next Step

Treat only successful cached summaries with present artifacts as "no work", or clear failed precise-generation summaries when retry-capable attach/prepare paths run.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed. Confirmed the no-changes branch of `workspace_precise_generation_needed` returned `scip_cached_...().is_none()`, so ANY cached summary (including `Failed`/`MissingTool`/`Timeout`) made it report no work — a user who fixed the environment and re-ran attach/prepare with no file changes never got a retry. Changed the branch to: `None => needs work`; `Some(summary) => retry only when status is Failed | MissingTool | Timeout`. Terminal states (`Succeeded`, plus `Skipped`/`Unsupported`/`NotConfigured`) remain no-work so generators are not re-spawned needlessly on every attach. Added regression test `precise_generation_failed_summary_does_not_suppress_retry` (registers an active task so decisions resolve deterministically without spawning a generation thread; a seeded `Succeeded` summary → `SkippedNoWork`, a seeded `MissingTool` summary → not `SkippedNoWork`). Passes.

DEVANA-KEY: crates/cli/src/mcp/server/precise_graph/generation.rs:943 | P2 | precise-failed-summary-skips-retry
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/mcp/server/precise_graph/generation.rs:943 - Failed/missing-tool/timeout precise summaries no longer satisfy the no-work predicate, so attach/prepare retries precise generation after the environment is fixed (regression test added).
