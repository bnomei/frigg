DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: wontfix
Location: crates/cli/src/indexer/manifest.rs:156 | Slug: manifest-metadata-short-circuit-stale-digest

# Changed-only manifest build reuses prior digest when only metadata matches

## Finding

`build_changed_only_with_hints_and_diagnostics` treats equality of `(path, size_bytes, mtime_ns)` as "unchanged" and copies the previous `FileDigest` without re-reading content. `metadata_matches_previous_digest` ignores `hash_blake3_hex`. Content can change in place without metadata movement, leaving a stale digest in the new manifest.

## Violated Invariant Or Contract

Persisted manifest `hash_blake3_hex` must reflect current file bytes. Changed-only indexing and metadata-only freshness validation must not treat metadata-equal files as content-unchanged without verifying the hash.

## Oracle

After `ReindexMode::ChangedOnly`, on-disk Blake3 of a path should match the stored manifest digest. `diff()` and `SemanticRefreshMode::ReuseExisting` depend on accurate digests.

## Counterexample

1. Full index stores `src/lib.rs` with digest `H_old`, size `S`, mtime `M`.
2. Rewrite file bytes in place preserving `S` and `M` (e.g. `touch -r`, editor preserving mtime).
3. Changed-only reindex without a dirty hint for that path.
4. `metadata_matches_previous_digest` matches → previous digest copied (`manifest.rs:156-158`).
5. `diff()` reports no change; semantic refresh may `ReuseExisting`; `validate_manifest_digests_for_root` also keys off metadata only.

## Why It Might Matter

Lexical, semantic, and projection layers can lag real file content while freshness reports Ready, until a metadata-changing edit or full reindex occurs.

## Proof

**Control-flow trace**

`metadata_matches_previous_digest` (path/size/mtime only) → `entries.push(previous.clone())` → skip `stream_file_blake3_digest` → stale hash in `current_manifest`.

**Contract mismatch**

`same_manifest_record` compares full digest; shortcut bypasses hash comparison entirely via `metadata_matches_previous_digest`.

## Counterevidence Checked

Normal editor saves usually bump mtime. `dirty_path_hints` bypass the shortcut. `ReindexMode::Full` always re-hashes. Tests cover changed-only when mtime changes but not metadata-equal/content-different cases.

## Suggested Next Step

Include `hash_blake3_hex` in the shortcut predicate, or always re-hash hinted and watch-dirty paths; optionally spot-check hashes during `validate_manifest_digests_for_root`.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: confirmed real but wontfix (intended tradeoff). `metadata_matches_previous_digest` (path/size/mtime_ns) gates the digest-reuse shortcut, so a file edited in place while preserving both size and mtime_ns keeps a stale `hash_blake3_hex` under a non-hinted changed-only build. This is the standard metadata-based change-detection heuristic (same as git/rsync-without-checksum): the only way to detect a same-metadata content change is to re-hash the bytes, which is exactly what `ReindexMode::Full` does and what changed-only mode exists to avoid — so "include hash_blake3_hex in the shortcut predicate" cannot be done without re-reading every file and collapsing changed-only into full-cost. Mitigations already in place and verified: (1) the shortcut is bypassed for `is_hinted` paths, and the watch supervisor passes `recent_paths` as dirty hints via `reindex_repository_with_runtime_config_and_dirty_paths`, so live edits are always re-hashed; (2) `ReindexMode::Full` always re-hashes; (3) normal editor saves bump mtime. The residual gap requires deliberately preserving size+mtime_ns (e.g. `touch -r`, archive restore with `--preserve`) AND a non-hinted CLI `reindex --changed`. Recommend interim guidance: run a full `reindex` after restoring files with preserved timestamps; if stronger guarantees are ever required, that is a deliberate perf/correctness decision (e.g. an opt-in `--verify-hashes` mode or probabilistic spot-checks), not a default-on change. No code change.

DEVANA-KEY: crates/cli/src/indexer/manifest.rs:156 | P2 | manifest-metadata-short-circuit-stale-digest
DEVANA-SUMMARY: Status=wontfix | P2 high crates/cli/src/indexer/manifest.rs:156 - Confirmed real but an intended metadata-based change-detection tradeoff: closing the same-size+mtime in-place-edit hole requires re-hashing every file (= full reindex). Watch dirty-hints already force re-hash of live edits; full reindex always re-hashes. Deferred to an opt-in verify mode if ever needed.