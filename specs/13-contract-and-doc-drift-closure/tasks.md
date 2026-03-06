# Tasks — 13-contract-and-doc-drift-closure

Meta:
- Spec: 13-contract-and-doc-drift-closure — Contract and Documentation Drift Closure
- Depends on: 00-contracts-and-governance, 08-hybrid-retrieval-and-deep-search-harness
- Global scope:
  - contracts/
  - docs/
  - specs/
  - crates/mcp/src/mcp/deep_search.rs
  - crates/mcp/tests/
  - scripts/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T003: Fix deep-search citation field mapping (`excerpt` primary) and update tests/fixtures (owner: worker:019cba95-ad8a-7bf1-8942-3b3ea7b5449b) (scope: crates/mcp/src/mcp/deep_search.rs, crates/mcp/tests/) (depends: -)
  - Started_at: 2026-03-04T20:41:46Z
  - Completed_at: 2026-03-04T20:45:00Z
  - Completion note: Citation composition now reads `excerpt` first with legacy `snippet` fallback; fixture/test coverage extended for precedence and fallback.
  - Validation result: `cargo test -p mcp citation_payloads` and `cargo test -p mcp playbook_suite` passed.
- [x] T001: Add contract changelog artifact and enforce via release-readiness gate (owner: worker:019cba95-a2c2-7ea0-aa6d-49aa070c566c) (scope: contracts/, scripts/check-release-readiness.sh) (depends: -)
  - Started_at: 2026-03-04T20:41:46Z
  - Completed_at: 2026-03-04T20:47:27Z
  - Completion note: Added `contracts/changelog.md` and release-readiness gate enforcement for this artifact.
  - Validation result: `bash scripts/check-release-readiness.sh` passed.
- [x] T002: Align config/runtime and language-support documentation to implementation truth (owner: worker:019cba95-a2c2-7ea0-aa6d-49aa070c566c) (scope: contracts/config.md, specs/04-symbol-graph-heuristic-nav/design.md, docs/overview.md if needed) (depends: -)
  - Started_at: 2026-03-04T20:41:46Z
  - Completed_at: 2026-03-04T20:47:27Z
  - Completion note: Aligned configuration and language-support docs with implemented runtime behavior.
  - Validation result: `bash scripts/check-release-readiness.sh` passed.
- [x] T004: Document repository-id stability semantics and deep-search harness surface status (owner: worker:019cba95-a2c2-7ea0-aa6d-49aa070c566c) (scope: contracts/README.md, docs/overview.md, benchmarks/deep-search.md) (depends: -)
  - Started_at: 2026-03-04T20:41:46Z
  - Completed_at: 2026-03-04T20:47:27Z
  - Completion note: Documented positional repository-id stability semantics and internal/test-only deep-search harness surface.
  - Validation result: `bash scripts/check-release-readiness.sh` passed.
