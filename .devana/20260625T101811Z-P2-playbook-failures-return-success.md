DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/cli_runtime/commands/playbooks.rs:48 | Slug: playbook-failures-return-success

# Playbook Regression Failures Return CLI Success

## Finding

The hybrid playbook command prints nonzero required or target failure counts, but still returns `Ok(())`.

## Violated Invariant Or Contract

A regression runner with explicit pass/fail counts should propagate failing regression status to the command result.

## Oracle

`HybridPlaybookProbeOutcome` defines `passed_required`, `passed_targets`, and `passed_all`, and the runner computes `required_failures` and `target_failures`.

## Counterexample

A playbook whose required witness is missing produces `required_failures = 1`. The CLI wrapper prints `status=ok` and exits successfully.

## Why It Might Matter

CI or automation can accept a broken retrieval regression suite because the command status does not reflect failed playbooks.

## Proof

Error-semantics mismatch: `run_hybrid_playbook_regressions` computes failure counts, but `run_hybrid_playbook_command` only writes optional output and prints a success summary at lines 44-49 before returning `Ok(())`. No branch checks the failure counts.

## Counterevidence Checked

Parse and load errors propagate through `?`. Target failures are counted only when target enforcement is requested. There is no alternate failing status path for nonzero regression failures.

## Suggested Next Step

Return an error when `required_failures > 0` or `target_failures > 0`, matching the existing enforcement semantics.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed. Confirmed `run_hybrid_playbook_command` printed `status=ok` and returned `Ok(())` regardless of `summary.required_failures`/`target_failures`. Now computes `regressions_failed = required_failures > 0 || (enforce_targets && target_failures > 0)`, prints the summary with an accurate `status=ok|failed`, and returns an `io::Error` (nonzero exit) when failed — so CI surfaces a broken retrieval suite. Target failures are gated on `enforce_targets` to match the existing enforcement semantics (target failures are only counted when enforcement is requested). Compiles. No bespoke test added: exercising the CLI wrapper needs a searcher + indexed repo + failing playbook YAML fixtures (disproportionate for a 3-line error-propagation change); the failure condition is a direct read of the already-computed summary counts.

DEVANA-KEY: crates/cli/src/cli_runtime/commands/playbooks.rs:48 | P2 | playbook-failures-return-success
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/cli_runtime/commands/playbooks.rs:48 - Hybrid playbook command now reports status=failed and returns a nonzero error when required (or enforced target) regressions fail.
