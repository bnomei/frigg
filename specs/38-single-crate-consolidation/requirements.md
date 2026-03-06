# Requirements — 38-single-crate-consolidation

## Goal
Consolidate the current multi-crate Rust workspace into one publishable crate (`frigg`) while preserving runtime behavior, test coverage, benchmark coverage, and release tooling.

## Functional requirements (EARS)
- WHEN consolidation starts THE SYSTEM SHALL keep `frigg` as the only runtime artifact and publish target.
- WHEN code from an internal crate is migrated THE SYSTEM SHALL preserve public API behavior consumed by CLI commands and MCP handlers.
- WHILE migration is in progress THE SYSTEM SHALL keep CI green at each phase (`cargo check`, tests, and critical benches where applicable).
- WHEN import paths are rewritten THE SYSTEM SHALL remove cross-crate references (`domain::`, `settings::`, `indexer::`, etc.) in production, tests, and benches.
- WHEN consolidation reaches cutover THE SYSTEM SHALL eliminate internal path dependencies between former workspace crates.
- IF a migration step breaks deterministic fixture or contract checks THEN THE SYSTEM SHALL block further cleanup until parity is restored.
- WHEN release scripts run after cutover THE SYSTEM SHALL use single-crate commands (no `-p` targeting removed crates).
- WHEN `cargo publish --dry-run` is executed for `frigg` THE SYSTEM SHALL complete dependency verification without local path-dependency errors.

## Non-functional requirements
- Migration must be incremental and reversible at commit boundaries.
- Security and contract gates must retain current guarantees (workspace boundaries, tool-surface parity, release-readiness checks).
- Benchmark/report workflows must continue to produce deterministic artifacts.
