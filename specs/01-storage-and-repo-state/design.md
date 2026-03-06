# Design — 01-storage-and-repo-state

## Normative excerpt (from `docs/overview.md`)
- Persist repository snapshots and manifests with deterministic IDs.
- Keep provenance/events replayable.
- Define operational workflows: `init`, `reindex`, `reindex --changed`, `verify`.

## Architecture
- `crates/storage/` owns schema creation, migration, and integrity checks.
- `crates/cli/` exposes `init` and `verify` commands that call storage APIs.
- Storage uses WAL mode and explicit tables for:
  - repository
  - snapshot
  - file_manifest
  - provenance_event
- `contracts/storage.md` documents schema intent and compatibility constraints.

## Data lifecycle
1. `init`: create/open DB, apply migrations, run integrity checks.
2. `verify`: validate schema presence, expected indices, and extension readiness.
3. `reindex*`: ingest pipeline updates `snapshot` + `file_manifest` and emits provenance rows.
