DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/storage/semantic_store.rs:180 | Slug: semantic-advance-deletes-skipped-reads

# Incremental semantic advance deletes chunks for changed files that fail to open

## Finding

During `IncrementalAdvance`, `advance_semantic_embeddings_for_repository` deletes
live semantic rows for every path in `changed_paths` and `deleted_paths` before
inserting new embedding records. `build_semantic_chunk_candidates` skips files
that fail to open or read with a warning and returns an empty candidate list for
that path without failing the refresh. The advance transaction still deletes the
old chunks and commits successfully, shrinking the semantic corpus for manifest
files that remain on disk.

## Violated Invariant Or Contract

After a successful incremental semantic refresh, every path in `changed_paths`
that remains in the manifest should retain live semantic chunks, or the refresh
should fail.

## Oracle

`build_semantic_chunk_candidates` documents that skipped files would otherwise
vanish from semantic search (~333–337). `advance_semantic_embeddings_for_repository`
always removes `changed_paths` before insert (`semantic_store.rs` ~180–197).
Distinct from manifest-read-false-deletion (`.devana/20260627T104036Z`), which
false-deletes at manifest diff time; this bug is at semantic advance time for
paths that are correctly listed as changed in the manifest.

## Counterexample

1. `src/lib.rs` is modified and included in manifest diff `changed_paths`
2. Between manifest scan and semantic refresh, `File::open(src/lib.rs)` fails
   (permissions, transient lock, delete/rename race)
3. Chunk builder logs warning, returns no candidates for that file
4. Advance deletes live rows for `src/lib.rs`, inserts zero replacement rows,
   commits `Ok(())`
5. Reindex summary reports success; manifest still lists `src/lib.rs`; semantic
   search has no chunks for it

## Why It Might Matter

Semantic search silently loses coverage for files that were not deleted, with
only a log warning that does not surface in `ReindexSummary` or MCP tool responses.

## Proof

**Control-flow trace:** open/read `Err` in chunk builder → `Ok(Vec::new())` →
advance `delete_live_semantic_rows_for_paths(changed_paths)` → empty insert →
`tx.commit()` → `Ok(())`.

**Dataflow trace:** manifest `changed_paths` → delete side effect → missing new
records for same paths.

## Counterevidence Checked

- Full `ReindexMode::Full` rebuild has different semantics (clears whole corpus)
- Advance epoch gate rejects stale `previous_snapshot_id` mismatches
- SQLite transaction rolls back on storage errors, not on empty candidate lists
- Per-file warnings exist but do not block commit or change refresh mode

## Suggested Next Step

If chunk build returns zero candidates for a path in `changed_paths`, retain
previous live rows or fail the advance transaction. Alternatively downgrade to
full rebuild when any changed path produces no candidates.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report from static inspection across
  `inside-out-paths` and `contracts-errors` trails.
- 2026-06-27: fixed. Confirmed `advance_semantic_embeddings_for_repository`
  (semantic_store.rs ~180-197) deletes live rows for all `changed_paths` ∪
  `deleted_paths` then inserts `records`, while `build_semantic_chunk_candidates`
  (indexer/semantic.rs) silently returned zero chunks for files that fail
  `File::open`/`read_to_string`. A changed file that failed the semantic read thus
  had its rows deleted with no replacement. Fix threads the skip signal through:
  `build_semantic_chunk_candidates` now returns `SemanticChunkBuild { candidates,
  unreadable_paths }` (semantic-eligible repo-relative paths skipped due to
  open/read errors), `build_semantic_embedding_records` forwards them as
  `SemanticEmbeddingBuild { records, unreadable_paths }`, and the IncrementalAdvance
  arm of `execute_semantic_refresh_plan` excludes unreadable paths from the
  delete set (`retained_changed_paths`) so their existing live rows survive. This
  mirrors the manifest-read fix philosophy (stale-but-present > silently missing);
  the warning is still logged. Legitimately-emptied files (readable, zero chunks)
  remain in the delete set and are correctly cleared. Full-rebuild path keeps the
  whole-corpus replace semantics (out of scope, only destructures the new tuple).
  Updated benchmark wrapper + 2 builder unit tests for the new return type. Added
  integration test
  `semantic_changed_only_retains_rows_for_changed_file_that_fails_semantic_read`
  using invalid-UTF-8 content (raw-byte manifest digest still flags it changed,
  but `read_to_string` fails) — deterministically isolating this bug from the
  sibling manifest-read fix. indexer (79) and storage (63) lib suites green.

DEVANA-KEY: crates/cli/src/storage/semantic_store.rs:180 | P1 | semantic-advance-deletes-skipped-reads
DEVANA-SUMMARY: Status=fixed | P1 high crates/cli/src/storage/semantic_store.rs:180 - Incremental semantic advance deleted changed_paths even when per-file open/read failures produced zero replacement chunks; fixed by surfacing unreadable paths from the chunk builder and excluding them from the advance delete set (retaining existing rows) plus a regression test.