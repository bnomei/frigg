# Storage Contract (`v1`)

This document defines the storage/runtime contract implemented by `crates/cli/src/storage/` and consumed by index/provenance paths.

## 1) Schema map (tables and intent)

Storage backend: local SQLite.

| Table | Primary key | Core columns | Contract intent |
| --- | --- | --- | --- |
| `schema_version` | `id` (`CHECK id = 1`) | `version`, `updated_at` | Tracks applied schema `migration` version. |
| `repository` | `repository_id` | `root_path`, `display_name`, `created_at` | Registers local repository roots. |
| `snapshot` | `snapshot_id` | `repository_id`, `kind`, `revision`, `created_at` | Stores deterministic repository `snapshot` identity for replay/diff workflows. |
| `file_manifest` | `(snapshot_id, path)` | `sha256`, `size_bytes`, `mtime_ns` | Stores per-`snapshot` file `manifest` records for incremental diffing. |
| `provenance_event` | `(trace_id, tool_name, created_at)` | `payload_json` | Stores replayable tool-call `provenance` events. |
| `semantic_chunk` | `(repository_id, snapshot_id, chunk_id)` | `path`, `language`, `chunk_index`, `start_line`, `end_line`, `content_text`, `created_at` | Stores shared per-`snapshot` semantic chunk metadata and chunk text exactly once per logical chunk. |
| `semantic_chunk_embedding` | `(repository_id, snapshot_id, chunk_id, provider, model)` | `trace_id`, `content_hash_blake3`, `embedding_blob`, `dimensions`, `created_at` | Stores lean provider/model-specific semantic embeddings without duplicating shared chunk text. |

### Vector subsystem tables

`Storage::initialize()` initializes vector storage with expected dimensions `1536` by default:

- Required backend: virtual table `embedding_vectors` via `vec0(embedding float[<dims>])`.
- Canonical semantic rows remain `semantic_chunk` + `semantic_chunk_embedding`; `embedding_vectors` is a derived sqlite-vec projection used for local top-k retrieval.
- Projection rebuild may pad shorter canonical embeddings up to `1536` dimensions for sqlite-vec compatibility, but canonical embedding reads continue to return the stored row dimensions unchanged.
- Semantic top-k queries clamp requested `k` to sqlite-vec's local maximum (`4096`) instead of widening the public API or silently switching backends.

### Required-table invariant

`verify` must fail if any required table is missing:

- `schema_version`
- `repository`
- `snapshot`
- `file_manifest`
- `provenance_event`
- `semantic_chunk`
- `semantic_chunk_embedding`

## 2) Migration policy

- Schema changes are versioned and ordered by monotonically increasing integer version.
- Each `migration` executes in one transaction; partial apply is not allowed.
- On successful `migration`, `schema_version.version` is updated in the same transaction.
- `initialize` is idempotent: rerunning it must not duplicate schema metadata or corrupt existing rows.
- `verify` enforces exact schema version match with latest known `migration`; mismatch is a hard failure.
- Current latest schema version is `4`.
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
  - `reindex` and `reindex --changed` persist `snapshot` + `file_manifest` state via `indexer::reindex_repository`.
  - Reindex walker/read failures for individual files are non-fatal; reindex continues and emits typed deterministic diagnostics (`walk`/`read`) in command summaries.
  - Provenance rows are appended by MCP tool handlers (`list_repositories`, `read_file`, `search_text`, `search_symbol`, `find_references`), not by `reindex` itself.

## 4) Deterministic ordering and snapshot semantics

- Manifest writes are normalized by path ordering before insert.
- `load_manifest_for_snapshot(...)` returns entries ordered by `path ASC`.
- Latest snapshot lookup is deterministic: `ORDER BY created_at DESC, snapshot_id DESC`.
- `reindex --changed` with zero additions/modifications/deletions reuses the previous `snapshot_id`.
- Provenance reads are deterministic by `ORDER BY created_at DESC, trace_id DESC` with caller-supplied `limit`.

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
| sqlite-vec extension unavailable or not registered | `internal` | Non-retryable until sqlite-vec extension registration is restored | Runtime requires sqlite-vec FFI auto-extension registration and pinned version checks. |
| Legacy non-sqlite-vec vector schema detected | `internal` | Non-retryable until operator migration/reset | Runtime rejects fallback-style `embedding_vectors` schemas and requires sqlite-vec virtual table provisioning via `frigg init`. |
| Temporary SQLite lock/unavailable local file handle | `internal` | Context-dependent | Current runtime surfaces these as `FriggError::Internal`. |
| Individual file traversal/read failures during reindex | _not a hard failure_ | Continue and inspect diagnostics | Reindex summaries expose deterministic typed `walk`/`read` diagnostics instead of failing the full run. |
| Snapshot/manifest lookup misses for requested id/path | `resource_not_found` | Retry only with corrected id/path | Exposed by caller/tool layer when applicable. |
| Provenance write/read serialization failure | `internal` | Non-retryable for same payload | Requires code/data fix. |
