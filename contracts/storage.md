# Storage Contract (`v1`)

This document defines the storage/runtime contract implemented by `crates/cli/src/storage/` and consumed by index/provenance paths.

## 1) Schema map (tables and intent)

Storage backend: local SQLite.

| Table | Primary key | Core columns | Contract intent |
| --- | --- | --- | --- |
| `schema_version` | `id` (`CHECK id = 1`) | `version`, `updated_at` | Tracks applied schema `migration` version. |
| `repository` | `repository_id` | `root_path`, `display_name`, `created_at` | Registers local repository roots. |
| `snapshot` | `snapshot_id` | `repository_id`, `kind`, `revision`, `created_at` | Stores deterministic repository `snapshot` identity for replay/diff workflows. Manifest snapshots remain the lexical/path-witness source of truth and semantic heads point at the manifest snapshot currently covered by the live semantic corpus. |
| `file_manifest` | `(snapshot_id, path)` | `sha256`, `size_bytes`, `mtime_ns` | Stores per-`snapshot` file `manifest` records for incremental diffing. |
| `path_witness_projection` | `(repository_id, snapshot_id, path)` | `path_class`, `source_class`, `path_terms_json`, `flags_json`, `created_at` | Stores snapshot-scoped path/surface witness projections used by hybrid ranking and pruned alongside obsolete manifest snapshots. |
| `provenance_event` | `(trace_id, tool_name, created_at)` | `payload_json` | Stores replayable tool-call `provenance` events. |
| `semantic_head` | `(repository_id, provider, model)` | `covered_snapshot_id`, `live_chunk_count`, `last_refresh_reason`, `created_at`, `updated_at` | Tracks the active live semantic corpus for a `(repository, provider, model)` tuple and the manifest snapshot it currently covers. |
| `semantic_chunk` | `(repository_id, provider, model, chunk_id)` | `snapshot_id`, `path`, `language`, `chunk_index`, `start_line`, `end_line`, `content_hash_blake3`, `content_text`, `created_at`, `updated_at` | Stores shared live semantic chunk metadata and chunk text once per logical chunk in the active `(repository, provider, model)` corpus instead of partitioning steady-state rows by historical snapshots. |
| `semantic_chunk_embedding` | `(repository_id, provider, model, chunk_id)` | `snapshot_id`, `trace_id`, `embedding_blob`, `dimensions`, `created_at`, `updated_at` | Stores lean provider/model-specific live embeddings without duplicating shared chunk text. Older semantic snapshot fallback is not part of the steady-state contract. |

### Vector subsystem tables

`Storage::initialize()` initializes vector storage with expected dimensions `1536` by default:

- Required backend: virtual table `embedding_vectors` via `vec0(embedding float[<dims>])`.
- Canonical semantic rows remain `semantic_head` + `semantic_chunk` + `semantic_chunk_embedding`; `embedding_vectors` is a derived sqlite-vec live projection used for local top-k retrieval.
- Projection rows are keyed to the live `(repository, provider, model)` corpus and are not partitioned by historical manifest snapshot.
- Projection rebuild may pad shorter canonical embeddings up to `1536` dimensions for sqlite-vec compatibility, but canonical embedding reads continue to return the stored row dimensions unchanged.
- Explicit vector repair via `repair_semantic_vector_store()` drops/recreates `embedding_vectors` and rebuilds it from the live semantic corpus.
- Semantic top-k queries clamp requested `k` to sqlite-vec's local maximum (`4096`) instead of widening the public API or silently switching backends.

### Required-table invariant

`verify` must fail if any required table is missing:

- `schema_version`
- `repository`
- `snapshot`
- `file_manifest`
- `path_witness_projection`
- `provenance_event`
- `semantic_head`
- `semantic_chunk`
- `semantic_chunk_embedding`

## 2) Migration policy

- Schema changes are versioned and ordered by monotonically increasing integer version.
- Each `migration` executes in one transaction; partial apply is not allowed.
- On successful `migration`, `schema_version.version` is updated in the same transaction.
- `initialize` is idempotent: rerunning it must not duplicate schema metadata or corrupt existing rows.
- `verify` enforces exact schema version match with latest known `migration`; mismatch is a hard failure.
- Current latest schema version is `6`.
- Backward-incompatible schema changes require contract update and an explicit migration note in this document.

### Vector backend transition policy

- sqlite-vec is mandatory for `initialize_vector_store`/`verify_vector_store`; fallback backend modes are not supported.
- Existing non-sqlite-vec vector schemas are rejected with deterministic errors and require operator migration (reset/reinit).
- Backend transitions must not silently rewrite/drop vector tables during readiness verification.

## 3) Local-first defaults

