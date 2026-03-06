# Requirements — 30-citation-hygiene-gate

## Goal
Close citation hygiene by enforcing deterministic offline checks for `docs/overview.md` source registry completeness.

## Functional requirements (EARS)
- WHEN `docs/overview.md` contains a primary-source URL in architecture/analysis sections THE SYSTEM SHALL require the same URL to appear in `### Fact-check registry (...)`.
- IF docs contain citation placeholder markers (`TODO`, `TBD`, `FIXME`, `citation needed`, `legacy placeholder`) THEN THE SYSTEM SHALL fail docs checks with deterministic diagnostics.
- WHEN docs sync and release-readiness checks run THE SYSTEM SHALL execute citation hygiene validation offline (without network dependency).
- WHEN citation registry metadata is validated THE SYSTEM SHALL require a deterministic date marker that aligns with project sync metadata.

## Non-functional requirements
- Deterministic output ordering and stable failure messages.
- No network calls during citation checks.
- Backward-compatible with existing docs-sync and release-ready workflows.
