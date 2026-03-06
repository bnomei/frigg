# Requirements — 40-symbol-resolution-indexes

## Goal
Deterministic Indexed Symbol Resolution

## Functional requirements (EARS)
- WHEN repository symbol corpora are built THE SYSTEM SHALL derive deterministic lookup indexes for stable ids, canonical repository-relative paths, and normalized symbol names without changing public result ordering.
- WHEN `search_symbol` runs with an exact-matchable or prefix-matchable query THE SYSTEM SHALL narrow candidate symbols through corpus indexes before considering lower-rank infix fallback work.
- WHEN read-only navigation tools resolve a target symbol from `symbol` input THE SYSTEM SHALL use indexed stable-id and exact-name lookup instead of scanning every symbol in the scoped corpora.
- WHEN read-only navigation tools resolve a target symbol from `path` plus `line` input THE SYSTEM SHALL use per-path symbol indexes and deterministic nearest-preceding selection instead of whole-corpus scans.
- IF indexed lookup cannot determine an authoritative candidate set THEN THE SYSTEM SHALL preserve current deterministic fallback semantics and typed empty-result behavior.
- IF indexed symbol resolution changes benchmark expectations, public notes, or documented operator guidance THEN THE SYSTEM SHALL update the corresponding contracts artifacts in the same change set.

## Non-functional requirements
- Deterministic ordering and tie-break behavior must remain backward-compatible.
- Warm-corpus symbol search and target-resolution latency should scale with narrowed candidate sets rather than total symbol count whenever an indexed query shape applies.
- Validation must include targeted regression coverage for `search_symbol`, `go_to_definition`, and location-based target resolution, plus benchmark or measurement coverage for the new indexed hot paths.
