DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/searcher/hybrid_execution/pipeline.rs:462 | Slug: hybrid-lexical-only-mode-mismatch

# Hybrid search uses two different lexical_only_mode predicates

## Finding

Post-selection guardrails in the hybrid pipeline compute `lexical_only_mode` from
semantic channel health and whether pre-fusion semantic hits are empty. The MCP
execution note exposed to clients computes `lexical_only_mode` from semantic status
and post-fusion `semantic_match_count`. When the semantic channel is healthy and
produced hits that none survived fusion, guardrails run with `lexical_only_mode =
false` while the response note reports `lexical_only_mode: true`.

## Violated Invariant Or Contract

The `lexical_only_mode` flag used for ranking policy and the `lexical_only_mode`
reported in hybrid search metadata must describe the same query state.

## Oracle

Pipeline guardrails (`pipeline.rs` ~462–463):

```rust
let lexical_only_mode = semantic_channel_result.health.status != ChannelHealthStatus::Ok
    || semantic_channel_result.hits.is_empty();
```

MCP note (`searcher/types.rs` ~336–337):

```rust
let lexical_only_mode =
    semantic_status != HybridSemanticStatus::Ok || semantic_match_count == 0;
```

`semantic_match_count` counts semantic hits that survived fusion overlap with final
matches, not pre-fusion channel hits.

## Counterexample

- Semantic channel status `Ok` with 5 pre-fusion hits
- Fusion produces final matches with zero semantic overlap → `semantic_match_count
  = 0`
- Guardrails apply full semantic ranking penalties (`lexical_only_mode = false`)
- `search_hybrid` note/metadata reports `lexical_only_mode: true` and may enable
  exact-pivot / witness demotion paths keyed off the note

## Why It Might Matter

Ranking policy and client-visible mode disagree on the same query, so agents
treating `lexical_only_mode` as ground truth may misread result trustworthiness
and provenance workload metadata that copies the note.

## Proof

**Contract mismatch:** two predicates for the same field name in one request path.

**Dataflow trace:** pre-fusion hits present → fusion drops all → divergent flags →
different guardrail behavior vs client metadata.

## Counterevidence Checked

- When semantic channel is unhealthy or produced zero hits, both predicates agree
- `search_hybrid` MCP layer applies additional guardrails after cloning
  `stage_attribution`, widening the provenance attribution gap but not causing this
  specific flag split

## Suggested Next Step

Unify `lexical_only_mode` derivation in one helper used by both pipeline
guardrails and `HybridExecutionNote`, or rename the fields if they intentionally
measure different stages.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report from static inspection across
  `contracts-errors` and `invariants-contracts` trails.
- 2026-06-27: fixed. Confirmed the split: pipeline guardrails computed
  `lexical_only_mode` from `health.status != Ok || hits.is_empty()` (pre-fusion),
  while `hybrid_execution_note_from_channel_results` used `status != Ok ||
  semantic_match_count == 0` (post-fusion overlap). A healthy semantic channel
  whose hits all lost fusion thus ran guardrails with lexical_only_mode=false but
  reported lexical_only_mode=true. Unified on the pre-fusion definition (the correct
  meaning — semantic still contributed to ranking even if no hit reached the final
  page) via a shared helper `hybrid_lexical_only_mode(status, hit_count)` in
  searcher/types.rs, called by both the note (now keyed on `semantic_hit_count`) and
  the pipeline guardrails. `HybridSemanticStatus` is a type alias of
  `ChannelHealthStatus` and the Filtered→Disabled note mapping never affects the Ok
  branch, so both call sites agree exactly. The one client-facing test asserting the
  note flag uses `semantic:false` (status != Ok dominates) and is unaffected. Added
  unit tests for the helper and the note (incl. the dropped-hits case that now
  reports lexical_only_mode=false). searcher lib suite green (364 tests).

DEVANA-KEY: crates/cli/src/searcher/hybrid_execution/pipeline.rs:462 | P2 | hybrid-lexical-only-mode-mismatch
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/searcher/hybrid_execution/pipeline.rs:462 - Pipeline guardrails and HybridExecutionNote derived lexical_only_mode from pre- vs post-fusion predicates; unified both on a shared pre-fusion helper so ranking policy and client-visible mode always agree, plus regression tests.