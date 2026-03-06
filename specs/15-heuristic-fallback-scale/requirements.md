# Requirements — 15-heuristic-fallback-scale

## Goal
Heuristic Fallback Scale Fix

## Functional requirements (EARS)
- WHEN heuristic fallback resolves references for large repositories THE SYSTEM SHALL avoid O(files*lines*symbols) full scans by using per-file symbol lookup indexes and bounded source loading.
- WHEN invalid input or unsafe runtime conditions are detected THE SYSTEM SHALL return typed deterministic errors consistent with contract mappings.
- WHILE processing repeated identical inputs THE SYSTEM SHALL preserve deterministic output ordering and metadata semantics.

## Non-functional requirements
- Deterministic behavior across repeated runs.
- Backward-compatible tool contracts unless explicitly versioned.
- Validation must include targeted tests/benches for the changed hot paths.
