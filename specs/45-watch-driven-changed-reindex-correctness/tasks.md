# Tasks — 45-watch-driven-changed-reindex-correctness

Meta:
- Spec: 45-watch-driven-changed-reindex-correctness — Watch-Driven Changed Reindex Correctness
- Depends on: 02-ingestion-and-incremental-index, 20-reindex-resilience-diagnostics, 35-semantic-runtime-mcp-surface, 42-manifest-freshness-validation, 44-integrated-local-watch-mode
- Global scope:
  - crates/cli/src/indexer/
  - crates/cli/src/searcher/
  - crates/cli/src/storage/
  - crates/cli/src/watch.rs
  - crates/cli/tests/
  - README.md
  - specs/45-watch-driven-changed-reindex-correctness/
  - specs/index.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add failing correctness regressions for watch-triggered changed-only refresh behavior (owner: mayor) (scope: crates/cli/src/indexer/, crates/cli/src/searcher/, crates/cli/tests/, specs/45-watch-driven-changed-reindex-correctness/) (depends: -)
  - Completed_at: 2026-03-06T13:37:36Z
  - Context: The watcher is only a scheduler. The protected contract is that watch-triggered refreshes must reuse the current changed-only pipeline while preserving latest-successful-snapshot serving and freshness for modify, delete, rename, docs visibility, and ignored-path exclusion.
  - Reuse_targets: existing manifest freshness tests, semantic indexing tests, hybrid playbook suite
  - Autonomy: standard
  - Risk: medium
  - Complexity: high
  - Bundle_with: T002
  - DoD: deterministic tests exist for modify/delete/rename cases, docs visibility, ignored noise, and at least one query-during-refresh scenario.
  - Validation: `cargo test -p frigg watch_runtime_ -- --nocapture`; `cargo test -p frigg latest_manifest_validation_requires_present_fresh_snapshot -- --nocapture`
  - Escalate if: proving query-during-refresh semantics requires a broader serving abstraction than the current synchronous startup/runtime layers expose.

- [x] T002: Implement or tighten changed-only freshness behavior needed by the new regressions (owner: mayor) (scope: crates/cli/src/indexer/, crates/cli/src/searcher/, crates/cli/src/storage/, crates/cli/src/watch.rs, specs/45-watch-driven-changed-reindex-correctness/) (depends: T001)
  - Completed_at: 2026-03-06T13:37:36Z
  - Context: After a watch-triggered refresh commits, superseded manifest and semantic data must no longer leak into results. Existing manifest freshness fallback behavior remains the safety net for stale persisted snapshots.
  - Reuse_targets: `reindex_repository(..., ReindexMode::ChangedOnly)`, manifest freshness validation, semantic row advancement helpers
  - Autonomy: standard
  - Risk: medium
  - Complexity: high
  - Bundle_with: T004
  - DoD: all new freshness regressions pass without introducing a new patch-based indexer.
  - Validation: `cargo test -p frigg watch_ -- --nocapture`; `set -a; source .env; set +a; export FRIGG_SEMANTIC_RUNTIME_ENABLED=true; export FRIGG_SEMANTIC_RUNTIME_PROVIDER=openai; cargo run -p frigg -- reindex --changed --workspace-root .`
  - Escalate if: fixing correctness would require changing persisted snapshot semantics instead of reusing the current contract.

- [x] T003: Preserve docs visibility and ignored-root exclusion across watcher-triggered refreshes (owner: mayor) (scope: crates/cli/src/indexer/, crates/cli/src/searcher/, crates/cli/src/watch.rs, specs/45-watch-driven-changed-reindex-correctness/) (depends: T001)
  - Completed_at: 2026-03-06T13:37:36Z
  - Context: `docs/` must remain indexable and searchable even when gitignored, while `.git/`, `.frigg/`, and `target/` must remain excluded from trigger and corpus behavior.
  - Reuse_targets: current explicit `docs/` walk, manifest/search ignore tests
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Bundle_with: T002
  - DoD: watcher-triggered refreshes preserve the same docs-visible and ignored-noise invariants as explicit reindex commands.
  - Validation: `cargo test -p frigg watch_runtime_initial_sync_preserves_docs_visibility_and_target_exclusion -- --nocapture`; `cargo test -p frigg excludes_target_artifacts -- --nocapture`
  - Escalate if: the current indexing/search ignore split needs a larger unification than this spec allows.

- [x] T004: Update operator guidance and validate watcher-triggered end-to-end behavior (owner: mayor) (scope: README.md, specs/45-watch-driven-changed-reindex-correctness/, specs/index.md) (depends: T002, T003)
  - Completed_at: 2026-03-06T13:37:36Z
  - Context: README guidance should explain when built-in watch mode is active, when external watchers still make more sense, and how changed-only refresh correctness is preserved.
  - Reuse_targets: existing README reindex section, strict hybrid playbook harness
  - Autonomy: standard
  - Risk: low
  - Complexity: medium
  - DoD: operator docs describe built-in vs external watcher usage accurately, and the strict playbook suite is executed after a watcher-triggered change path.
  - Validation: `set -a; source .env; set +a; export FRIGG_SEMANTIC_RUNTIME_ENABLED=true; export FRIGG_SEMANTIC_RUNTIME_PROVIDER=openai; cargo test -p frigg --test playbook_hybrid_suite -- --nocapture`
  - Escalate if: operator guidance needs a broader deployment-positioning rewrite beyond the local watch scope.
