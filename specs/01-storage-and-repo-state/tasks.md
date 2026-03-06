# Tasks — 01-storage-and-repo-state

Meta:
- Spec: 01-storage-and-repo-state — Storage and Repository State
- Depends on: -
- Global scope:
  - crates/storage/
  - crates/cli/
  - contracts/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T003: Wire CLI operational commands for storage bootstrap (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/cli/) (depends: T001, T002)
  - Started_at: 2026-03-04T16:56:33Z
  - Completed_at: 2026-03-04T17:20:05Z
  - Completion note: Added deterministic `init` and `verify` CLI subcommands for per-repository storage bootstrap with non-zero failure exits; closure delayed until vector readiness path stabilized.
  - Validation result: `cargo run -p frigg -- init --workspace-root .` and `cargo run -p frigg -- verify --workspace-root .` passed.
- [x] T001: Implement storage migration framework and base schema (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/storage/) (depends: -)
  - Started_at: 2026-03-04T16:25:22Z
  - Completed_at: 2026-03-04T16:54:13Z
  - Completion note: Added migration-based storage initialization with schema version tracking, idempotent migration apply, and storage-level migration tests.
  - Validation result: `cargo test -p storage` and `CARGO_HOME=/tmp/cargo-home-frigg cargo check` passed.
- [x] T002: Add storage integrity verification API (owner: worker:019cb9aa-f5a4-7432-822d-ff92e6b1c83a) (scope: crates/storage/) (depends: T001)
  - Started_at: 2026-03-04T16:54:13Z
  - Completed_at: 2026-03-04T16:56:33Z
  - Completion note: Implemented `Storage::verify()` with schema/table/version checks and transaction-backed read/write probe plus verify-focused tests.
  - Validation result: `cargo test -p storage verify` passed.
- [x] T004: Document storage contract and invariants (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: contracts/) (depends: T001)
  - Started_at: 2026-03-04T16:56:33Z
  - Completed_at: 2026-03-04T16:59:32Z
  - Completion note: Added `contracts/storage.md` covering schema map, migration policy, local-first defaults, and failure-mode taxonomy.
  - Validation result: `test -f contracts/storage.md` and `rg -n "snapshot|manifest|provenance|migration" contracts/storage.md` passed.
