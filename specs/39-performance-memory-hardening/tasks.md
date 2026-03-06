# Tasks — 39-performance-memory-hardening

Meta:
- Spec: 39-performance-memory-hardening — Performance And Memory Hardening
- Depends on: 24-find-references-resource-budgets, 33-regex-trigram-bitmap-acceleration, 34-readonly-ide-navigation-tools, 35-semantic-runtime-mcp-surface, 38-single-crate-consolidation
- Global scope:
  - crates/cli/src/indexer/
  - crates/cli/src/searcher/
  - crates/cli/src/mcp/
  - crates/cli/src/graph/
  - crates/cli/src/storage/
  - contracts/
  - benchmarks/
  - README.md
  - specs/39-performance-memory-hardening/
  - specs/index.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Stream manifest hashing and `read_file` line slicing (owner: mayor) (scope: crates/cli/src/indexer/, crates/cli/src/mcp/, specs/39-performance-memory-hardening/) (depends: -)
  - Started_at: 2026-03-06T10:00:00Z
  - Completed_at: 2026-03-06T10:25:00Z
  - Completion note: Replaced manifest digest whole-file reads with buffered Blake3 streaming and rewrote `read_file` line-slice mode to read selected lines without materializing the full file; added lossy UTF-8 line-slice regression coverage.
  - Validation result: `cargo test -p frigg core_read_file_ -- --nocapture` passed; `cargo test -p frigg manifest -- --nocapture` passed.

- [x] T002: Remove query-time repository walks from text and hybrid search where manifests exist (owner: mayor) (scope: crates/cli/src/searcher/, crates/cli/src/storage/, specs/39-performance-memory-hardening/) (depends: T001)
  - Started_at: 2026-03-06T10:25:00Z
  - Completed_at: 2026-03-06T13:15:00Z
  - Completion note: Routed literal and selected regex searches through streaming line scans, preferred persisted manifest candidates when manifest paths resolve on disk, and hardened stale-manifest fallback back to live filesystem walks for mismatched `.frigg` snapshots.
  - Validation result: `cargo test -p frigg candidate_discovery_ -- --nocapture` passed; `cargo test -p frigg --test tool_handlers -- --nocapture` passed.

- [x] T003: Add symbol-corpus and SCIP-discovery cache fastpaths (owner: mayor) (scope: crates/cli/src/mcp/, specs/39-performance-memory-hardening/) (depends: T001)
  - Started_at: 2026-03-06T13:15:00Z
  - Completed_at: 2026-03-06T15:00:00Z
  - Completion note: Added repo-local latest precise-graph reuse keyed by cached discovery metadata, plus manifest-backed corpus reuse that falls back to live discovery when persisted manifest paths do not resolve under the active workspace root.
  - Validation result: `cargo test -p frigg scip_ -- --nocapture` passed; `cargo test -p frigg --test tool_handlers -- --nocapture` passed.

- [x] T004: Add precise-graph secondary indexes and use them in navigation handlers (owner: mayor) (scope: crates/cli/src/graph/, crates/cli/src/mcp/, specs/39-performance-memory-hardening/) (depends: T003)
  - Started_at: 2026-03-06T13:30:00Z
  - Completed_at: 2026-03-06T15:00:00Z
  - Completion note: Added by-repository, by-file, by-symbol, and by-target precise indexes with incremental upkeep across replace/overlay SCIP ingest paths and switched navigation helpers to indexed definition/relationship lookups.
  - Validation result: `cargo test -p frigg scip_ -- --nocapture` passed; `cargo test -p frigg --test tool_handlers -- --nocapture` passed.

- [x] T005: Reduce semantic indexing/query memory pressure and incremental write amplification (owner: mayor) (scope: crates/cli/src/indexer/, crates/cli/src/searcher/, crates/cli/src/storage/, specs/39-performance-memory-hardening/) (depends: T002)
  - Started_at: 2026-03-06T15:00:00Z
  - Completed_at: 2026-03-06T16:00:00Z
  - Completion note: Added lean semantic projection loads plus late chunk-text hydration for top-ranked semantic hits, and changed semantic changed-only reindex to advance unchanged rows while replacing only changed/deleted paths.
  - Validation result: `cargo test -p frigg semantic_ -- --nocapture` passed; `cargo test -p frigg -- --nocapture` passed.

- [x] T006: Sync docs, contracts, changelog, and ledger with the hardening pass (owner: mayor) (scope: contracts/, benchmarks/, README.md, specs/39-performance-memory-hardening/, specs/index.md) (depends: T001, T002, T003, T004, T005)
  - Started_at: 2026-03-06T16:00:00Z
  - Completed_at: 2026-03-06T16:20:00Z
  - Completion note: Updated the public changelog, semantic contract notes, and spec ledger/index so the repo documents the shipped performance/memory hardening behavior and the stale-manifest fallback semantics discovered during validation.
  - Validation result: `cargo test -p frigg -- --nocapture` passed; manual contract/spec consistency pass completed.
