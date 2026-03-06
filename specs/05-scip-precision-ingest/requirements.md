# Requirements — 05-scip-precision-ingest

## Scope
Add precise L3 navigation path via SCIP ingestion and override heuristics when available.

## EARS requirements
- When SCIP artifacts are provided, the precision subsystem shall ingest occurrences, symbols, and relationships into query storage.
- When precise reference data exists for a symbol, the system shall prefer precise references over heuristic references.
- While both precise and heuristic references exist, the system shall label source precision in response payloads.
- If SCIP payload parsing fails, then ingestion shall return typed invalid-input diagnostics without corrupting existing graph state.
- The system shall support incremental SCIP updates at file-level granularity.
