# Tasks — 50-typescript-tsx-semantic-e2e-parity

Meta:
- Spec: 50-typescript-tsx-semantic-e2e-parity — TypeScript/TSX Semantic And E2E Parity
- Depends on: 35-semantic-runtime-mcp-surface, 45-watch-driven-changed-reindex-correctness, 48-typescript-tsx-runtime-symbol-surface, 49-typescript-tsx-precise-scip-parity
- Global scope:
  - crates/cli/src/indexer/mod.rs
  - crates/cli/src/searcher/mod.rs
  - crates/cli/src/watch.rs
  - crates/cli/src/storage/mod.rs
  - crates/cli/src/mcp/deep_search.rs
  - crates/cli/tests/
  - crates/cli/benches/
  - fixtures/playbooks/
  - benchmarks/
  - contracts/
  - docs/overview.md
  - README.md
  - scripts/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- [ ] T001: Extend semantic chunk eligibility, hybrid language normalization, and watch invalidation for TypeScript/TSX (owner: unassigned) (scope: crates/cli/src/indexer/mod.rs, crates/cli/src/searcher/mod.rs, crates/cli/src/watch.rs, crates/cli/src/storage/mod.rs) (depends: -)
  - Context: semantic chunk classification currently skips `.ts` and `.tsx`, and watch-driven semantic refresh eligibility follows that classifier. End-to-end parity requires semantic indexing, hybrid retrieval, and changed-only/watch invalidation to agree on the same TypeScript language-family rules.
  - Reuse_targets: `semantic_chunk_language_for_path`, `NormalizedLanguage`, changed-only manifest diff handling, watch semantic-refresh gating
  - Autonomy: high
  - Risk: medium
  - Complexity: medium
  - DoD: TypeScript/TSX become semantic chunk candidates, hybrid filters normalize `typescript|ts|tsx`, and changed-only/watch paths invalidate stale TypeScript/TSX semantic state correctly.
  - Validation: `cargo test -p frigg semantic_`, `cargo test -p frigg watch_`
  - Escalate if: TypeScript/TSX semantic chunk quality requires a language-specific splitter instead of the current chunking model.

- [ ] T002: Add end-to-end TypeScript/TSX regression suites for hybrid search, stale-result invalidation, provenance, and replay (owner: unassigned) (scope: crates/cli/tests/, crates/cli/tests/fixtures/, fixtures/playbooks/) (depends: T001)
  - Context: full parity needs more than isolated unit tests. The repo should prove TypeScript/TSX behavior through hybrid search, changed-only invalidation, provenance rows, and replay/deep-search traces under realistic temporary workspaces or fixture playbooks.
  - Reuse_targets: `tool_handlers.rs`, `provenance.rs`, `deep_search_replay.rs`, playbook hybrid suites, existing stale-manifest regression helpers
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: deterministic TypeScript/TSX regression coverage exists for hybrid semantic on/off/degraded behavior, stale-result invalidation after `.ts`/`.tsx` edits, provenance payload determinism, and replay or deep-search traces backed by TypeScript/TSX tool calls.
  - Validation: `cargo test -p frigg --test tool_handlers`, `cargo test -p frigg --test provenance`, `cargo test -p frigg --test deep_search_replay`, `cargo test -p frigg --test playbook_hybrid_suite`
  - Escalate if: fixture volume becomes too large for inline workspaces and a dedicated mini repo under `fixtures/repos/` is required.

- [ ] T003: Add TypeScript/TSX benchmark workloads and report coverage (owner: unassigned) (scope: crates/cli/benches/, benchmarks/, scripts/) (depends: T001, T002)
  - Context: TypeScript/TSX parity changes parser cost, semantic chunk generation, and hybrid ranking witness sets. Benchmark/report contracts should account for that explicitly instead of relying on Rust/PHP-only workloads.
  - Reuse_targets: `tool_latency.rs`, `search_latency.rs`, `benchmarks/budgets.v1.json`, `benchmarks/generate_latency_report.py`, existing report-generation flow
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: representative TypeScript/TSX workloads exist for MCP/search latency benches, budget/report inputs are updated, and release-readiness artifacts account for the new workload IDs.
  - Validation: `cargo bench -p frigg --bench tool_latency -- --noplot`, `cargo bench -p frigg --bench search_latency -- --noplot`, `python3 benchmarks/generate_latency_report.py`
  - Escalate if: benchmark runtime grows enough that the new TypeScript/TSX workloads need sampling or a lighter representative set.

- [ ] T004: Sync TypeScript/TSX end-to-end docs, contracts, and release-gate guidance (owner: unassigned) (scope: contracts/, docs/overview.md, README.md, specs/index.md, contracts/changelog.md, specs/50-typescript-tsx-semantic-e2e-parity/, scripts/) (depends: T003)
  - Context: docs stay live. Once TypeScript/TSX claims extend through runtime symbols, precise SCIP, semantic indexing, watch correctness, and replay/benchmark coverage, the public support matrix must be synchronized in one change set.
  - Reuse_targets: `contracts/tools/v1/README.md`, `contracts/errors.md`, `contracts/changelog.md`, `docs/overview.md`, `README.md`, release-ready/docs-sync guidance
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - DoD: public docs/contracts/program index describe the validated TypeScript/TSX support matrix end-to-end, including semantic/hybrid behavior and the lack of a Node runtime dependency, without implying JS/JSX support.
  - Validation: `just docs-sync`, `just release-ready`
  - Escalate if: documentation language would imply JavaScript/JSX support or external toolchain requirements that are not actually part of the implemented surface.

## Done

- (none)
