DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/indexer/manifest.rs:162 | Slug: manifest-read-failure-false-deletion

# Transient manifest read failures classify existing files as deleted

## Finding

During `ChangedOnly` manifest builds, `stream_file_blake3_digest` failures emit a
`ManifestDiagnosticKind::Read` diagnostic and `continue`, omitting the file from
the new manifest entries. `diff()` then treats any path present in the old manifest
but missing from the new manifest as deleted. If the file still exists on disk,
`normalize_deleted_repository_relative_path` resolves it successfully, so
`has_unresolved_deleted_paths` stays false and semantic refresh proceeds in
`IncrementalAdvance` mode with that path in `deleted_paths`. Embeddings for a
still-present file are removed even though the failure was transient I/O, not an
actual deletion.

## Violated Invariant Or Contract

A file must not be treated as deleted from the repository index when the only
evidence is a digest-read failure and the path still exists on disk.

## Oracle

`build_changed_only_with_hints_and_diagnostics` documents read issues as
non-fatal diagnostics but still omits the entry (~162–170). `diff()` deletion
semantics (~402–405) key solely on manifest membership. `build_semantic_refresh_plan`
uses `has_unresolved_deleted_paths` only when normalized deleted count differs from
raw `manifest_diff.deleted.len()` (`semantic.rs` ~236), not when diagnostics
indicate read failures for would-be deleted paths.

## Counterexample

1. Repository has `src/lib.rs` indexed in snapshot A.
2. `ChangedOnly` refresh runs; transient `EACCES` or `EBUSY` on
   `stream_file_blake3_digest(src/lib.rs)` during manifest build.
3. Diagnostic `Read` recorded; `src/lib.rs` omitted from new entries.
4. `diff` marks `src/lib.rs` deleted; `normalize_deleted_repository_relative_path`
   succeeds because the file still exists.
5. `SemanticRefreshMode::IncrementalAdvance` calls
   `advance_semantic_embeddings_for_repository` with `deleted_paths` containing
   `src/lib.rs`.
6. Reindex returns `Ok(ReindexSummary)`; watch/MCP report success.

## Why It Might Matter

Semantic search can lose coverage for files that were never deleted, with only a
non-fatal diagnostic in `ReindexDiagnostics` that callers may ignore.

## Proof

**Dataflow trace:** digest read `Err` → entry omitted → `diff.deleted` →
normalized `deleted_paths` → `advance_semantic_embeddings_for_repository` delete
side effect.

**Control-flow trace:** `continue` on read error bypasses retaining the previous
digest for unchanged hinted/non-hinted files when re-hash fails.

## Counterevidence Checked

- `has_unresolved_deleted_paths` triggers full rebuild when deleted paths cannot
  be normalized (e.g. truly missing files), not when normalization succeeds for
  falsely deleted entries.
- Full `ReindexMode::Full` rebuild would recover, but default watch `ManifestFast`
  and MCP `ChangedOnly` paths use incremental semantics.
- Diagnostics are surfaced in summary but do not block persistence or semantic
  advance today.

## Suggested Next Step

On digest-read failure for a path present in the previous manifest, retain the
previous `FileDigest` (or fail the refresh) instead of omitting the entry. Treat
read failures paired with `manifest_diff.deleted` as forcing full semantic rebuild
or hard failure.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report written from static source inspection
  across `inside-out-paths`, `contracts-errors`, and `invariants-contracts` trails.
- 2026-06-27: fixed. Confirmed `build_changed_only_with_hints_and_diagnostics`
  (manifest.rs) `continue`d past a `stream_file_blake3_digest` failure, omitting the
  walked-but-unreadable file from the new entries; `diff()` then keyed deletion on
  manifest membership and marked the still-present file deleted, feeding
  `IncrementalAdvance` semantic refresh's `deleted_paths`. Fix: on digest-read
  failure, if the path existed in the previous manifest, retain that previous
  `FileDigest` instead of dropping it (the diagnostic is still recorded and the
  unchanged on-disk mtime forces a re-hash next refresh). A brand-new file with no
  previous entry is simply deferred — it was never in the old manifest so `diff()`
  cannot mark it deleted. Added `#[cfg(unix)]` regression test
  `changed_only_retains_previous_entry_when_digest_read_fails` (chmod 0o000 to force
  the read failure) asserting the entry is retained, a Read diagnostic is recorded,
  and `diff().deleted` stays empty. Full `indexer::tests::manifest` suite green
  (17 tests).

DEVANA-KEY: crates/cli/src/indexer/manifest.rs:162 | P1 | manifest-read-failure-false-deletion
DEVANA-SUMMARY: Status=fixed | P1 high crates/cli/src/indexer/manifest.rs:162 - ChangedOnly manifest digest-read failures dropped still-present files so diff marked them deleted and incremental semantic advance purged their embeddings; fixed by retaining the previous manifest entry on read failure plus a regression test.