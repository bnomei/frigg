DEVANA-FINDING: v1
DEVANA-STATE: fixed | P1 | high | security=no
DEVANA-KEY: crates/cli/src/mcp/server/presentation.rs:873 | go-to-disambiguation-wrong-recovery

# go_to_definition disambiguation is presented as PreciseGraphUnavailable

## Finding

When target resolution needs disambiguation (`NavigationTargetSelection::DisambiguationRequired`), `go_to_definition` returns success with empty `matches`, `mode: UnavailableNoPrecise`, populated `target_selection`, and **empty** `recovery`. `present_go_to_definition_response` sees empty matches + empty recovery + `UnavailableNoPrecise` and fills recovery via `for_zero_hit` with `reason_override = PreciseGraphUnavailable`. Compact mode then strips `metadata`/`note` (where `disambiguation_required` was duplicated), leaving agents with a false SCIP-unavailable recovery that steers toward waiting for precise generation instead of re-calling with path/line or `stable_symbol_id`.

## Violated Invariant Or Contract

Disambiguation-required empty results must not claim precise-graph absence. Recovery must match the real resolution barrier (`disambiguation_required`), not overload `UnavailableNoPrecise` into `PRECISE_GRAPH_UNAVAILABLE`.

## Oracle

Producer metadata explicitly sets `disambiguation_required: true` while reusing `UnavailableNoPrecise` mode. Recovery builders include a dedicated `precise_graph_unavailable` message about missing SCIP. FUT empty-go-to recovery exists for missing params; disambiguation is a different state. Presenter should not invent SCIP failure when `target_selection.status` already explains the empty hit set.

## Counterexample

1. Call `go_to_definition` with a symbol that has multiple same-rank definitions and no path/line disambiguator.
2. Producer returns `matches: []`, `target_selection.status = disambiguation_required`, `recovery: default()`.
3. Presenter overwrites recovery with `PreciseGraphUnavailable` / message “no precise graph/SCIP data”.
4. Compact response drops metadata; agent follows SCIP/workspace wait guidance instead of supplying location/stable id.

## Why It Might Matter

Important navigation workflow breakage: agents loop on wrong remediation (index/SCIP wait) while candidates for disambiguation were available. High-confidence contract mismatch on the default compact surface.

## Proof

**Producer** (`go_to_definition.rs` disambiguation arm ~435–463): empty matches, `mode: UnavailableNoPrecise`, `recovery: RecoveryFields::default()`.

**Consumer** (`presentation.rs` ~873–889): empty recovery + `UnavailableNoPrecise` ⇒ `reason_override = PreciseGraphUnavailable`.

**Contract mismatch:** `target_selection` says disambiguate; top-level recovery says no SCIP.

## Counterevidence Checked

- Integration tests assert empty matches + mode but not recovery correctness.
- Empty-param path correctly uses `empty_go_to_definition()` before presentation.
- Strongest false-positive: “mode UnavailableNoPrecise always means no SCIP.” Ruled out by producer metadata `disambiguation_required: true` and non-empty `target_selection` candidates on the same response.
- Same presenter pattern may affect sibling location tools that reuse empty recovery + that mode; this report keys the present-path bug.

## Suggested Next Step

On disambiguation returns, set dedicated non-empty recovery (or a mode that present does not map to SCIP). In present: if `target_selection.status == DisambiguationRequired`, do not apply `PreciseGraphUnavailable`.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-09: open by Devana. Initial report written from static source inspection during exhaustive `--all` hunt.
- 2026-07-09: fixed by presenting DisambiguationRequired with dedicated recovery instead of PreciseGraphUnavailable. Tests: `recovery_disambiguation_required_builder_is_actionable`, `go_to_definition_disambiguation_does_not_claim_precise_graph_unavailable`.

DEVANA-KEY: crates/cli/src/mcp/server/presentation.rs:873 | go-to-disambiguation-wrong-recovery
DEVANA-SUMMARY: fixed | P1 | high | go_to_definition disambiguation empty hits are presented as PreciseGraphUnavailable recovery, steering agents away from location/stable-id disambiguation.
