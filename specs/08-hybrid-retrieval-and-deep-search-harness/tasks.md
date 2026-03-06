# Tasks — 08-hybrid-retrieval-and-deep-search-harness

Meta:
- Spec: 08-hybrid-retrieval-and-deep-search-harness — Hybrid Retrieval and Replayable Loop
- Depends on: 03-text-search-engine, 04-symbol-graph-heuristic-nav, 05-scip-precision-ingest, 06-embeddings-and-vector-store, 07-mcp-server-and-tool-contracts
- Global scope:
  - crates/search/
  - crates/mcp/
  - fixtures/playbooks/
  - benchmarks/

## In Progress

- (none)

## Blocked

- (none)

## Todo

## Done

- [x] T004: Build evaluation playbook suite and acceptance metrics (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: fixtures/playbooks/, benchmarks/) (depends: T002, T003)
  - Started_at: 2026-03-04T20:09:19Z
  - Completed_at: 2026-03-04T20:14:31Z
  - Completion note: Added deep-search evaluation playbook suite with expected-output fixtures, deterministic suite tests, and acceptance metrics documentation.
  - Validation result: `cargo test -p mcp playbook_suite` passed (2 tests).
- [x] T003: Add citation/source composer for final answer payloads (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: crates/search/, crates/mcp/) (depends: T001, T002)
  - Started_at: 2026-03-04T20:02:51Z
  - Completed_at: 2026-03-04T20:09:19Z
  - Completion note: Added typed citation/source composer binding claims to tool-call IDs and concrete file spans with deterministic ordering.
  - Validation result: `cargo test -p mcp citation_payloads` passed (2 tests).
- [x] T002: Implement replayable deep-search harness execution model (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: crates/mcp/, fixtures/playbooks/) (depends: T001)
  - Started_at: 2026-03-04T19:56:56Z
  - Completed_at: 2026-03-04T20:02:51Z
  - Completion note: Added replayable deep-search playbook runner with persisted trace artifacts and deterministic replay diff checks.
  - Validation result: `cargo test -p mcp deep_search_replay` passed (3 tests).
- [x] T001: Implement hybrid retrieval aggregator and scoring policy (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: crates/search/) (depends: -)
  - Started_at: 2026-03-04T19:52:57Z
  - Completed_at: 2026-03-04T19:56:56Z
  - Completion note: Added deterministic hybrid retrieval aggregator with configurable channel weights, normalized scoring, and stable tie-break ordering.
  - Validation result: `cargo test -p searcher hybrid_ranking` passed (3 tests).
