DEVANA-FINDING: v1
Priority: P2 | Confidence: medium | Security-sensitive: no | Status: fixed
Location: crates/cli/src/graph/scip_support.rs:159 | Slug: scip-path-escape-navigation

# SCIP Paths Can Escape Navigation Results

## Finding

SCIP document `relative_path` values are trimmed but not normalized or checked for absolute paths and parent-directory escapes before they enter the precise graph.

## Violated Invariant Or Contract

Precise graph paths returned by navigation should be repository-relative paths that follow the same containment contract as manifest and read-file paths.

## Oracle

Manifest and retrieval projection paths normalize repository-relative paths and reject root escapes. Navigation result handles are later consumed as repository-relative `read_file` paths.

## Counterexample

A SCIP artifact document has `relative_path` set to `../outside.rs` and contains a definition for a symbol. `go_to_definition` can return `../outside.rs` as a precise location path.

## Why It Might Matter

Navigation can return unusable off-root paths and store them in result handles. Follow-up `read_match` then fails containment validation instead of opening the reported result.

## Proof

Dataflow trace: `map_scip_document` trims only emptiness at `crates/cli/src/graph/scip_support.rs:159`. Precise navigation later calls `canonicalize_navigation_path`, which joins without canonicalizing and returns `relative_display_path` at `crates/cli/src/mcp/server/navigation_precise.rs:145`. Presentation stores those paths as read-match anchors.

## Counterevidence Checked

Direct `read_file` canonicalizes and denies root escapes. Retrieval projection SCIP enrichment is gated by manifest witnesses. This issue is specific to precise graph/navigation responses.

## Suggested Next Step

Normalize SCIP document paths with the same repository-relative path validator used for manifests, and reject absolute or parent-escaping document paths during SCIP ingest.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-27: fixed. Confirmed `map_scip_document` (scip_support.rs:159) only trimmed and empty-checked `relative_path`, so a SCIP document path like `../outside.rs` or `/etc/passwd` flowed into the precise graph and could surface as a `go_to_definition` location stored in a read-match anchor that `read_match`/`read_file` later reject on containment. Fix: added `normalize_scip_document_relative_path`, a pure lexical containment normalizer mirroring the manifest contract (`indexer::manifest::repository_relative_path_string_from_relative`) — it folds backslashes to `/` first so Windows-style escapes are caught on every platform, collapses `.` segments, and rejects any `..`/root/drive-prefix component. `map_scip_document` now normalizes the trimmed path and returns a typed `ScipInvalidInputCode::InvalidDocumentPath` (new variant) on escape, before any occurrences/symbols are ingested. Regression test `scip_ingest_rejects_document_paths_that_escape_repository_root` (graph/tests.rs) asserts `../outside.rs`, `src/../../outside.rs`, `/etc/passwd`, and `..\\outside.rs` are each rejected with the typed code and leave precise state unmutated, while `./src/a.rs` normalizes and ingests. `cargo test graph::` (18) and the SCIP ingest suite green.

DEVANA-KEY: crates/cli/src/graph/scip_support.rs:159 | P2 | scip-path-escape-navigation
DEVANA-SUMMARY: Status=fixed | P2 medium crates/cli/src/graph/scip_support.rs:159 - SCIP document paths are not containment-checked before graph ingest, so precise navigation can return off-root paths that read_match cannot open.
