# Requirements — 39-performance-memory-hardening

## Goal
Performance And Memory Hardening

## Functional requirements (EARS)
- WHEN Frigg hashes files for manifest construction THE SYSTEM SHALL stream file bytes without loading entire files into memory.
- WHEN `read_file` serves a bounded line range THE SYSTEM SHALL compute the returned slice without materializing the full file content in memory.
- WHEN text, regex, or hybrid search runs against an indexed repository THE SYSTEM SHALL prefer persisted manifest-backed candidate discovery over fresh full-repository filesystem walks.
- WHEN navigation tools reuse an unchanged repository corpus or unchanged SCIP artifact set THE SYSTEM SHALL reuse cached corpus and precise-graph state without repeating full filesystem discovery work.
- WHEN precise navigation resolves occurrences, relationships, or symbol selections THE SYSTEM SHALL use targeted indexes instead of scanning whole precise-graph maps for point lookups.
- WHEN semantic search ranks repository chunks THE SYSTEM SHALL avoid loading snapshot fields that are not required for scoring and top-k response construction.
- WHEN semantic reindex runs in changed-only mode THE SYSTEM SHALL avoid deleting and rewriting unchanged semantic embedding rows for the repository.
- IF performance hardening changes a public default, note shape, or operator-facing runtime knob THEN THE SYSTEM SHALL update the corresponding public contracts and changelog in the same change set.

## Non-functional requirements
- Deterministic ordering and typed error behavior must remain unchanged unless explicitly documented.
- Peak memory usage for the audited hot paths must decrease by removing avoidable whole-file, whole-artifact, and whole-snapshot buffering.
- Validation must include targeted regression coverage for each optimized path plus at least one full `cargo test -p frigg` run before completion.
