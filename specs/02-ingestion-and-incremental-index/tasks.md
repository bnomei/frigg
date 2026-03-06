# Tasks — 02-ingestion-and-incremental-index

Meta:
- Spec: 02-ingestion-and-incremental-index — Ingestion and Incremental Index
- Depends on: 01-storage-and-repo-state
- Global scope:
  - crates/index/
  - crates/storage/
  - fixtures/repos/

## In Progress

- (none)

## Blocked

- (none)

## Todo

## Done

- [x] T003: Implement `reindex` and `reindex --changed` execution pipeline (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/index/, crates/cli/) (depends: T001, T002)
  - Started_at: 2026-03-04T17:21:40Z
  - Completed_at: 2026-03-04T19:03:53Z
  - Completion note: Added shared reindex entrypoints with full/changed modes and CLI wiring for deterministic operational summaries.
  - Validation result: `cargo run -p frigg -- reindex --workspace-root .`, `cargo run -p frigg -- reindex --changed --workspace-root .`, and `cargo test -p indexer incremental_roundtrip_changed_only_detects_modified_added_and_deleted_files` passed.
- [x] T002: Persist and load manifests through storage layer (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/storage/, crates/index/) (depends: T001)
  - Started_at: 2026-03-04T17:17:22Z
  - Completed_at: 2026-03-04T17:21:40Z
  - Completion note: Added manifest persistence/load APIs in storage and indexer-side manifest store wiring with roundtrip/incremental tests.
  - Validation result: `cargo test -p storage manifest` and `cargo test -p indexer incremental_roundtrip` passed.
- [x] T001: Implement manifest snapshot model and delta calculator (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/index/) (depends: -)
  - Started_at: 2026-03-04T17:09:38Z
  - Completed_at: 2026-03-04T17:13:31Z
  - Completion note: Added deterministic manifest-diff API (`diff`) over extended file manifest records including `mtime_ns`, with stable ordering and classification tests.
  - Validation result: `cargo test -p indexer manifest_diff` passed (3 tests).
- [x] T004: Add ingestion fixtures and determinism tests (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: fixtures/repos/, crates/index/) (depends: T001)
  - Started_at: 2026-03-04T17:13:53Z
  - Completed_at: 2026-03-04T17:17:22Z
  - Completion note: Added deterministic fixture repository and determinism-focused manifest builder tests validating stable repeated output and ignore filtering.
  - Validation result: `cargo test -p indexer determinism` passed twice with identical results.
