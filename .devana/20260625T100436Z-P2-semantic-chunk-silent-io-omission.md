DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/indexer/semantic.rs:329 | Slug: semantic-chunk-silent-io-omission

# Semantic chunk candidate build silently omits files on I/O failure

## Finding

During full semantic rebuild, `build_semantic_chunk_candidates` returns `Ok(Vec::new())` when `File::open` or `read_to_string` fails for a manifest entry, with no diagnostic. Reindex can commit successfully while the semantic corpus omits semantic-eligible paths that remain in the manifest.

## Violated Invariant Or Contract

Full semantic rebuild must embed every manifest entry that `semantic_chunk_language_for_path` accepts, or fail the refresh with a diagnostic. Silent omission creates manifest↔semantic corpus mismatch.

## Oracle

After successful full semantic replace/advance, each manifest path with a semantic-eligible language should have `semantic_chunk` rows, or the refresh should error. Symbol extraction records I/O failures in diagnostics (`symbols/extraction.rs`).

## Counterexample

1. Manifest includes `src/lib.rs` with a semantic-eligible language.
2. During `build_semantic_chunk_candidates`, `File::open` or `read_to_string` fails (permissions, transient lock).
3. Function returns `Ok(Vec::new())` at `semantic.rs:329-334` with no diagnostic.
4. `replace_semantic_embeddings_for_repository` commits; semantic search cannot recall that file while manifest still lists it.

## Why It Might Matter

Important files can disappear from semantic and hybrid retrieval after an otherwise "successful" reindex, with no operator-visible error. Incremental `ChangedOnly` will not revisit unchanged manifest entries.

## Proof

**Dataflow trace**

Manifest entry → `build_semantic_chunk_candidates` I/O error → empty vec (success) → zip with embeddings → storage commit → search miss.

**Cross-entry mismatch**

Manifest walk emits `ManifestBuildDiagnostic` on read failure (`manifest.rs:164-170`); semantic second pass swallows the same failures.

## Counterevidence Checked

Unsupported languages correctly return empty at `semantic.rs:323-324`. Non-semantic paths are intentionally skipped. Failure needs semantic-eligible path with I/O error during an otherwise successful refresh.

## Suggested Next Step

Mirror manifest diagnostic behavior: propagate I/O errors or emit `ManifestBuildDiagnostic`-style entries and fail the semantic refresh when any eligible path cannot be read.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed (observability). Confirmed `build_semantic_chunk_candidates` returned `Ok(Vec::new())` on both `File::open` and `read_to_string` failure with no signal, so a semantic-eligible manifest path could silently vanish from the corpus after a "successful" reindex. Both branches now emit a structured `warn!` (repository_id, path, error) before skipping. Deliberately chose warn-and-skip over the report's "fail the refresh" alternative: `read_to_string` also fails on non-UTF-8 content (a code-extensioned file with invalid bytes is in the manifest because manifest hashing is byte-based), and `File::open` can fail on a delete race between the manifest and semantic passes — failing the entire semantic refresh on one such file would be a worse regression than the original bug. The fix removes the *silent* part of the omission (operator-visible warning) without threading a new diagnostics channel through the return type. Compiles clean; no unit test added (log-emission assertions need a tracing-subscriber capture harness and add little value for a skip+warn change).

DEVANA-KEY: crates/cli/src/indexer/semantic.rs:329 | P2 | semantic-chunk-silent-io-omission
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/indexer/semantic.rs:329 - Per-file open/read failures in semantic chunk build now emit a structured warning instead of silently dropping the file; chose warn+skip over fail-refresh to avoid breaking the whole index on a non-UTF-8/deleted file.