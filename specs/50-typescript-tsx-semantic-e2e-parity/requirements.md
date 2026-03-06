# Requirements — 50-typescript-tsx-semantic-e2e-parity

## Goal
Close TypeScript and TSX end-to-end parity across semantic retrieval, changed-only/watch lifecycle, provenance/replay, and release gates.

## Functional requirements (EARS)
- WHEN semantic runtime is enabled and repository indexing processes `.ts` or `.tsx` files THE SYSTEM SHALL generate deterministic semantic chunks and persist embeddings for those files using the same provider/model metadata and replacement semantics as existing supported source languages.
- WHEN a client calls `search_hybrid` with `language=typescript`, `ts`, or `tsx` THE SYSTEM SHALL return deterministic lexical, graph, and semantic evidence over TypeScript/TSX sources when available.
- WHEN changed-only reindex or built-in watch mode observes TypeScript/TSX additions, edits, renames, or deletions THE SYSTEM SHALL refresh symbol, precise, and semantic search state without stale TypeScript/TSX results surviving.
- WHEN provenance, deep-search, and replay flows invoke TypeScript/TSX-backed search or navigation tools THE SYSTEM SHALL preserve deterministic payloads, citations, and replay semantics.
- IF TypeScript/TSX parity changes benchmark expectations or release-readiness inputs THEN THE SYSTEM SHALL update workload budgets, generated reports, and gating docs in the same change set.
- WHILE identical TypeScript/TSX repository state and runtime configuration are replayed THE SYSTEM SHALL preserve deterministic ordering and note metadata across hybrid search and navigation outputs.

## Non-functional requirements
- TypeScript/TSX parity SHALL NOT require Node, `tsc`, or any JavaScript runtime dependency for Frigg itself.
- Semantic-on, semantic-off, degraded, and strict-failure coverage SHALL include TypeScript/TSX representative cases or documented rationale for shared cross-language coverage.
- Release gates, docs sync, and benchmark/report generation SHALL pass with TypeScript/TSX claims enabled.
