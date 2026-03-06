# Requirements — 37-public-surface-parity-gates

## Goal
Enforce executable parity between runtime MCP `tools/list`, schema contracts, and public tool-surface documentation.

## Functional requirements (EARS)
- WHEN runtime tool registration changes THE SYSTEM SHALL fail contract tests unless `tools/list` names match the declared public schema set.
- IF a tool appears in schema docs but is not runtime-exposed (for the active feature profile) THEN THE SYSTEM SHALL fail release gates with deterministic diagnostics.
- IF a tool is runtime-exposed but lacks a schema contract entry THEN THE SYSTEM SHALL fail release gates with deterministic diagnostics.
- WHILE optional feature gates alter tool surface THE SYSTEM SHALL publish deterministic profile-specific manifests and docs annotations.
- WHEN contract/docs sync checks run THE SYSTEM SHALL verify tool-surface claims in `contracts/tools/v1/README.md` and `docs/overview.md` against executable runtime data.

## Non-functional requirements
- Parity checks must run offline and deterministically in local CI/release workflows.
- Failure messages must identify exact missing/extra tools and active feature profile.
- Existing read-only/write-surface policy checks must remain intact.
