# Requirements — 17-symlink-safe-provenance-paths

## Goal
Symlink-Safe Provenance Paths

## Functional requirements (EARS)
- WHEN provenance storage paths are resolved THE SYSTEM SHALL reject symlink escapes and enforce canonical-root boundaries before any write.
- WHEN invalid input or unsafe runtime conditions are detected THE SYSTEM SHALL return typed deterministic errors consistent with contract mappings.
- WHILE processing repeated identical inputs THE SYSTEM SHALL preserve deterministic output ordering and metadata semantics.

## Non-functional requirements
- Deterministic behavior across repeated runs.
- Backward-compatible tool contracts unless explicitly versioned.
- Validation must include targeted tests/benches for the changed hot paths.
