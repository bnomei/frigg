# Requirements — 35-semantic-runtime-mcp-surface

## Goal
Promote Track C semantic retrieval from library-only components to an exposed deterministic MCP runtime capability.

## Functional requirements (EARS)
- WHEN semantic runtime is enabled THE SYSTEM SHALL build and persist embeddings for indexed code chunks using configured provider and model settings.
- WHERE semantic runtime is enabled THE SYSTEM SHALL support OpenAI and Google embedding providers with non-empty API keys.
- WHEN a client calls `search_hybrid` with semantic retrieval enabled THE SYSTEM SHALL combine lexical, graph, and semantic evidence into one deterministic ranked response.
- IF provider credentials are missing, blank, or invalid THEN THE SYSTEM SHALL return typed deterministic errors and SHALL NOT mutate semantic index state.
- IF semantic provider calls fail during query execution THEN THE SYSTEM SHALL degrade to lexical and graph channels with explicit partial-channel metadata unless strict-semantic mode is enabled.
- WHILE identical query inputs and repository snapshot state are replayed THE SYSTEM SHALL return deterministic ordering and provenance IDs for hybrid evidence.
- WHEN semantic runtime is disabled THE SYSTEM SHALL preserve existing behavior for `search_text`, `search_symbol`, and `find_references`.

## Non-functional requirements
- Semantic startup and query failure messages must remain deterministic and contract-mapped.
- Semantic retrieval must stay read-only from MCP client perspective (no write/destructive tool semantics).
- Latency budgets for semantic-enabled queries must be defined and added to benchmark reporting.
