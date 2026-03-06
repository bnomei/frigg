# Requirements — 33-regex-trigram-bitmap-acceleration

## Goal
Close Slice 4 regex scale gap by adding deterministic trigram/bitmap prefilter acceleration for regex search.

## Functional requirements (EARS)
- WHEN regex search runs over repository files THE SYSTEM SHALL apply a deterministic trigram/bitmap prefilter when required literals can be derived safely.
- IF a regex pattern cannot produce safe required literals THEN THE SYSTEM SHALL fall back to current regex behavior without changing correctness.
- WHILE prefiltering is enabled THE SYSTEM SHALL preserve deterministic ordering and match equivalence with the non-prefilter path.
- WHEN benchmark/report generation runs THE SYSTEM SHALL include regex workloads that reflect sparse/no-hit cases and document measured budgets.

## Non-functional requirements
- No false negatives introduced by prefiltering.
- Deterministic output ordering remains unchanged.
- Benchmark artifacts and docs remain synchronized with implementation.
