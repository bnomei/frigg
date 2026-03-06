# Design — 06-embeddings-and-vector-store

## Normative excerpt (from `docs/overview.md`)
- Selected first implementation: OpenAI + Google adapters behind shared provider interface.
- Vector persistence target: SQLite + sqlite-vec.
- Semantic layer must integrate with deterministic evidence/provenance.

## Architecture
- `crates/embeddings/` owns provider trait, adapters, request batching, and retry policy.
- `crates/storage/` owns vector table lifecycle and integrity checks.
- Provider selection/model wiring is caller-owned today (provider construction + `EmbeddingRequest.model`) and documented in `contracts/semantic.md` (not in `FriggConfig`).

## Reliability model
- Typed errors include provider, category, retryability, and request trace id.
- Startup self-check validates vector-store availability and expected dimensions policy, with sqlite-vec preferred and deterministic fallback support when unavailable.
