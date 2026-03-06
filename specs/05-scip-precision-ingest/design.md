# Design — 05-scip-precision-ingest

## Normative excerpt (from `docs/overview.md`)
- SCIP is a transmission format; ingest into query-optimized storage.
- Occurrences include range and role-bit semantics.
- Precise references should override heuristic paths when available.

## Architecture
- `fixtures/scip/` stores representative SCIP artifacts for regression tests.
- `crates/graph/` adds ingestion adapters mapping SCIP documents to symbol/occurrence/edge records.
- `crates/mcp/` and query paths read precision metadata and enforce precedence.

## Data model highlights
- `symbol` keyed by stable SCIP symbol string.
- `occurrence` linked to file + symbol + role bits.
- `relationship` edges normalized for graph traversal.

## Precision precedence
1. Query precise index first.
2. If no precise result, fall back to heuristic resolver.
3. Always emit `precision: precise|heuristic` in result metadata.
