# Requirements — 31-write-surface-security-gates

## Goal
Add enforceable policy gates for future write-capable MCP tools before any write surface is introduced.

## Functional requirements (EARS)
- IF a destructive/write MCP tool is introduced THEN THE SYSTEM SHALL require explicit write confirmation semantics (`confirm`) and a typed `confirmation_required` error path.
- WHEN write-capable tools are introduced THE SYSTEM SHALL preserve path sandboxing and regex safety invariants already required for read/search surfaces.
- WHEN release-readiness checks run THE SYSTEM SHALL fail if write-surface policy guards are missing or drifted.
- WHILE the public tool contract remains read-only THE SYSTEM SHALL enforce deterministic tests that prevent accidental unsafe write-surface introduction.

## Non-functional requirements
- Policy checks must be deterministic and CI-friendly.
- Existing read-only tool behavior and schemas remain backward-compatible.
- Security contract documentation must remain synchronized with release gates.
