# Design — 41-partial-precise-degradation

## Scope
- `crates/cli/src/mcp/`
- `crates/cli/src/graph/`
- `contracts/`
- `benchmarks/`
- `specs/41-partial-precise-degradation/`
- `specs/index.md`

## Problem statement
The current precise ingest path keeps per-artifact failure diagnostics, but if any artifact fails ingest the server clears all precise data for the corpus before caching the graph. That converts a localized artifact hygiene problem into a repository-wide precision downgrade.

The current behavior is operationally safe, but it throws away successfully ingested precise data and makes mixed-quality SCIP repos degrade more often than necessary.

## Approach
Retain successfully ingested precise data and make request-time handlers partial-aware:

1. Remove the corpus-wide `clear_precise_data()` reaction to mixed ingest success.
2. Extend cached precise-graph metadata with an explicit coverage mode:
   - `full`: all discovered artifacts ingested successfully;
   - `partial`: at least one artifact ingested successfully and at least one failed;
   - `none`: no usable precise artifact data exists.
3. Treat precise absence differently by coverage mode:
   - in `full` mode, existing precise absence semantics remain authoritative;
   - in `partial` mode, precise hits may be returned, but empty precise lookups are not authoritative and must fall back to heuristic resolution;
   - in `none` mode, the request goes directly to heuristic fallback.
4. Update response notes so callers can distinguish `precise`, `precise_partial`, and heuristic outcomes without guessing from artifact counters alone.

## Architecture changes

### Cached precise-graph state
- Preserve the successfully ingested precise records inside `CachedPreciseGraph`.
- Record coverage mode and artifact success/failure counts alongside existing discovery metadata.

### Request-time resolution rules
- `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls` must branch on precise coverage mode before interpreting empty precise lookups.
- In partial mode:
  - non-empty precise results can be returned as degraded precise answers;
  - empty precise results must route through heuristic fallback because absence is not authoritative.

### Public metadata
- Preserve the existing deterministic note structure as much as possible.
- Add explicit coverage/precision fields instead of overloading existing boolean flags.

## Risks
- Partial precise mode can accidentally overstate certainty if empty precise lookups are still treated as authoritative.
- Handler branching can drift across the read-only tool surface if one tool is not updated with the shared partial-mode rule.
- Public note changes can break tests or contract docs if not updated together.

## Validation strategy
- Mixed-success SCIP ingest tests proving retained precise data survives artifact failures.
- Handler tests proving partial-mode non-empty precise hits are returned while empty precise lookups fall back to heuristic resolution.
- Contract/docs coverage for any new precision-mode metadata.
