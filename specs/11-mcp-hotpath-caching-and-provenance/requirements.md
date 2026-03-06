# Requirements — 11-mcp-hotpath-caching-and-provenance

## Scope
Eliminate avoidable hot-path recomputation and synchronous storage setup in MCP symbol/reference and provenance paths.

## EARS requirements
- When repeated `search_symbol` calls target unchanged repository content, the Frigg server shall reuse cached symbol corpora instead of reparsing all source files.
- When repeated `find_references` calls target unchanged repository + SCIP artifacts, the Frigg server shall reuse cached precise graph state instead of re-ingesting every artifact.
- When provenance events are recorded for repeated tool calls, the Frigg server shall reuse initialized storage handles per repository target.
- If provenance event identifiers are generated concurrently, then the Frigg server shall produce collision-resistant monotonically ordered trace identifiers.
- While maintaining cache reuse, the Frigg server shall preserve deterministic output ordering and typed error behavior.