- Primary state is local disk SQLite; no remote database is required by default.
- Connection initialization sets `PRAGMA journal_mode = WAL` for local durability/concurrency.
- Operational contract:
  - `init` creates or opens the local DB and applies pending migrations.
  - `verify` checks required tables, schema version, repository read/write probe behavior, and vector subsystem readiness.
  - Server startup path (no CLI subcommand) runs strict vector readiness checks before serving MCP; startup aborts unless active backend is `sqlite_vec`.
  - `reindex` and `reindex --changed` persist `snapshot` + `file_manifest` + `path_witness_projection` state via `indexer::reindex_repository`.
  - When semantic runtime is enabled, semantic storage is advanced as one live corpus per `(repository, provider, model)` keyed by `semantic_head`; changed-only refreshes update only changed/deleted semantic rows instead of maintaining steady-state snapshot-partitioned semantic corpora.
  - Older semantic snapshot fallback is removed. If the active provider/model has no live semantic corpus covering the current manifest snapshot, runtime health reports that missing coverage directly instead of consulting older semantic partitions.
  - Successful reindex prunes manifest snapshots to the latest `8` by default while protecting any snapshot still referenced by an active `semantic_head.covered_snapshot_id`.
  - Reindex walker/read failures for individual files are non-fatal; reindex continues and emits typed deterministic diagnostics (`walk`/`read`) in command summaries.
  - Provenance rows are appended by MCP tool handlers (`list_repositories`, `read_file`, `search_text`, `search_symbol`, `find_references`), not by `reindex` itself, and retention is bounded to the latest `10_000` events by default.

## 4) Deterministic ordering, live-corpus, and retention semantics

- Manifest writes are normalized by path ordering before insert.
- `load_manifest_for_snapshot(...)` returns entries ordered by `path ASC`.
- Latest snapshot lookup is deterministic: `ORDER BY created_at DESC, snapshot_id DESC`.
- Active semantic reads are resolved through `semantic_head` for the current `(repository_id, provider, model)` tuple.
- Older semantic snapshot fallback is not supported; missing active-model coverage is surfaced directly as missing semantic readiness instead of widening reads to stale semantic partitions.
- `reindex --changed` with zero additions/modifications/deletions reuses the previous `snapshot_id`.
- Manifest snapshot pruning is deterministic: snapshots are ordered by `created_at DESC, rowid DESC`, the latest `8` are retained by default, and any snapshot referenced by an active `semantic_head` is protected from deletion even when it falls outside that bound.
- Provenance reads are deterministic by `ORDER BY created_at DESC, trace_id DESC` with caller-supplied `limit`.
- Provenance pruning is deterministic by `ORDER BY created_at DESC, rowid DESC` and retains the latest `10_000` events by default.

## 5) Failure-mode taxonomy

Storage failures must map to `contracts/errors.md` canonical codes.

| Failure condition | Canonical error code | Retry guidance | Notes |
| --- | --- | --- | --- |
| Invalid storage input (bad path/invalid params) | `invalid_params` | Do not retry unchanged | Caller/config fix required. |
| Required table missing during verify | `internal` | Non-retryable until repaired | Indicates schema drift or corruption. |
| Schema version mismatch | `internal` | Non-retryable until migration/init is run | Service should abort serving tool requests. |
| Transaction failure during migration apply/commit | `internal` | Retry only after operator action | Migration state must be inspected before retry. |
| Invalid vector dimensions input (`expected_dimensions <= 0`) | `invalid_params` | Do not retry unchanged | Caller must provide a positive `expected_dimensions` value. |
| Vector subsystem readiness mismatch/missing table | `internal` | Non-retryable until repaired | Includes on-disk vector schema/metadata mismatches and missing vector readiness tables. |
| Live semantic rows and sqlite-vec projection diverge | `internal` | Non-retryable until repaired | MCP/workspace health may report `semantic_vector_partition_out_of_sync`; use storage repair to rebuild `embedding_vectors` from the live semantic corpus. |
| sqlite-vec extension unavailable or not registered | `internal` | Non-retryable until sqlite-vec extension registration is restored | Runtime requires sqlite-vec FFI auto-extension registration and pinned version checks. |
| Legacy non-sqlite-vec vector schema detected | `internal` | Non-retryable until operator migration/reset | Runtime rejects fallback-style `embedding_vectors` schemas and requires sqlite-vec virtual table provisioning via `frigg init`. |
| Temporary SQLite lock/unavailable local file handle | `internal` | Context-dependent | Current runtime surfaces these as `FriggError::Internal`. |
| Individual file traversal/read failures during reindex | _not a hard failure_ | Continue and inspect diagnostics | Reindex summaries expose deterministic typed `walk`/`read` diagnostics instead of failing the full run. |
| Snapshot/manifest lookup misses for requested id/path | `resource_not_found` | Retry only with corrected id/path | Exposed by caller/tool layer when applicable. |
| Provenance write/read serialization failure | `internal` | Non-retryable for same payload | Requires code/data fix. |
