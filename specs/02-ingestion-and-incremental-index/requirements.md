# Requirements — 02-ingestion-and-incremental-index

## Scope
Implement file discovery, hashing, diffing, and changed-only indexing behavior.

## EARS requirements
- When a workspace is indexed, the ingestion subsystem shall respect `.gitignore`-style filtering.
- When reindex is requested, the ingestion subsystem shall hash discovered files and index only added/changed/deleted files.
- While indexing, the ingestion subsystem shall emit deterministic snapshot identifiers and provenance events.
- If a file cannot be read or parsed, then the ingestion subsystem shall continue indexing remaining files and report typed warnings.
- The ingestion subsystem shall provide `reindex` and `reindex --changed` execution paths with deterministic output summaries.
