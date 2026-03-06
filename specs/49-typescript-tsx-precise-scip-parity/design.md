# Design — 49-typescript-tsx-precise-scip-parity

## Scope
- crates/cli/src/graph/mod.rs
- crates/cli/src/storage/mod.rs
- crates/cli/src/mcp/server.rs
- crates/cli/tests/tool_handlers.rs
- fixtures/scip/
- contracts/
- docs/overview.md
- README.md

## Normative excerpt (in-repo)
- `specs/05-scip-precision-ingest` defines the precise path: ingest occurrences, symbols, and relationships; prefer precise results first; keep incremental replacement semantics.
- `specs/41-partial-precise-degradation` requires Frigg to retain successful precise data and degrade metadata instead of clearing the full precise overlay when one artifact path fails.
- `specs/48-typescript-tsx-runtime-symbol-surface` introduces TypeScript/TSX symbols into the runtime corpus; this spec closes the L3 precise gap.

## Architecture decisions
1. Reuse the existing generic SCIP ingest and storage path. Do not add TypeScript-specific tables, schema branches, or MCP tool variants unless fixture-driven validation exposes a real gap.
2. Treat `.ts` and `.tsx` as path-level document variants inside one precise TypeScript family. Matching still relies on canonical repository-relative paths and existing repository scoping rules.
3. Use precise data to close known heuristic gaps:
   - `#private` members and other non-identifier names
   - overload declarations and declaration-only anchors
   - import/re-export based targets
   - JSX component definitions, calls, and implementation edges when the artifact provides them
4. Preserve partial-aware behavior from spec 41. Malformed or over-budget artifacts degrade note metadata for affected paths rather than clearing valid precise data from unrelated files.
5. Keep generator guidance neutral. Docs should explain accepted artifact placement, path expectations, and validated behavior, but should not hard-code one external SCIP generator brand into the public contract.

## Precise integration plan

### Fixture matrix
- Add TS/TSX fixture artifacts under `fixtures/scip/` that cover:
  - definitions and references
  - implementation/type relationships
  - call hierarchy edges
  - private identifiers
  - overload or declaration-only anchors
  - TSX component definitions and references
  - malformed/path-mismatch cases

### Ingest and storage
- Reuse the current parse/map/apply flow in `graph` and the current repository-scoped precise replacement semantics.
- Validate canonical path handling for `.ts` and `.tsx` documents so precise rows match the runtime symbol corpus and repository-relative output contracts exactly.
- Preserve incremental replacement semantics: replacing one TS/TSX document does not clear unrelated precise rows.

### Query precedence
- `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls` keep the current precise-first ordering rules.
- TypeScript/TSX precise results should reuse the same note metadata fields already exposed for precise-vs-heuristic distinction and partial degradation.
- Heuristic fallback remains available when precise coverage is absent or non-authoritative.

## Validation and rollout
- Add graph/storage tests to prove TS/TSX ingest, canonical path matching, and incremental replacement semantics.
- Add tool-handler integration tests to prove precise precedence and partial degradation for TS/TSX targets.
- Update docs/contracts in the same change set, but keep language scoped to the artifact behaviors Frigg has actually validated.
