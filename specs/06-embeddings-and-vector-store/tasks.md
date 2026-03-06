# Tasks — 06-embeddings-and-vector-store

Meta:
- Spec: 06-embeddings-and-vector-store — Semantic Provider and Vector Layer
- Depends on: 01-storage-and-repo-state
- Global scope:
  - crates/embeddings/
  - crates/storage/
  - contracts/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T003: Implement sqlite-vec integration and startup self-check (owner: worker:019cb9aa-fae5-7d13-a6e5-6b73c3f68600) (scope: crates/storage/, crates/embeddings/) (depends: T001)
  - Started_at: 2026-03-04T16:56:33Z
  - Completed_at: 2026-03-04T17:20:05Z
  - Completion note: Added sqlite-vec auto-registration readiness path, vector table lifecycle checks, and startup verification integration across storage/embeddings flows.
  - Validation result: `cargo test -p storage vector_store`, `cargo run -p frigg -- init --workspace-root .`, and `cargo run -p frigg -- verify --workspace-root .` passed.
- [x] T001: Implement provider trait contract and shared request model (owner: worker:019cb9aa-fae5-7d13-a6e5-6b73c3f68600) (scope: crates/embeddings/) (depends: -)
  - Started_at: 2026-03-04T16:25:22Z
  - Completed_at: 2026-03-04T16:34:22Z
  - Completion note: Reworked embeddings abstraction with stable provider trait surface, shared request/response models, and typed error taxonomy with retryability semantics.
  - Validation result: `cargo test -p embeddings provider_trait` passed (4 tests).
- [x] T002: Implement OpenAI and Google embedding adapters with retries (owner: worker:019cb9aa-fae5-7d13-a6e5-6b73c3f68600) (scope: crates/embeddings/) (depends: T001)
  - Started_at: 2026-03-04T16:34:22Z
  - Completed_at: 2026-03-04T16:43:08Z
  - Completion note: Implemented real OpenAI/Google adapter request flows with retry/backoff/timeout behavior and typed provider/transport error mapping.
  - Validation result: `cargo test -p embeddings provider_adapters` passed (4 tests).
- [x] T004: Document semantic config contract and failure behavior (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: contracts/) (depends: T001, T002)
  - Started_at: 2026-03-04T16:43:08Z
  - Completed_at: 2026-03-04T16:45:30Z
  - Completion note: Added semantic provider contract coverage for provider/model request handling, retry semantics, and failure taxonomy mapping; clarified that semantic provider/model selection is caller-owned and not part of `FriggConfig`.
  - Validation result: `rg -n "OPENAI|GEMINI|retry|embedding" contracts/config.md contracts/semantic.md` passed.
