# Requirements — 26-provenance-target-integrity

## Goal
Provenance Target Integrity

## Functional requirements (EARS)
- IF a tool call specifies an unknown repository_id THEN THE SYSTEM SHALL not attribute provenance to a different repository.
- WHEN invalid input or unsafe runtime conditions are detected THE SYSTEM SHALL return typed deterministic errors consistent with contract mappings.
- WHILE processing repeated identical inputs THE SYSTEM SHALL preserve deterministic output ordering and metadata semantics.

## Non-functional requirements
- Deterministic behavior across repeated runs.
- Backward-compatible tool contracts unless explicitly versioned.
- Validation must include targeted tests/benches for the changed hot paths.
