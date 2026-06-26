DEVANA-FINDING: v1
Priority: P2 | Confidence: medium | Security-sensitive: no | Status: wontfix
Location: crates/cli/src/indexer/symbols/spans.rs:36 | Slug: location-column-eol-false-success

# Oversized Location Columns Resolve False Positions

## Finding

Location-based tools reject zero and line-outside-file values, but a column past the end of an existing line is silently clamped to the line end.

## Violated Invariant Or Contract

A provided 1-based line and column should identify a real file position or be rejected as outside bounds. It should not silently move to a different position.

## Oracle

The same location path returns "outside file" when no offset exists, and zero line/column values are invalid.

## Counterexample

For a file whose first line is `fn alpha() {}`, request `inspect_syntax_tree` or `go_to_definition` with `line=1` and a very large `column`. The coordinate does not exist, but it resolves as if the cursor were at end of line.

## Why It Might Matter

Navigation and structural inspection can return a syntax focus or symbol for an impossible coordinate, making callers trust an unrelated location.

## Proof

Counterexample value: `byte_offset_for_line_column` computes `column.saturating_sub(1).min(line_len)` at line 36, so any oversized column becomes EOL. `inspect_syntax_tree_internal` then focuses a node at that offset, and navigation fallback computes a symbol query from the clamped location.

## Counterevidence Checked

Line zero and column zero are rejected, and a line past EOF returns `None`. This is distinct from the existing PHP multibyte slice panic; it is shared false-success behavior for oversized columns.

## Suggested Next Step

Return `None` when `column - 1` exceeds the target line length, and update location callers to surface invalid-params for out-of-range columns.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: wontfix (intended, convention-standard behavior). Confirmed `byte_offset_for_line_column` clamps `column.saturating_sub(1).min(line_len)` at spans.rs:55,65. However this matches the LSP Position contract: "If the character value is greater than the line length it defaults back to the line length." The requested *line* must still exist — line 0 / column 0 are rejected and a line past EOF returns `None` — so the resolved offset is always on the addressed line, clamped only to that line's end (not a wildly unrelated location). The function is also shared: navigation_resolution.rs:638 (the PHP/Blade helper, just hardened for multibyte slicing) and identifier_token resolution deliberately pass an oversized column to find a token around end-of-line, and `rust_navigation_query_hint`/inspection rely on the same lenient resolution. The report's proposed global "return None when column-1 exceeds line length" would break those deliberately-lenient callers and diverge from LSP, while only converting a benign same-line EOL clamp into a hard rejection. Net: the behavior is an intentional editor-coordinate convention, not a correctness defect. Decision: keep clamping. If a future caller genuinely needs exact-coordinate strictness, add a separate strict variant for that caller rather than changing the shared lenient function. No code change.

DEVANA-KEY: crates/cli/src/indexer/symbols/spans.rs:36 | P2 | location-column-eol-false-success
DEVANA-SUMMARY: Status=wontfix | P2 medium crates/cli/src/indexer/symbols/spans.rs:36 - Column-past-EOL clamping matches the LSP Position convention; the line is still required to exist so results stay on the addressed line, and the shared function is intentionally lenient for the PHP/identifier helpers. Global "return None" would break those and diverge from LSP. Kept as intended behavior.
