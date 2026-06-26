DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/storage/semantic_store_read.rs:694 | Slug: semantic-advance-stale-snapshot-drops-chunks

# Incremental semantic advance leaves unchanged chunks with a stale `snapshot_id`, so vector search silently drops them

## Finding

After an incremental ("changed-only") semantic reindex, only the chunks for
added/modified paths are re-inserted with the new snapshot id; unchanged chunks
keep their previous `semantic_chunk.snapshot_id`, while `semantic_head` is advanced
to the new covered snapshot. The vector top-k membership gate filters candidate
chunk ids with `semantic_chunk.snapshot_id = ?2`, where `?2` is the head's new
covered snapshot. Every unchanged chunk therefore fails the gate and is dropped
from results, even though its row and its vector are still live. The vector
`MATCH` itself does not filter by snapshot, so the unchanged chunks are returned
by the KNN scan and then silently discarded by the gate.

## Violated Invariant Or Contract

For all live chunks of a repository/provider/model, `semantic_chunk.snapshot_id`
is assumed to equal `semantic_head.covered_snapshot_id`. The incremental advance
breaks this for every unchanged chunk. Every other read path joins through
`semantic_head` and filters `head.covered_snapshot_id`; only the membership gate
re-imposes the now-inconsistent `chunk.snapshot_id` dimension.

## Oracle

Neighboring read implementations are the source of truth. Payloads, previews,
embeddings, count, and readiness all filter `head.covered_snapshot_id = ?2` with
no `chunk.snapshot_id` predicate:
- `crates/cli/src/storage/semantic_store_read.rs:122` (embeddings)
- `:245`, `:404`, `:762`, `:841` (payloads/previews/health)
The membership gate at `:694` is the lone read that additionally requires
`semantic_chunk.snapshot_id = ?2`, a dimension the writer does not maintain for
unchanged chunks.

## Counterexample

1. Full reindex of `repo-001` → snapshot `S1`. Chunks for `src/main.rs` and
   `src/lib.rs` inserted with `snapshot_id = S1`; head covered = `S1`; both have
   vector rows.
2. Edit only `src/lib.rs`. Changed-only reindex → snapshot `S2`. Plan picks
   `IncrementalAdvance` with `records_manifest = added ∪ modified` (only the lib
   chunk) — `crates/cli/src/indexer/reindex/semantic.rs:255-268`.
3. `advance_semantic_embeddings_for_repository` deletes lib rows, inserts the new
   lib chunk with `snapshot_id = S2`, upserts head covered = `S2`. The `src/main.rs`
   row is untouched → still `snapshot_id = S1`; its vector row persists.
4. Semantic search selects the head's covered snapshot `S2`. Vector `MATCH`
   returns the `src/main.rs` chunk (no snapshot filter). The gate runs
   `... WHERE snapshot_id = S2 AND chunk_id IN (...)`; the main chunk has `S1`, so
   it is excluded and `matches.retain(...)` drops it. The unchanged file is
   invisible to semantic search.

## Why It Might Matter

After any incremental reindex (the common case for file-watch refreshes), every
unchanged file disappears from semantic vector search until the next full rebuild —
i.e. results collapse toward only recently-changed files. Persisted, silent,
correctness regression on the primary semantic-search path.

## Proof

State-transition + contract mismatch:
- Writer advances head snapshot but leaves unchanged chunk snapshot ids:
  `crates/cli/src/storage/semantic_store.rs:180-220` deletes only `removed_paths`
  rows/vectors and inserts only the supplied (changed-only) records; head upsert at
  `:212-220`. No statement rewrites unchanged `semantic_chunk.snapshot_id`.
- Records passed are changed-only:
  `crates/cli/src/indexer/reindex/semantic.rs:260-265` builds `records_manifest`
  from `manifest_diff.added ∪ modified`.
- Vector MATCH is snapshot-agnostic:
  `crates/cli/src/storage/semantic_store_read.rs:601-611` filters only
  `repository_id`, `provider`, `model`, `embedding MATCH`.
- Gate re-imposes stale dimension:
  `crates/cli/src/storage/semantic_store_read.rs:689-700`
  (`AND snapshot_id = ?2`), applied via `:643-652` `matches.retain(...)`.

## Counterevidence Checked

- Are unchanged vector rows deleted on advance? No — `delete_vector_rows_for_chunk_ids`
  only removes `removed_chunk_ids` (changed/deleted paths); the unchanged chunk's
  vector survives and is returned by MATCH, then dropped by the gate.
- Is incremental advance actually reached? Yes. `build_semantic_refresh_plan`
  selects `IncrementalAdvance` when `head.covered == previous_snapshot_id` and no
  unresolved deletes (`reindex/semantic.rs:236,255-268`).
- Does a trigger rewrite `chunk.snapshot_id`? No triggers exist in `storage/schema.rs`.
- Do existing tests catch it? Advance tests assert via head-join read paths
  (`load_semantic_embeddings_for_repository_snapshot`, health) that do not exercise
  the vector top-k membership gate after an advance, so the bug is masked.

## Suggested Next Step

Either change the membership gate at `semantic_store_read.rs:694` to scope by
`head.covered_snapshot_id` (matching the sibling reads) instead of
`semantic_chunk.snapshot_id`, or have the advance rewrite unchanged chunks'
`snapshot_id` to the new covered snapshot inside the advance transaction.

## Status Notes

- 2026-06-26: fixed. Confirmed the advance (`semantic_store.rs:180-220`) only deletes removed paths and inserts changed-only records — it never rewrites unchanged chunks' `snapshot_id` — while the membership gate (`load_allowed_semantic_chunk_ids_for_snapshot_on_connection`) was the lone read filtering `semantic_chunk.snapshot_id = ?2`. Rewrote the gate to join `semantic_head` and filter `head.covered_snapshot_id = ?2` (provider/model/language preserved), exactly mirroring the sibling reads (embeddings/payloads/previews/health). Chose the gate fix over rewriting chunk stamps because the rest of the codebase already treats `head.covered_snapshot_id` as authoritative for liveness. Added regression test `semantic_vector_topk_returns_unchanged_chunks_after_incremental_advance`; the existing `semantic_vector_topk_membership_filter_still_probes_each_candidate_row` still passes.

DEVANA-KEY: crates/cli/src/storage/semantic_store_read.rs:694 | P1 | semantic-advance-stale-snapshot-drops-chunks
DEVANA-SUMMARY: Status=fixed | P1 high crates/cli/src/storage/semantic_store_read.rs:694 - Vector top-k membership gate now scopes by head.covered_snapshot_id (like every sibling read) instead of chunk.snapshot_id, so unchanged chunks survive an incremental advance (regression test added).
