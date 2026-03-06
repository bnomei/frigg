# Semantic Provider Contract (`v1`)

This document defines the runtime `embeddings` contract implemented in `crates/cli/src/embeddings/`.
It is a runtime/library contract, not a public MCP tool schema.

## Runtime composition contract

- Supported provider kinds: `openai`, `google`.
- `vector_store` is used for vector-store readiness verification errors.
- Semantic runtime options are composition-owned through `FriggConfig.semantic_runtime` (`crates/cli/src/settings/mod.rs`):
  - `enabled: bool` (default `false`)
  - `provider: Option<SemanticRuntimeProvider>` (`openai|google`)
  - `model: Option<String>`
  - `strict_mode: bool` (default `false`)
- Validation is deterministic:
  - when `enabled=false`, semantic runtime startup validation is skipped and existing MCP startup behavior is preserved.
  - when `enabled=true`, `provider` is required.
  - when `enabled=true`, `model` is optional; if omitted, the provider default is used.
  - when `enabled=true`, an explicitly provided `model` must not be blank after trim.
- Startup configuration inputs are explicit CLI/env composition keys:
  - `--semantic-runtime-enabled` / `FRIGG_SEMANTIC_RUNTIME_ENABLED`
  - `--semantic-runtime-provider` / `FRIGG_SEMANTIC_RUNTIME_PROVIDER`
  - `--semantic-runtime-model` / `FRIGG_SEMANTIC_RUNTIME_MODEL`
  - `--semantic-runtime-strict-mode` / `FRIGG_SEMANTIC_RUNTIME_STRICT_MODE`

## Provider request contract

- Runtime injects provider-specific default models when `semantic_runtime.model` is omitted:
  - `openai` -> `text-embedding-3-small`
  - `google` -> `gemini-embedding-001`
- `EmbeddingRequest.model` must be non-empty.
- Purpose values are `document` and `query`.
- Request validation is deterministic:
  - `model` must not be blank.
  - `input` must contain at least one value.
  - each `input` value must not be blank.
  - `dimensions`, when present, must be greater than `0`.

## Credentials and environment wiring

- OpenAI and Google providers require non-empty API keys at call time.
- Blank API key fails fast as a validation error on `api_key`.
- Semantic runtime startup credential gate requires:
  - `OPENAI_API_KEY` when `provider=openai`
  - `GEMINI_API_KEY` when `provider=google`
- Missing or blank required key fails startup deterministically before serving semantic-enabled requests.
- Semantic startup gate failure shape is deterministic:
  - error code: `invalid_params`
  - typed sources: semantic runtime config validation or semantic runtime credential validation

## Retry semantics

- Runtime retry policy defaults:
  - `max_retries = 2`
  - `initial_backoff = 200ms`
  - `max_backoff = 2s`
- Backoff is exponential (`initial_backoff * 2^retry_index`) and capped by `max_backoff`.
- Retryable HTTP statuses: `408`, `409`, `425`, `429`, `500`, `502`, `503`, `504`.
- Retryable transport failures: timeout/connect/body I/O failures from the HTTP transport layer.
- Google provider also treats `RESOURCE_EXHAUSTED`, `UNAVAILABLE`, `DEADLINE_EXCEEDED`, and `ABORTED` status names as retryable when present in upstream payloads.
- Non-retryable failures stop retry attempts immediately.

## Failure behavior and taxonomy alignment

Mapping to `contracts/errors.md` is deterministic at the semantic layer.

| Runtime error shape | Typical trigger | Canonical error code | Retryable |
| --- | --- | --- | --- |
| `EmbeddingError::Validation` | blank model/input/api key, invalid dimensions | `invalid_params` | `false` |
| `EmbeddingError::Provider` (`Retryable`) | upstream rate limit/temporary outage/retryable provider status | `rate_limited` or `unavailable` | `true` |
| `EmbeddingError::Provider` (`NonRetryable`) | unsupported model, hard auth/policy rejection, invalid upstream success payload | `invalid_params`, `access_denied`, or `internal` | `false` |
| `EmbeddingError::Transport` (`Retryable`) | retryable network timeout/connect/body failures | `timeout` or `unavailable` | `true` |
| `EmbeddingError::Transport` (`NonRetryable`) | non-retryable transport failure, local vector-store readiness failure | `internal` | `false` |

Error payload invariants:
- Provider failures carry `provider`, optional `status_code`, optional provider `code`, `retryability`, and optional `trace_id`.
- Transport failures carry `provider`, `operation`, `retryability`, and optional `trace_id`.
- Semantic startup gate failures carry deterministic startup metadata in CLI summary lines (`semantic_code`, `semantic_provider`, `semantic_model`) and abort startup.

## Response and trace metadata

- Successful responses include `provider`, `model`, and `vectors`, with optional `usage`.
- `trace_id` is pass-through: when present on request, it is propagated to response and provider/transport failures.
- Query-time hybrid retrieval uses a lean semantic projection contract:
  - ranking loads embedding rows without `content_text` payloads;
  - excerpt text is fetched only for the final selected semantic hits returned to the caller.
- Changed-only semantic reindex is incremental at the storage layer:
  - unchanged chunk rows are advanced to the new snapshot without rewriting their embeddings;
  - changed and deleted repository-relative paths are replaced deterministically in the active snapshot.
- Vector-store readiness helpers:
  - `verify_vector_store_readiness(...)` validates storage-backed vector readiness (default dimensions: `1536` when unspecified).
  - `verify_sqlite_vec_readiness(...)` additionally enforces strict sqlite-vec readiness and fails for unavailable extension registration or non-sqlite-vec on-disk schemas.
