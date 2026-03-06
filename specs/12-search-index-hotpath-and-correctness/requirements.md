# Requirements — 12-search-index-hotpath-and-correctness

## Scope
Improve search/index efficiency and correctness while preserving deterministic ordering and contract behavior.

## EARS requirements
- When text search runs with a small `limit`, the searcher shall avoid unnecessary full-match retention and still return deterministic top results.
- While scanning source lines for matches, the searcher shall minimize avoidable allocation overhead in hot loops.
- When heuristic references are computed, the system shall preserve multiple valid references on the same line.
- If filesystem traversal/read errors occur during indexing or search, then the system shall emit deterministic diagnostics instead of silently dropping failures.
- When diagnostics are emitted, MCP responses shall surface diagnostic counts/notes without breaking existing schemas.
