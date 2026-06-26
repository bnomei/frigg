DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/embeddings/google.rs:215 | Slug: google-error-code-overwrites-retryability

# Google provider error `code` field overwrites retryable gRPC status classification

## Finding

When parsing Google error envelopes, retryability is first set from gRPC-style `status` strings like `UNAVAILABLE`, then unconditionally overwritten by `status_retryability(envelope.error.code)`. A numeric `code` of 400 on a 503/`UNAVAILABLE` response becomes `NonRetryable`, skipping embedding retries.

## Violated Invariant Or Contract

Retryability must follow the effective transient failure signal. Embedding transport retry contract expects `RESOURCE_EXHAUSTED`, `UNAVAILABLE`, and similar statuses to be retryable regardless of a conflicting numeric code in the JSON body.

## Oracle

`status_retryability` maps 503/429 to retryable (`transport.rs`). Google error handling should not downgrade transient outages to permanent failure.

## Counterexample

HTTP 503 with body `{"error":{"status":"UNAVAILABLE","code":400,"message":"backend overloaded"}}`.

1. Line 210 sets `Retryability::Retryable` from `UNAVAILABLE`.
2. Line 215-216 overwrites with `status_retryability(400)` → `NonRetryable`.
3. `embed_with_retry` performs a single attempt with no backoff.

## Why It Might Matter

Transient Google embedding outages during reindex or semantic refresh fail immediately instead of retrying, leaving semantic indexes stale or attach-time indexing incomplete.

## Proof

**Control-flow trace**

`status_retryability(status_code)` → gRPC status branch sets Retryable → `provider_status_code` branch overwrites unconditionally.

**Counterexample value**

`status="UNAVAILABLE"`, `code=400`, HTTP 503.

## Counterevidence Checked

Pure HTTP 429/503 without conflicting body still retryable. Tests cover display strings, not overwrite ordering (`embeddings/tests.rs`). OpenAI path uses different error parsing.

## Suggested Next Step

Apply numeric `code` only when gRPC `status` is absent, or prefer retryable classification when HTTP status and gRPC status disagree.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.

## Status Notes

- 2026-06-26: fixed. Confirmed `map_provider_http_error` set retryable from the gRPC `status` string then unconditionally overwrote it with `status_retryability(envelope.error.code)`, so an `UNAVAILABLE`/503 body carrying `code:400` became NonRetryable. Added a `retryable_from_grpc_status` flag; the numeric-code classification now runs only when a transient gRPC status has NOT already marked the failure retryable. This preserves the desirable upgrade case (a non-transient status with a 429/503 code still becomes retryable) while preventing the downgrade. Added regression test `provider_adapters_google_keeps_transient_status_retryable_despite_conflicting_code` (503 + UNAVAILABLE + code 400 → Retryable). Passes.

DEVANA-KEY: crates/cli/src/embeddings/google.rs:215 | P2 | google-error-code-overwrites-retryability
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/embeddings/google.rs:215 - Numeric error.code no longer downgrades a retryable gRPC status; transient UNAVAILABLE/RESOURCE_EXHAUSTED outages stay retryable (regression test added).