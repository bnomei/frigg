# Tasks — 42-manifest-freshness-validation

Meta:
- Spec: 42-manifest-freshness-validation — Manifest Snapshot Freshness Validation
- Depends on: 16-symbol-corpus-cache-fastpath, 39-performance-memory-hardening
- Global scope:
  - crates/cli/src/mcp/
  - crates/cli/src/searcher/
  - contracts/
  - benchmarks/
  - specs/42-manifest-freshness-validation/
  - specs/index.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add metadata-only manifest freshness validation for symbol-corpus reuse (owner: mayor) (scope: crates/cli/src/mcp/, specs/42-manifest-freshness-validation/) (depends: -)
  - Started_at: 2026-03-06T12:30:00Z
  - Completed_at: 2026-03-06T14:05:00Z
  - Context: The current symbol-corpus fast path trusts persisted manifest digests after checking only path existence. The new implementation must validate size and mtime metadata before reusing snapshot-backed cache signatures and source-path discovery.
  - Reuse_targets: `load_latest_manifest_snapshot`, `manifest_source_paths_for_root`, `root_signature`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: symbol-corpus reuse derives signatures from validated metadata, rejects stale snapshots deterministically, and avoids reusing stale in-memory corpora for changed repositories.
  - Validation: targeted `cargo test -p frigg --test tool_handlers` stale-manifest coverage.
  - Escalate if: metadata-only validation cannot reliably distinguish current versus stale snapshots on supported filesystems without a contract decision.

- [x] T002: Apply the same freshness contract to manifest-backed search candidate discovery and sync docs/bench notes (owner: mayor) (scope: crates/cli/src/searcher/, contracts/, benchmarks/, specs/42-manifest-freshness-validation/, specs/index.md) (depends: T001)
  - Completed_at: 2026-03-06T14:05:00Z
  - Context: Search and symbol-corpus paths should not apply different snapshot freshness rules. Candidate discovery must reuse the same validator or wrapper helper.
  - Reuse_targets: `candidate_files_for_repository`, spec 39 manifest-backed search fallback behavior
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: search candidate discovery rejects stale snapshots with the same freshness rule used by the symbol-corpus path; contracts and benchmark notes reflect the new metadata-validation contract if operator-visible behavior changes.
  - Validation: targeted `cargo test -p frigg --lib candidate_discovery_` plus docs sync.
  - Escalate if: the shared freshness helper forces a cross-module refactor larger than this spec’s write scope.
