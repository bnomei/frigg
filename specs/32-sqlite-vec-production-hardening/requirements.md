# Requirements — 32-sqlite-vec-production-hardening

## Goal
Harden sqlite-vec runtime behavior for production readiness with explicit registration, version-pin checks, startup verification, and deeper integrity coverage.

## Functional requirements (EARS)
- WHEN storage opens sqlite connections THE SYSTEM SHALL ensure sqlite-vec registration is initialized deterministically.
- IF `vec_version()` is available but does not match the required pinned version policy THEN THE SYSTEM SHALL fail readiness with deterministic typed diagnostics.
- WHEN MCP server startup path runs (no subcommand) THE SYSTEM SHALL perform strict vector readiness checks before serving requests.
- WHILE vector migration safety is enforced THE SYSTEM SHALL preserve explicit no-implicit-transition semantics between sqlite-vec and fallback backends.

## Non-functional requirements
- Deterministic failure messages and repeatable readiness behavior.
- Backward-compatible schema contracts unless explicitly versioned/documented.
- Validation coverage must include runtime tests for registration, version mismatch, and startup gate behavior.
