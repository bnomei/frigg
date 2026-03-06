# Requirements — 49-typescript-tsx-precise-scip-parity

## Goal
Add validated precise SCIP parity for TypeScript and TSX navigation and reference flows.

## Functional requirements (EARS)
- WHEN TypeScript/TSX SCIP artifacts are present under supported ingest locations THE SYSTEM SHALL ingest occurrences, symbols, and relationships for `.ts` and `.tsx` documents using the existing precise storage model.
- WHEN precise TypeScript/TSX data exists for a target symbol THE SYSTEM SHALL prefer precise results over heuristic results for `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls`.
- WHEN a subset of TypeScript/TSX SCIP documents is replaced THE SYSTEM SHALL update only the affected precise rows and SHALL preserve unrelated repository precise state.
- IF a TypeScript/TSX SCIP artifact is malformed, path-incompatible, or exceeds resource budgets THEN THE SYSTEM SHALL emit typed invalid-input or partial/degraded precision metadata and SHALL NOT clear valid precise data for unaffected files.
- WHEN precise TypeScript/TSX data covers symbols that are weakly served by identifier-token heuristics, including private members, overload signatures, re-exported symbols, and JSX component definitions, THE SYSTEM SHALL surface those precise results through the existing read-only navigation tools.
- WHEN public docs/contracts describe precise coverage or artifact expectations THE SYSTEM SHALL include TypeScript/TSX-specific guidance without binding Frigg to one external indexer implementation.

## Non-functional requirements
- Precise TypeScript/TSX results SHALL preserve canonical path semantics and deterministic ordering.
- Existing Rust/PHP SCIP ingest semantics and partial-aware degradation behavior SHALL remain unchanged.
