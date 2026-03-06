# Tasks — 38-single-crate-consolidation

Meta:
- Spec: 38-single-crate-consolidation — Single Crate Consolidation
- Depends on: 37-public-surface-parity-gates
- Global scope:
  - Cargo.toml
  - Cargo.lock
  - crates/
  - scripts/
  - Justfile
  - docs/
  - specs/38-single-crate-consolidation/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Capture baseline validation matrix and freeze migration guardrails (owner: mayor) (scope: Justfile, scripts/, specs/38-single-crate-consolidation/) (depends: -)
  - Started_at: 2026-03-05T15:45:00Z
  - Completed_at: 2026-03-05T16:00:00Z
  - Completion note: Baseline validation matrix recorded as all-green before structural migration. Results: `cargo check --workspace` pass, `cargo test --workspace` pass, `just docs-sync` pass (`tool-surface-parity status=pass`), `just release-ready` pass (`benchmark_summary pass=37 fail=0 missing=0`).
  - Validation result: `cargo check --workspace`; `cargo test --workspace`; `just docs-sync`; `just release-ready`.
- [x] T002: Create monolith module skeleton inside `frigg` crate with temporary compatibility aliases (owner: mayor) (scope: crates/cli/src/) (depends: T001)
  - Started_at: 2026-03-05T16:00:00Z
  - Completed_at: 2026-03-05T16:35:00Z
  - Completion note: Added `crates/cli/src/lib.rs` as monolith entrypoint and rewired `main.rs` to consume in-package modules.
  - Validation result: `cargo check -p frigg`.
- [x] T003: Migrate foundational crates (`domain`, `settings`, `graph`, `storage`) into `frigg` modules (owner: mayor) (scope: crates/core/, crates/config/, crates/graph/, crates/storage/, crates/cli/src/) (depends: T002)
  - Started_at: 2026-03-05T16:35:00Z
  - Completed_at: 2026-03-05T16:50:00Z
  - Completion note: Copied foundational sources into `crates/cli/src/{domain,settings,graph,storage}` and normalized imports to `crate::...`.
  - Validation result: `cargo check -p frigg`; `cargo test -p frigg --test security`.
- [x] T004: Migrate retrieval crates (`embeddings`, `indexer`, `searcher`) into `frigg` modules (owner: mayor) (scope: crates/embeddings/, crates/index/, crates/search/, crates/cli/src/) (depends: T003)
  - Started_at: 2026-03-05T16:50:00Z
  - Completed_at: 2026-03-05T17:00:00Z
  - Completion note: Copied retrieval sources into `crates/cli/src/{embeddings,indexer,searcher}` and fixed module-local test imports.
  - Validation result: `cargo check -p frigg`; `cargo test -p frigg`.
- [x] T005: Migrate MCP + CLI integration into single crate (owner: mayor) (scope: crates/mcp/, crates/cli/src/) (depends: T004)
  - Started_at: 2026-03-05T17:00:00Z
  - Completed_at: 2026-03-05T17:15:00Z
  - Completion note: Migrated MCP runtime module into `crates/cli/src/mcp`, rewired integration tests/benches to `frigg::mcp` paths, and kept tool-surface parity checks green.
  - Validation result: `cargo test -p frigg --test security`; `cargo test -p frigg --test tool_handlers`; `cargo test -p frigg --test provenance`; `cargo test -p frigg --test playbook_suite`.
- [x] T006: Migrate `testkit` to monolith test support and rewrite residual imports (owner: mayor) (scope: crates/testkit/, crates/cli/src/, crates/cli/tests/, crates/cli/benches/) (depends: T005)
  - Started_at: 2026-03-05T17:15:00Z
  - Completed_at: 2026-03-05T17:20:00Z
  - Completion note: Migrated test support into `crates/cli/src/test_support.rs` and rewrote remaining cross-crate references to in-crate module paths.
  - Validation result: `rg -n "\\b(domain|settings|storage|indexer|searcher|embeddings|graph|mcp|testkit)::" crates --glob '*.rs'`.
- [x] T007: Update operational commands, scripts, and docs to single-crate invocations/paths (owner: mayor) (scope: Justfile, scripts/, docs/) (depends: T006)
  - Started_at: 2026-03-05T17:20:00Z
  - Completed_at: 2026-03-05T17:35:00Z
  - Completion note: Updated `Justfile`, `scripts/check-release-readiness.sh`, and benchmark/security docs from multi-package invocations to `-p frigg` with explicit bench/test targets.
  - Validation result: `just docs-sync`; `just release-ready`; `python3 scripts/check-tool-surface-parity.py`.
- [x] T008: Remove obsolete crate manifests/directories and collapse workspace to one package (owner: mayor) (scope: Cargo.toml, Cargo.lock, crates/) (depends: T007)
  - Started_at: 2026-03-05T17:35:00Z
  - Completed_at: 2026-03-05T17:40:00Z
  - Completion note: Reduced workspace members to `crates/cli` only and removed obsolete `Cargo.toml` files for former internal crates; Cargo metadata now reports a single package.
  - Validation result: `cargo check`; `cargo test -p frigg`; `cargo metadata --no-deps --format-version 1`.
- [x] T009: Publish-readiness verification for single crate (owner: mayor) (scope: Cargo.toml, crates/cli/Cargo.toml, README.md) (depends: T008)
  - Started_at: 2026-03-05T17:40:00Z
  - Completed_at: 2026-03-05T17:50:00Z
  - Completion note: `cargo publish --dry-run -p frigg --allow-dirty` now succeeds. Accepted warnings: missing package metadata fields (`description/documentation/homepage/repository`) and intentionally externalized test/bench entries excluded from publish tarball.
  - Validation result: `cargo publish --dry-run -p frigg --allow-dirty`.
