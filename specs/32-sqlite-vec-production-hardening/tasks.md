# Tasks — 32-sqlite-vec-production-hardening

Meta:
- Spec: 32-sqlite-vec-production-hardening — sqlite-vec Production Hardening
- Depends on: 06-embeddings-and-vector-store, 21-vector-backend-migration-safety, 25-release-gate-execution-hardening
- Global scope:
  - crates/storage/, crates/embeddings/, crates/cli/, scripts/smoke-ops.sh, contracts/, docs/overview.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Enforce sqlite-vec registration and pinned-version readiness in storage runtime (owner: worker:019cbd37-60bd-7130-84af-cfe893293e21) (scope: crates/storage/, crates/embeddings/) (depends: -)
  - Started_at: 2026-03-05T08:57:48Z
  - Completed_at: 2026-03-05T09:06:51Z
  - Completion note: Added deterministic sqlite-vec readiness hardening in storage with pinned-version enforcement (including normalized `v` prefix handling) and expanded storage tests for mismatch/failure semantics while preserving migration safety behavior.
  - Validation result: Worker + mayor verified `cargo test -p storage` and `cargo test -p embeddings` both passed.
- [x] T002: Add strict startup and smoke validation for hardened sqlite-vec readiness (owner: worker:019cbd37-60bd-7130-84af-cfe893293e21) (scope: crates/cli/, scripts/smoke-ops.sh, contracts/, docs/overview.md) (depends: T001)
  - Started_at: 2026-03-05T09:08:53Z
  - Completed_at: 2026-03-05T09:14:19Z
  - Completion note: Enforced strict vector readiness gate on no-subcommand startup, added deterministic startup failure tests and smoke fallback verification, and synchronized storage/changelog/overview docs to mark startup hardening complete.
  - Validation result: Worker + mayor verified `cargo test -p frigg` and `bash scripts/smoke-ops.sh` passed.
