# Tasks — 35-semantic-runtime-mcp-surface

Meta:
- Spec: 35-semantic-runtime-mcp-surface — Semantic Runtime MCP Surface
- Depends on: 06-embeddings-and-vector-store, 08-hybrid-retrieval-and-deep-search-harness, 32-sqlite-vec-production-hardening
- Global scope:
  - crates/cli/
  - crates/config/
  - crates/embeddings/
  - crates/index/
  - crates/search/
  - crates/storage/
  - crates/mcp/
  - contracts/
  - benchmarks/
  - docs/overview.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Define semantic runtime config contract and startup wiring (owner: worker:019cbdf0-d773-7c50-bf56-c76dd031e8f6) (scope: crates/cli/, crates/config/, contracts/) (depends: -)
  - Started_at: 2026-03-05T12:20:14Z
  - Completed_at: 2026-03-05T12:30:19Z
  - Completion note: Added typed semantic runtime contract in `settings::FriggConfig` and CLI/env composition wiring, plus deterministic startup fail-fast validation for provider/model/credential requirements while preserving default disabled behavior; synchronized semantic/config contract docs and changelog.
  - Validation result: `cargo test -p settings`, `cargo test -p frigg`, and `just docs-sync` passed.
- [x] T002: Implement semantic indexing pipeline over existing chunk/index artifacts (owner: worker:019cbdf0-d773-7c50-bf56-c76dd031e8f6) (scope: crates/index/, crates/storage/, crates/embeddings/) (depends: T001)
  - Started_at: 2026-03-05T12:30:52Z
  - Completed_at: 2026-03-05T12:43:48Z
  - Completion note: Added deterministic semantic indexing in `indexer` with env-gated runtime config parse, stable chunk candidate derivation and embedding execution, and transactional repository-scoped semantic embedding persistence APIs in `storage` with schema migration/test coverage; preserved disabled-mode behavior.
  - Validation result: `cargo test -p storage` and `cargo test -p indexer` passed.
- [x] T003: Implement semantic channel retrieval and hybrid ranking integration (owner: worker:019cbdf0-d773-7c50-bf56-c76dd031e8f6) (scope: crates/search/, crates/embeddings/, crates/storage/) (depends: T002)
  - Started_at: 2026-03-05T12:44:22Z
  - Completed_at: 2026-03-05T12:55:12Z
  - Completion note: Added deterministic hybrid retrieval execution in `searcher` with semantic channel embedding/querying against persisted snapshot semantic embeddings, semantic on/off toggle behavior, and explicit degraded/strict-failure status metadata in output notes.
  - Validation result: `cargo test -p searcher hybrid_ranking -- --nocapture` and `cargo test -p searcher semantic_channel -- --nocapture` passed.
- [x] T004: Expose `search_hybrid` as a new read-only MCP v1 tool (owner: worker:019cbdf0-d773-7c50-bf56-c76dd031e8f6) (scope: crates/mcp/, contracts/tools/v1/, contracts/errors.md) (depends: T003)
  - Started_at: 2026-03-05T13:08:04Z
  - Completed_at: 2026-03-05T13:14:24Z
  - Completion note: Added `search_hybrid` schema and typed wrappers, registered runtime MCP handler wired to searcher hybrid retrieval with deterministic note/provenance metadata and strict semantic failure mapping, and synchronized tool/error contract docs.
  - Validation result: `cargo test -p mcp schema_ -- --nocapture` and `cargo test -p mcp --test tool_handlers -- --nocapture` passed.
- [x] T005: Add strict/degraded semantic mode tests and replay determinism coverage (owner: worker:019cbdf0-d773-7c50-bf56-c76dd031e8f6) (scope: crates/mcp/tests/, crates/search/, fixtures/playbooks/) (depends: T004)
  - Started_at: 2026-03-05T13:15:01Z
  - Completed_at: 2026-03-05T13:20:54Z
  - Completion note: Added deterministic semantic replay coverage in `searcher` for semantic-enabled, degraded, and strict-failure paths; added deep-search partial-channel replay fixture/tests to prove deterministic mixed error/success trace handling; added citation payload deterministic coverage ensuring errored steps are excluded consistently; and fixed contract/internal replay outcome-type matching in runtime replay tests.
  - Validation result: `cargo test -p mcp deep_search_replay -- --nocapture`, `cargo test -p mcp citation_payloads -- --nocapture`, `cargo test -p mcp --test tool_handlers -- --nocapture`, and `cargo test -p searcher hybrid_ranking_semantic_ -- --nocapture` passed.
- [x] T006: Add benchmark budgets and docs sync for semantic runtime tooling (owner: worker:019cbdf0-d773-7c50-bf56-c76dd031e8f6) (scope: benchmarks/, crates/mcp/benches/, crates/search/benches/, docs/overview.md, contracts/changelog.md) (depends: T005)
  - Started_at: 2026-03-05T13:42:03Z
  - Completed_at: 2026-03-05T13:55:58Z
  - Completion note: Completed semantic runtime benchmark rollout by adding deterministic hybrid semantic-toggle and semantic-degraded workloads in both `search_latency` and `mcp_tool_latency` benches, synchronizing benchmark methodology docs and budget contracts for the new workload IDs, and regenerating release-readiness benchmark artifacts with updated summary counts.
  - Validation result: Worker validation passed (`cargo bench -p mcp`, `cargo bench -p searcher`, `python3 benchmarks/generate_latency_report.py`, `just docs-sync`), and mayor re-validation passed (`cargo bench -p mcp --bench tool_latency -- --noplot`, `cargo bench -p searcher --bench search_latency -- --noplot`, `python3 benchmarks/generate_latency_report.py`, `just docs-sync`, `just release-ready`) with final `benchmark_summary pass=37 fail=0 missing=0`.
