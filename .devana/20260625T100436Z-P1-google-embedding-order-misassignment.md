DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: invalid
Location: crates/cli/src/embeddings/google.rs:265 | Slug: google-embedding-order-misassignment

# Google batch embeddings assigned by response position, not request index

## Finding

The Google embedding adapter builds `EmbeddingVector.index` from `enumerate()` over the response array, not from any provider order field. The semantic indexer then strips `.index` and zips vectors to chunks by position. A spec-legal out-of-order Google response silently stores every chunk's vector against the wrong chunk.

## Violated Invariant Or Contract

Batch embedding vector *i* must correspond to request input *i*. Downstream storage keys chunks by content hash and path, so mis-pairing corrupts the semantic index without surfacing an error.

## Oracle

Google batch embedding API returns an `embeddings` array; vectors must be matched to inputs by position or explicit index. The OpenAI adapter has the same class of bug (separate report). Semantic ingest at `indexer/semantic.rs:135-139` expects positional correspondence after stripping indices.

## Counterexample

1. Batch request with texts `["text-A", "text-B"]`.
2. Google returns HTTP 200 with `embeddings` length 2 in reversed order.
3. `parse_success_response` assigns `index` 0/1 via `enumerate()` (`google.rs:265-279`).
4. `RuntimeSemanticEmbeddingExecutor` maps `vector.values` in iterator order, discarding `.index` (`semantic.rs:135-139`).
5. `build_semantic_embedding_records` zips chunks to vectors by position.
6. Reindex commits successfully with swapped vectors for A and B.

## Why It Might Matter

Semantic search and hybrid retrieval return wrong excerpts for otherwise correct natural-language queries whenever the provider returns a permuted batch. The failure is silent and persists until a full re-embed.

## Proof

**Dataflow trace**

Request texts → Google HTTP 200 → `parse_success_response` positional `enumerate` → `EmbeddingVector { index, values }` → executor strips index → `zip(chunks, vectors)` → SQLite semantic store.

## Counterevidence Checked

Empty or length-mismatched responses error (`google.rs:254-261`, `semantic.rs:260-265`). Only array length is validated, not index correspondence. Distinct from the OpenAI ordering bug at `openai.rs:199` (separate provider path).

## Suggested Next Step

Sort or validate Google response vectors against request indices (if the API exposes them), or reorder by a stable per-item key before zipping with chunks; add a mock test with permuted response order.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: invalid. The premise (a "spec-legal out-of-order Google response") does not hold for `batchEmbedContents`. The response payload `GoogleEmbeddingPayload` (google.rs:58-62) carries only `values`/`embedding` — there is no per-item `index` field, because Google's `models.batchEmbedContents` contract returns the `embeddings[]` array in the same order as the `requests[]` array. Positional `enumerate()` (google.rs:265) is therefore the correct and only possible association; there is no key to reorder by. This is the documented distinction from OpenAI's `/v1/embeddings`, which includes `index` precisely because its array order is not guaranteed (that one was a real bug, fixed separately). Length mismatches (fewer/more embeddings than inputs) are not silent: they are caught downstream by the `vectors.len() == batch.len()` guard in indexer/semantic.rs. No code change.

DEVANA-KEY: crates/cli/src/embeddings/google.rs:265 | P1 | google-embedding-order-misassignment
DEVANA-SUMMARY: Status=invalid | P1 high crates/cli/src/embeddings/google.rs:265 - Google batchEmbedContents returns embeddings in request order and the response has no index field, so positional enumerate() assignment is contractually correct; not a real ordering bug (unlike OpenAI).