DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/watch/supervisor.rs:656 | Slug: watch-startup-refresh-error-swallow

# Watch supervisor silently drops startup refresh when freshness evaluation fails

## Finding

`queue_startup_refresh_if_needed` and `queue_semantic_followup_if_needed` both
use `let Ok(status) = startup_refresh_status(...) else { return }` with no log or
scheduler side effect. When `startup_refresh_status` returns `Err` (storage I/O,
`semantic_runtime.validate_startup` failure, projection-family query error), watch
does not enqueue manifest-fast or semantic-followup work and emits no operator
signal. MCP attach surfaces the same evaluation through
`WorkspaceIndexAction::Failed` with a lifecycle summary.

## Violated Invariant Or Contract

After watch lease acquisition or manifest-fast success, when repository freshness
requires a follow-up refresh, that work must be queued or the failure must be
observable. Silent `return` on `Err` violates the asymmetry with MCP attach error
surfacing.

## Oracle

`ensure_workspace_index_for_attach` returns `WorkspaceIndexAction::Failed` with
`readiness_error` when evaluation fails (`workspace_session.rs` ~518–529). Watch
enqueue helpers swallow the same `startup_refresh_status` `Err` (`supervisor.rs`
~656–659, ~693–695).

## Counterexample

1. Watch lease acquired on a repo needing semantic follow-up
2. `startup_refresh_status` returns `Err` (e.g. provenance DB read failure)
3. `queue_semantic_followup_if_needed` returns immediately after manifest-fast
   success without enqueueing semantic follow-up
4. Watch logs manifest-fast success; scheduler has no semantic-followup pending
5. Repository stays semantically stale until an unrelated path change retriggers
   manifest-fast and hopefully succeeds the freshness check

## Why It Might Matter

Watch can appear healthy while indexes never reach semantic readiness, with no
client-visible failure on the watch path.

## Proof

**Control-flow trace:** `startup_refresh_status` → `Err` → bare `return` → no
`enqueue_initial_sync` / `enqueue_semantic_followup` → no log.

**Cross-entry mismatch:** MCP attach reports failure; watch enqueue silently skips.

## Counterevidence Checked

- Path-change-driven `record_path_change` eventually re-queues manifest-fast
- Semantic followup deferral via `mark_failed` when another SemanticRefresh is
  active is intentional backoff, not silent drop
- `should_refresh=false` on `Ok` status is correct no-op behavior; this report is
  only the `Err` branch

## Suggested Next Step

Log at `warn!` and/or `mark_failed` with retry when `startup_refresh_status`
returns `Err`, mirroring other watch refresh failure paths.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report from static inspection across
  `outside-in-entrypoints`, `contracts-errors`, and `state-lifecycle` trails.
- 2026-06-27: fixed. Confirmed both `queue_startup_refresh_if_needed` and
  `queue_semantic_followup_if_needed` used `let Ok(status) = startup_refresh_status(..)
  else { return }` with no log or scheduler effect; `startup_refresh_status`
  (watch/repository.rs) errors on `validate_startup` (semantic creds), freshness
  storage I/O, or projection-family query failures. Fixes:
  * `queue_startup_refresh_if_needed`: on Err, `warn!` and conservatively
    `enqueue_initial_sync(ManifestFast)`. Manifest-fast runs lexical-only (the
    dispatch disables semantic for it), so it makes progress even when the Err was a
    semantic-credential problem, and the reindex re-derives freshness.
  * `queue_semantic_followup_if_needed`: on Err, `warn!` only — deliberately NOT
    enqueueing semantic work, since the common cause (missing credentials) would put
    a semantic refresh into a failing retry loop. Observability satisfied.
  Added unit test
  `startup_refresh_eval_failure_queues_manifest_fast_instead_of_silent_drop` (enabled
  semantic runtime + empty credentials → validate_startup Err before any filesystem
  access; asserts a manifest-fast refresh becomes pending). watch lib suite green
  (30 tests).

DEVANA-KEY: crates/cli/src/watch/supervisor.rs:656 | P1 | watch-startup-refresh-error-swallow
DEVANA-SUMMARY: Status=fixed | P1 high crates/cli/src/watch/supervisor.rs:656 - Watch enqueue helpers silently returned on startup_refresh_status Err; fixed by warn-logging both paths and conservatively queueing a lexical manifest-fast refresh on startup eval failure (semantic followup logs only to avoid a credential-failure retry loop) plus a regression test.