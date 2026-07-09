DEVANA-FINDING: v1
DEVANA-STATE: fixed | P1 | high | security=no
DEVANA-KEY: crates/cli/src/mcp/server/search_tools/batch.rs:176 | batch-all-zero-multi-hypothesis-recovery

# BATCH_ALL_ZERO recovery reuses multi_hypothesis correction after batch already ran

## Finding

When all probes return zero hits, `search_batch` seeds recovery with `RecoveryFields::multi_hypothesis`, then only overwrites `error_code` (`BATCH_ALL_ZERO`) and `message` (and sometimes `suggested_next` from probe summaries). Fields that remain from `multi_hypothesis` include `correction_hint` telling the agent to “Prefer search_batch when available” and `zero_hit_reason: QueryMiss`, even when every `probe_summary` reports stronger reasons (`scope_excluded_all_candidates`, `index_stale_possible`, `query_looks_like_regex`).

## Violated Invariant Or Contract

Post-batch all-zero recovery must not recommend re-entering the multi-hypothesis pre-batch routing strategy as the primary correction, and top-level `zero_hit_reason` must not erase stronger per-probe diagnostics.

## Oracle

`multi_hypothesis` is documented/built as the **pre-batch** routing builder (prefer batch / parallel probes). `BATCH_ALL_ZERO` message correctly says inspect `probe_summary`, but leftover multi_hypothesis fields contradict that. FUT-006 expects structured zero-hit reasons to drive next actions.

## Counterexample

1. All batch probes zero because of overly tight path_regex (probe summaries: `ScopeExcludedAllCandidates`).
2. Top-level recovery: `error_code=BATCH_ALL_ZERO`, `zero_hit_reason=query_miss`, `correction_hint` still prefers `search_batch`.
3. Compact agent re-issues another batch with similar probes instead of broadening scope or consulting index staleness.

## Why It Might Matter

Wastes multi-turn agent loops and hides actionable zero-hit class. High-confidence recovery-contract bug on the new batch surface.

## Proof

**Control-flow:** `all_zero` branch assigns `multi_hypothesis` then patches only subset of fields (`batch.rs` ~176–198).

**Contract mismatch:** finished batch still carries pre-batch multi-hypothesis correction/reason.

## Counterevidence Checked

- Tests only assert weak recovery presence, not correction correctness.
- `suggested_next` may be overwritten from probe summaries, but `correction_hint` / `zero_hit_reason` remain wrong.
- Strongest false-positive: “message is enough.” Ruled out because compact agents prioritize `zero_hit_reason` and `correction_hint` fields.

## Suggested Next Step

Build a dedicated `batch_all_zero` recovery from aggregated probe diagnostics (reason priority, no multi_hypothesis correction), or clear multi_hypothesis fields after setting `BATCH_ALL_ZERO`.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-09: open by Devana. Initial report written from static source inspection during exhaustive `--all` hunt.
- 2026-07-09: fixed by dedicated `RecoveryFields::batch_all_zero` plus strongest probe reason aggregation (no multi_hypothesis leftover). Tests: `batch_all_zero_recovery_is_not_multi_hypothesis`, `strongest_batch_zero_reason_prefers_actionable_codes`.

DEVANA-KEY: crates/cli/src/mcp/server/search_tools/batch.rs:176 | batch-all-zero-multi-hypothesis-recovery
DEVANA-SUMMARY: fixed | P1 | high | BATCH_ALL_ZERO recovery keeps multi_hypothesis correction and QueryMiss reason after batch already finished, hiding stronger probe diagnostics.
