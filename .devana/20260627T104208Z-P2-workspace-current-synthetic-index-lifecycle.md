DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server.rs:1405 | Slug: workspace-current-synthetic-index-lifecycle

# workspace_current returns synthetic index_lifecycle not reflecting repo state

## Finding

`workspace_current` always constructs `index_lifecycle` with hardcoded placeholders:
`WorkspaceAttachIndexMode::Ensure`, `waited_for_completion=false`,
`WorkspaceIndexAction::SkippedNoWork`, and no failure summary. It does not read
stored attach history from `AttachedWorkspace`. When the repository is not index-
ready and no active index task is running, `workspace_index_lifecycle_summary`
falls through to `WorkspaceIndexLifecyclePhase::Skipped`, which the operator runbook
documents as meaning the caller used `index_mode=skip`.

## Violated Invariant Or Contract

`workspace_current.index_lifecycle` should describe current index readiness and the
action needed, not a synthetic attach snapshot that never occurred.

## Oracle

`docs/operator-runbook.md` describes phase `skipped` as caller-requested skip.
`workspace_current` builder (`server.rs` ~1405–1413) ignores actual attach mode
and dirty-root state. Phase derivation (`workspace_session.rs` ~399–417) maps
not-ready + no active tasks + `SkippedNoWork` to `Skipped`.

## Counterexample

1. `workspace_attach` with `index_mode=ensure`; index becomes ready
2. Files change on disk (`lexical_ready=false`), watch off, no active tasks
3. `workspace_current` returns:
   - `phase: skipped` (runbook: intentional skip)
   - `mode: ensure`
   - `action_taken: skipped_no_work`
   - `lexical_ready: false`

Operators following the runbook can misread a dirty/stale repo as an intentional
skip attach.

## Why It Might Matter

Automation using `workspace_current` for readiness decisions can skip needed
reindex work or misinterpret stale state.

## Proof

**Contract mismatch:** documented phase semantics vs synthetic lifecycle inputs.

**Counterexample value:** dirty root + `SkippedNoWork` placeholder → `phase:
skipped`.

## Counterevidence Checked

- `workspace_attach` returns accurate attach-time lifecycle from real parameters
- `recommended_action` may still suggest `RerunReindex` when not ready, partially
  mitigating misread of `phase`
- No integration test validates `workspace_current.index_lifecycle` accuracy

## Suggested Next Step

Derive `workspace_current.index_lifecycle` from live readiness, active tasks, and
dirty-root state instead of attach placeholders; add a distinct phase for stale/
needs-refresh when not ready without an active task.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report from static inspection across
  `invariants-contracts` and `outside-in-entrypoints` trails.
- 2026-06-27: fixed. Confirmed `workspace_current` passes synthetic
  `Ensure`/`SkippedNoWork` placeholders to `workspace_index_lifecycle_summary`, whose
  phase derivation fell through to `Skipped` for not-ready + idle + SkippedNoWork —
  and the runbook documents `skipped` as an intentional `index_mode=skip`, so a
  dirty/stale repo read as a deliberate no-op. Note the summary already recomputes
  readiness and active tasks live, so only the phase label for the idle case was
  wrong. Fix: added a distinct `WorkspaceIndexLifecyclePhase::Stale` variant and
  route not-ready + idle + `SkippedNoWork` (i.e. NOT an intentional
  `SkippedByRequest`) to it; `Stale` maps to `recommended_action = RerunReindex`.
  Both existing `Skipped`-asserting tests use `index_mode=skip` (→ `SkippedByRequest`
  → still `Skipped`) and stay green. Documented the new phase in
  docs/operator-runbook.md. (The remaining `mode: Ensure` field is left as-is —
  `AttachedWorkspace` does not store the original attach mode and the operator-facing
  signal is phase + recommended_action.) Added integration test
  `workspace_current_reports_stale_not_skipped_for_unindexed_repository`. workspace
  tool_handlers (10) and runtime_gate workspace (7) suites green.

DEVANA-KEY: crates/cli/src/mcp/server.rs:1405 | P2 | workspace-current-synthetic-index-lifecycle
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/mcp/server.rs:1405 - workspace_current reported phase=skipped (intentional skip) for dirty/stale idle repos; added a distinct Stale phase (→ RerunReindex) for not-ready+idle+non-skip, updated the runbook, plus a regression test.