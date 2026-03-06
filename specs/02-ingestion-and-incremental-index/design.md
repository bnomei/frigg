# Design — 02-ingestion-and-incremental-index

## Normative excerpt (from `docs/overview.md`)
- Use ignore-aware recursive walk.
- Maintain `(path, size, mtime, hash)` manifest and diff old/new manifests.
- Index only changed files for incremental updates.

## Architecture
- `crates/index/` owns discovery, hashing, diff calculation, and ingest orchestration.
- `crates/storage/` persists snapshot and manifest state used for diffs.
- Fixture repositories for deterministic tests live in `fixtures/repos/`.

## Core flow
1. Discover files with `ignore::WalkBuilder`.
2. Build manifest with content hash and metadata.
3. Load prior manifest for snapshot/workspace.
4. Compute delta: added, modified, deleted.
5. Emit changed-file work queue for downstream indexers.

## Output contract
- Deterministic summary fields: `snapshot_id`, `files_scanned`, `files_changed`, `files_deleted`, `duration_ms`.
