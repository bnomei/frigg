# Requirements — 06-embeddings-and-vector-store

## Scope
Implement semantic provider abstraction and local vector persistence.

## EARS requirements
- The semantic subsystem shall expose a provider abstraction for embeddings independent of provider-specific API details.
- Where provider mode is enabled, the semantic subsystem shall support OpenAI and Google embedding providers.
- When embeddings are persisted, the semantic subsystem shall store vectors in local SQLite-backed vector storage.
- If provider requests fail, then the semantic subsystem shall return typed retryable/non-retryable errors.
- While semantic retrieval is used, the system shall preserve deterministic provenance of provider/model/chunk identifiers.
