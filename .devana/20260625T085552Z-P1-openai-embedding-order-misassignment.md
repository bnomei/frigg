DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/embeddings/openai.rs:199 | Slug: openai-embedding-order-misassignment

# OpenAI embedding response consumed by wire order, ignoring `index`, mis-assigns vectors to chunks

## Finding

The OpenAI embeddings adapter parses the `data[]` array in wire (array) order and
discards each element's `index` field when it never re-sorts by it. The semantic
indexer then pairs the returned vectors with the input chunks positionally
(`batch.iter().zip(vectors)`). The OpenAI `/v1/embeddings` contract does not
guarantee that `data[]` is returned in input order — each element carries an
`index` precisely so callers can re-associate. If the provider returns the batch
in any order other than ascending input order, every chunk in the batch is stored
with another chunk's embedding. The length-equality guard does not catch this
because the counts still match.

## Violated Invariant Or Contract

The i-th persisted embedding must be the embedding of the i-th input chunk.
The reassociation key is the response element's `index`, not its array position.

## Oracle

- OpenAI `/v1/embeddings` API: response `data` objects must be matched to inputs
  by their `index` field; array order is not guaranteed.
- The code's own model encodes `index: usize` per returned vector
  (`crates/cli/src/embeddings/mod.rs` `EmbeddingVector.index`; captured at
  `openai.rs:203`), acknowledging position is not authoritative — yet it is never
  read again (`rg` finds only the capture site, zero use sites).

## Counterexample

Batch of 3 chunks `[A, B, C]` (real batches are up to
`SEMANTIC_EMBEDDING_BATCH_SIZE = 24`, `crates/cli/src/indexer/mod.rs:78`).
Provider returns, per spec-legal ordering:
`data = [{index:2, embedding:Vc}, {index:0, embedding:Va}, {index:1, embedding:Vb}]`.
- `parse_success_response` builds `vectors = [Vc, Va, Vb]` (wire order).
- Indexer flattens to `[Vc, Va, Vb]` and `zip`s with `[A, B, C]`.
- Length check `vectors.len() == batch.len()` passes (3 == 3).
- Stored: A→Vc, B→Va, C→Vb. Every embedding is mis-paired.

## Why It Might Matter

Silent, persisted corruption of the semantic vector store: chunks are searchable
under the wrong embedding, degrading semantic search relevance with no error
surfaced. Provider-dependent and intermittent (real OpenAI commonly returns
ascending order), which makes it hard to notice and hard to attribute — hence P1
rather than P0.

## Proof

Dataflow trace:
- `crates/cli/src/embeddings/openai.rs:199-206` — `parsed.data.into_iter().map(...)`
  preserves array order; `item.index` is copied into `EmbeddingVector.index` but
  not used to order the collection.
- `crates/cli/src/indexer/semantic.rs:135-139` —
  `response.vectors.into_iter().map(|vector| vector.values).collect()` drops `index`.
- `crates/cli/src/indexer/semantic.rs:268` —
  `for (chunk, embedding) in batch.iter().zip(vectors)` binds positionally, then
  `:282-297` persists `chunk_id`/`path`/`lines` from `chunk` with `embedding`.

Contract mismatch: response carries `index` reassociation key; consumer uses array
position.

## Counterevidence Checked

- Length guard (`indexer/semantic.rs:260`): equal counts under reordering, no help.
- Empty / non-finite vector guards (`:269-280`): vectors are valid, just mis-paired.
- Google adapter is NOT affected: `batchEmbedContents` returns embeddings
  positionally and `google.rs` assigns `index` from enumeration order consistent
  with input order.
- Query path is NOT affected: `searcher/semantic.rs` embeds a single-element input
  and asserts `len()==1`, so reordering is harmless there. Only the document
  indexing batch path corrupts.

## Suggested Next Step

Before discarding `index`, reorder `parsed.data` by `item.index` (or place each
`item.embedding` into a pre-sized `Vec` at position `item.index`, validating the
indices form `0..n`) in `parse_success_response`.

## Status Notes

- 2026-06-26: fixed. Confirmed `parse_success_response` built vectors in wire order and the per-item `index` was captured but never read, while the indexer pairs positionally. Rewrote the parser to place each `item.embedding` into a pre-sized slot at `item.index`, erroring on out-of-range, repeated, or missing indices so the result is always a complete `0..n` permutation with `EmbeddingVector.index == position`. Added tests: reordering a reversed `data[]` and rejecting a repeated/incomplete index set. All openai tests pass.

DEVANA-KEY: crates/cli/src/embeddings/openai.rs:199 | P1 | openai-embedding-order-misassignment
DEVANA-SUMMARY: Status=fixed | P1 high crates/cli/src/embeddings/openai.rs:199 - OpenAI embedding responses now re-associated by `index` into a validated 0..n permutation instead of consumed by array position (tests added).
