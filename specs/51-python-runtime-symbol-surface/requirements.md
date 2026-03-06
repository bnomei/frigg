# Requirements — 51-python-runtime-symbol-surface

## Goal
Add Python as a first-class runtime L1/L2 language for Frigg's symbol and read-only navigation surfaces.

## Functional requirements (EARS)
- WHEN repository manifests include `.py` files THE SYSTEM SHALL treat them as supported source files for symbol-corpus construction, warm-corpus rebuilds, and runtime symbol queries.
- WHEN a supported Python file is indexed THE SYSTEM SHALL extract deterministic symbol definitions for modules, classes, decorated functions, `async def` functions, methods, and simple identifier-bound constant or attribute declarations needed for common application and service code.
- WHEN a client calls `search_symbol`, `document_symbols`, or `search_structural` against supported Python sources THE SYSTEM SHALL return canonical repository-relative results with deterministic ordering and note metadata aligned to current supported-language behavior.
- WHEN a client calls `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, or `outgoing_calls` for a Python-backed symbol or source position THE SYSTEM SHALL reuse the existing symbol-corpus and heuristic fallback flows without Python-specific contract drift.
- WHEN a client supplies a Python-oriented runtime `language` filter THE SYSTEM SHALL accept `python` and `py` as aliases for one logical Python family.
- IF Python parser configuration or symbol extraction fails for a file THEN indexing SHALL continue and SHALL emit typed diagnostics without corrupting cached corpus state.
- WHILE identical repository state and query inputs are replayed THE SYSTEM SHALL preserve stable Python symbol IDs and deterministic byte-equivalent ordering.

## Non-functional requirements
- Initial Python support in this spec SHALL be limited to `.py` files; `.pyi`, notebooks, and generated stubs are out of scope.
- This spec SHALL NOT make precise SCIP or semantic-runtime parity claims for Python.
- Public docs/contracts SHALL describe Python support as runtime L1/L2 only until later follow-on work exists.
