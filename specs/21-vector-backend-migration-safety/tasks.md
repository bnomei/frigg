# Tasks — 21-vector-backend-migration-safety

Meta:
- Spec: 21-vector-backend-migration-safety — Vector Backend Migration Safety
- Depends on: 01-storage-and-repo-state, 06-embeddings-and-vector-store
- Global scope:
  - crates/storage/, contracts/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Vector Backend Migration Safety and lock with regression coverage (owner: worker:019cbad0-8a9b-7731-b098-874c17c3c35a) (scope: crates/storage/, contracts/) (depends: -)
  - Started_at: 2026-03-04T21:46:03Z
  - Completed_at: 2026-03-04T21:52:42Z
  - Completion note: Added schema-first backend inference and deterministic transition guardrails (fallback stays fallback, sqlite-vec schema blocks implicit downgrade when unavailable) with transition regression tests.
  - Validation result: `cargo test -p storage` passed (14 tests) and `cargo test -p embeddings` passed (10 tests).
