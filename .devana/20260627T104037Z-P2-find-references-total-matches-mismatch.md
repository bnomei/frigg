DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server/navigation_tools/references.rs:472 | Slug: find-references-total-matches-mismatch

# find_references reports total_matches before limit truncation

## Finding

`find_references` sets `total_matches` from the full precise or heuristic
reference list, then builds `matches` with `.take(limit)`. When references exceed
`limit`, `total_matches` can be larger than `matches.len()`. Integration tests
assert `total_matches == matches.len()`, and `search_hybrid` sets
`total_matches` to `matches.len()`, so the MCP contract appears to require aligned
counts.

## Violated Invariant Or Contract

`FindReferencesResponse.total_matches` should equal `matches.len()` for the
returned page, matching the test contract in
`crates/cli/tests/tool_handlers/references.rs` and hybrid search behavior.

## Oracle

```39:39:crates/cli/tests/tool_handlers/references.rs
    assert_eq!(response.total_matches, response.matches.len());
```

```751:751:crates/cli/src/mcp/server/search_tools/hybrid.rs
            "total_matches": matches.len(),
```

## Counterexample

- `limit = 5`
- Precise references for symbol: 20 occurrences
- `total_matches = 20` (`references.rs` ~471–472)
- `matches.len() = 5` after `.take(limit)` (~483–485)

Client code or agents using `total_matches` to size pagination or completion
checks will believe more matches were returned than the response contains.

## Why It Might Matter

Downstream MCP clients may truncate early, mis-report coverage, or fail assertions
that mirror the repo's own reference tests.

## Proof

**Contract mismatch:** response builder sets `total_matches` from pre-truncation
count while `matches` is post-`limit` slice (~909–910 shows the same pattern on
the heuristic loader path).

## Counterevidence Checked

- `result_handle` is `None` on these paths, so there is no documented pagination
  continuation using `total_matches` as a pre-limit total in the response schema.
- Small fixture tests keep counts equal, hiding the mismatch unless `limit` is
  exercised with many references.

## Suggested Next Step

Set `total_matches = matches.len()` before returning, or document and implement
true pagination if `total_matches` is intended as a pre-limit total.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report written from static source inspection
  across `invariants-contracts` trail.
- 2026-06-27: fixed. Confirmed all three response paths (precise-direct ~564,
  precise+heuristic-supplement ~885, heuristic-only ~1034) set the response
  `total_matches` from the `total_matches` variable, which holds the pre-limit count
  (assigned at 472/819/909) while `matches` is built with `.take(limit)`. Fixed by
  setting each `FindReferencesResponse.total_matches` to `matches.len()` (the
  returned page). Field order — `total_matches` precedes `matches` in the struct
  literal — so `matches.len()` borrows before `matches` is moved. The pre-limit
  total is intentionally preserved: precise paths already record it as
  `precise_reference_count` in metadata, and the `total_matches` variable still
  feeds provenance/telemetry (~1078/1111) unchanged. Added regression test
  `find_references_total_matches_equals_returned_page_under_limit` (7 `User`
  references; a limit=50 query proves >2 are available, a limit=2 query asserts
  matches.len()==2 and total_matches==matches.len()). Full `tool_handlers`
  references suite green (21 tests).

DEVANA-KEY: crates/cli/src/mcp/server/navigation_tools/references.rs:472 | P2 | find-references-total-matches-mismatch
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/mcp/server/navigation_tools/references.rs:472 - find_references reported the pre-limit reference count as response total_matches while matches were truncated by limit; fixed by setting the response total_matches to matches.len() (pre-limit total retained in metadata/provenance) plus a regression test.