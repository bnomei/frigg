# Frigg Program Index

Updated: 2026-03-06
Sync note with date: 2026-03-06 (added follow-on specs 48-51 for TypeScript/TSX parity plus Python runtime onboarding after the watch/integration and quiet-stdio follow-up wave)
Execution mode target: adaptive (cap: 4, bundle depth: 2)

## Program objective
Deliver a local-first, privacy-conscious Deep Search + Code Search + Code Graph MCP system in Rust with deterministic tool outputs, provenance, typed errors, and production-grade security/performance gates, then continuously close post-implementation security/perf/contract gaps via spec-led remediation.

## Spec graph (no phased rollout)

| Spec | Goal | Depends on | Primary write scope |
| --- | --- | --- | --- |
| `00-contracts-and-governance` | Tool/config/error contracts and sync rules | - | `contracts/`, `scripts/` |
| `01-storage-and-repo-state` | SQLite schema + repo/snapshot state lifecycle | - | `crates/storage/`, `crates/cli/` |
| `02-ingestion-and-incremental-index` | Ignore-aware manifest + changed-file indexing | `01-storage-and-repo-state` | `crates/index/`, `fixtures/repos/` |
| `03-text-search-engine` | Deterministic literal/regex search + ranking baseline | `02-ingestion-and-incremental-index` | `crates/search/`, `crates/index/` |
| `04-symbol-graph-heuristic-nav` | L1/L2 symbol graph and heuristic references | `02-ingestion-and-incremental-index` | `crates/index/`, `crates/graph/` |
| `05-scip-precision-ingest` | L3 precise references via SCIP ingestion | `04-symbol-graph-heuristic-nav` | `crates/graph/`, `fixtures/scip/` |
| `06-embeddings-and-vector-store` | Provider abstraction (OpenAI/Google) + sqlite-vec | `01-storage-and-repo-state` | `crates/embeddings/`, `crates/storage/` |
| `07-mcp-server-and-tool-contracts` | MCP tool handlers, stdio/http transport, provenance | `00-contracts-and-governance`, `01-storage-and-repo-state` | `crates/mcp/`, `crates/cli/` |
| `08-hybrid-retrieval-and-deep-search-harness` | Hybrid retrieval and replayable deep-search loop harness | `03-text-search-engine`, `04-symbol-graph-heuristic-nav`, `05-scip-precision-ingest`, `06-embeddings-and-vector-store`, `07-mcp-server-and-tool-contracts` | `crates/search/`, `crates/mcp/`, `fixtures/playbooks/` |
| `09-security-benchmarks-ops` | Security tests, perf budgets, operability commands | `03-text-search-engine`, `04-symbol-graph-heuristic-nav`, `06-embeddings-and-vector-store`, `07-mcp-server-and-tool-contracts` | `docs/security/`, `benchmarks/`, `scripts/`, `crates/cli/` |
| `10-mcp-surface-hardening` | HTTP/auth/path/regex hardening + typed error envelope consistency | `07-mcp-server-and-tool-contracts`, `09-security-benchmarks-ops` | `crates/cli/`, `crates/mcp/` |
| `11-mcp-hotpath-caching-and-provenance` | Symbol/SCIP/provenance hot-path caching and storage index tuning | `10-mcp-surface-hardening` | `crates/mcp/`, `crates/storage/` |
| `12-search-index-hotpath-and-correctness` | Search/index hot-loop performance and diagnostics/correctness fixes | `03-text-search-engine`, `04-symbol-graph-heuristic-nav` | `crates/search/`, `crates/index/`, `crates/mcp/` |
| `13-contract-and-doc-drift-closure` | Contract/docs drift closure and citation field consistency | `00-contracts-and-governance`, `08-hybrid-retrieval-and-deep-search-harness` | `contracts/`, `docs/`, `specs/`, `crates/mcp/` |
| `14-benchmark-coverage-expansion` | Add missing benchmark workloads and budget/report coverage | `09-security-benchmarks-ops`, `11-mcp-hotpath-caching-and-provenance`, `12-search-index-hotpath-and-correctness` | `crates/*/benches/`, `benchmarks/` |
| `15-heuristic-fallback-scale` | Fix heuristic fallback superlinear behavior and memory pressure | `04-symbol-graph-heuristic-nav`, `11-mcp-hotpath-caching-and-provenance` | `crates/index/`, `crates/mcp/` |
| `16-symbol-corpus-cache-fastpath` | Remove expensive cache-hit rebuild/clone behavior in symbol corpus caching | `11-mcp-hotpath-caching-and-provenance` | `crates/mcp/`, `crates/index/` |
| `17-symlink-safe-provenance-paths` | Enforce symlink-safe canonical boundaries for provenance storage writes | `10-mcp-surface-hardening`, `11-mcp-hotpath-caching-and-provenance` | `crates/mcp/`, `crates/storage/`, `crates/cli/` |
| `18-provenance-strict-persistence` | Make provenance persistence strict-by-default with typed failure surface | `11-mcp-hotpath-caching-and-provenance` | `crates/mcp/` |
| `19-mcp-async-blocking-isolation` | Isolate blocking work from async MCP handler threads | `07-mcp-server-and-tool-contracts`, `11-mcp-hotpath-caching-and-provenance` | `crates/mcp/` |
| `20-reindex-resilience-diagnostics` | Preserve reindex progress on unreadable files with typed diagnostics | `02-ingestion-and-incremental-index` | `crates/index/`, `crates/cli/`, `contracts/` |
| `21-vector-backend-migration-safety` | Harden sqlite-vec/fallback backend transitions with migration-safe semantics | `01-storage-and-repo-state`, `06-embeddings-and-vector-store` | `crates/storage/`, `contracts/` |
| `22-tool-path-semantics-unification` | Unify path contract across core MCP tools | `07-mcp-server-and-tool-contracts`, `13-contract-and-doc-drift-closure` | `crates/mcp/`, `crates/search/`, `contracts/` |
| `23-smoke-ops-fresh-binary` | Fail smoke ops on build failures instead of stale-binary fallback | `09-security-benchmarks-ops` | `scripts/` |
| `24-find-references-resource-budgets` | Add deterministic resource guards to `find_references` artifact/source processing | `10-mcp-surface-hardening`, `11-mcp-hotpath-caching-and-provenance` | `crates/mcp/`, `crates/graph/` |
| `25-release-gate-execution-hardening` | Convert release gate to execution-backed freshness checks | `09-security-benchmarks-ops`, `14-benchmark-coverage-expansion` | `scripts/`, `docs/security/`, `benchmarks/` |
| `26-provenance-target-integrity` | Prevent provenance misattribution for invalid repository hints | `11-mcp-hotpath-caching-and-provenance` | `crates/mcp/`, `crates/mcp/tests/` |
| `27-doc-contract-sync-wave2` | Close remaining semantic/benchmark/storage contract drift | `13-contract-and-doc-drift-closure` | `contracts/`, `benchmarks/`, `specs/06-embeddings-and-vector-store/` |
| `28-storage-error-trace-diff-corrections` | Correct error classification and trace diff structural checks | `00-contracts-and-governance`, `08-hybrid-retrieval-and-deep-search-harness` | `crates/storage/`, `crates/mcp/`, `contracts/` |
| `29-graph-storage-benchmark-expansion` | Add graph/storage hot-path benchmarks and report integration | `14-benchmark-coverage-expansion` | `crates/graph/benches/`, `crates/storage/benches/`, `benchmarks/` |
| `30-citation-hygiene-gate` | Enforce deterministic offline citation hygiene checks for overview fact-check registry | `13-contract-and-doc-drift-closure`, `27-doc-contract-sync-wave2` | `scripts/`, `docs/overview.md`, `docs/security/`, `Justfile` |
| `31-write-surface-security-gates` | Add enforceable policy/test gates for future write-capable MCP tools | `09-security-benchmarks-ops`, `10-mcp-surface-hardening`, `25-release-gate-execution-hardening` | `contracts/`, `docs/security/`, `scripts/`, `crates/mcp/` |
| `32-sqlite-vec-production-hardening` | Enforce sqlite-vec runtime registration/version/startup hardening | `06-embeddings-and-vector-store`, `21-vector-backend-migration-safety`, `25-release-gate-execution-hardening` | `crates/storage/`, `crates/embeddings/`, `crates/cli/`, `scripts/`, `contracts/` |
| `33-regex-trigram-bitmap-acceleration` | Add deterministic trigram/bitmap regex prefilter + benchmark coverage | `03-text-search-engine`, `12-search-index-hotpath-and-correctness`, `14-benchmark-coverage-expansion` | `crates/search/`, `benchmarks/`, `contracts/`, `docs/overview.md` |
| `34-readonly-ide-navigation-tools` | Add read-only IDE-style navigation/query MCP tools (`go_to_definition`, `find_declarations`, `find_implementations`, call hierarchy, `document_symbols`, `search_structural`) | `04-symbol-graph-heuristic-nav`, `05-scip-precision-ingest`, `07-mcp-server-and-tool-contracts`, `22-tool-path-semantics-unification` | `crates/mcp/`, `crates/graph/`, `crates/index/`, `contracts/`, `benchmarks/` |
| `35-semantic-runtime-mcp-surface` | Expose Track C semantic retrieval as deterministic runtime capability (`search_hybrid`, semantic indexing/query composition) | `06-embeddings-and-vector-store`, `08-hybrid-retrieval-and-deep-search-harness`, `32-sqlite-vec-production-hardening` | `crates/cli/`, `crates/config/`, `crates/index/`, `crates/search/`, `crates/storage/`, `crates/mcp/`, `contracts/`, `benchmarks/` |
| `36-deep-search-runtime-tools` | Promote deep-search harness APIs to optional runtime MCP tools (`deep_search_run`, replay diff, citation compose) | `08-hybrid-retrieval-and-deep-search-harness`, `10-mcp-surface-hardening` | `crates/cli/`, `crates/mcp/`, `contracts/`, `benchmarks/`, `docs/overview.md` |
| `37-public-surface-parity-gates` | Enforce runtime/schema/docs tool-surface parity with deterministic gate failures | `34-readonly-ide-navigation-tools`, `35-semantic-runtime-mcp-surface`, `36-deep-search-runtime-tools` | `crates/mcp/`, `crates/cli/`, `contracts/tools/v1/`, `docs/overview.md`, `scripts/`, `Justfile` |
| `39-performance-memory-hardening` | Eliminate audited full-walk/full-buffer hot paths across search, navigation, SCIP ingest, semantic retrieval, and file reads | `24-find-references-resource-budgets`, `33-regex-trigram-bitmap-acceleration`, `34-readonly-ide-navigation-tools`, `35-semantic-runtime-mcp-surface`, `38-single-crate-consolidation` | `crates/cli/src/{indexer,searcher,mcp,graph,storage}/`, `contracts/`, `benchmarks/`, `README.md` |
| `40-symbol-resolution-indexes` | Replace warm-corpus symbol search and target-resolution linear scans with deterministic lookup indexes | `34-readonly-ide-navigation-tools`, `39-performance-memory-hardening` | `crates/cli/src/{mcp,indexer}/`, `contracts/`, `benchmarks/` |
| `41-partial-precise-degradation` | Retain successful precise SCIP data and make handlers partial-aware instead of clearing the whole precise overlay | `05-scip-precision-ingest`, `24-find-references-resource-budgets`, `34-readonly-ide-navigation-tools`, `39-performance-memory-hardening` | `crates/cli/src/{mcp,graph}/`, `contracts/`, `benchmarks/` |
| `42-manifest-freshness-validation` | Validate persisted manifest snapshot freshness before symbol/search fast-path reuse | `16-symbol-corpus-cache-fastpath`, `39-performance-memory-hardening` | `crates/cli/src/{mcp,searcher}/`, `contracts/`, `benchmarks/` |
| `43-document-symbols-byte-guards` | Add deterministic byte-budget guards to `document_symbols` before full-file reads | `34-readonly-ide-navigation-tools`, `39-performance-memory-hardening` | `crates/cli/src/mcp/`, `contracts/`, `benchmarks/` |
| `44-integrated-local-watch-mode` | Add built-in local watch scheduling for stdio and loopback HTTP over the existing changed-only reindex path | `07-mcp-server-and-tool-contracts`, `20-reindex-resilience-diagnostics`, `42-manifest-freshness-validation` | `crates/cli/src/{main,settings,watch}.rs`, `README.md`, `specs/` |
| `45-watch-driven-changed-reindex-correctness` | Preserve manifest, semantic, docs-visible, and ignored-path correctness under watcher-triggered changed-only refreshes | `02-ingestion-and-incremental-index`, `35-semantic-runtime-mcp-surface`, `42-manifest-freshness-validation`, `44-integrated-local-watch-mode` | `crates/cli/src/{indexer,searcher,storage,watch}/`, `crates/cli/tests/`, `README.md` |
| `47-session-workspace-attach-and-stdio-defaults` | Replace static startup-root dependence for MCP serving with session-driven workspace attach, stdio cwd/git-root auto-selection, and stdio watch-off one-shot defaults | `07-mcp-server-and-tool-contracts`, `10-mcp-surface-hardening`, `22-tool-path-semantics-unification`, `44-integrated-local-watch-mode` | `crates/cli/src/{main,settings,mcp}/`, `contracts/`, `README.md`, `specs/` |
| `48-typescript-tsx-runtime-symbol-surface` | Onboard `.ts` and `.tsx` into the symbol corpus and L1/L2 read-only runtime navigation/query surfaces | `34-readonly-ide-navigation-tools`, `40-symbol-resolution-indexes`, `42-manifest-freshness-validation` | `crates/cli/src/{indexer,mcp,searcher}/`, `crates/cli/tests/`, `contracts/`, `docs/`, `README.md` |
| `49-typescript-tsx-precise-scip-parity` | Validate and harden precise TypeScript/TSX navigation/reference parity via SCIP artifacts | `05-scip-precision-ingest`, `41-partial-precise-degradation`, `48-typescript-tsx-runtime-symbol-surface` | `crates/cli/src/{graph,storage,mcp}/`, `crates/cli/tests/`, `fixtures/scip/`, `contracts/`, `docs/`, `README.md` |
| `50-typescript-tsx-semantic-e2e-parity` | Close TypeScript/TSX semantic, watch/reindex, provenance, benchmark, and release-gate parity end-to-end | `35-semantic-runtime-mcp-surface`, `45-watch-driven-changed-reindex-correctness`, `48-typescript-tsx-runtime-symbol-surface`, `49-typescript-tsx-precise-scip-parity` | `crates/cli/src/{indexer,searcher,watch,storage,mcp}/`, `crates/cli/tests/`, `crates/cli/benches/`, `benchmarks/`, `contracts/`, `docs/`, `README.md`, `scripts/` |
| `51-python-runtime-symbol-surface` | Add `.py` runtime L1/L2 symbol and navigation support without taking on Python precise SCIP or semantic parity yet | `34-readonly-ide-navigation-tools`, `40-symbol-resolution-indexes`, `42-manifest-freshness-validation` | `Cargo.toml`, `crates/cli/Cargo.toml`, `crates/cli/src/{indexer,mcp,searcher}/`, `crates/cli/tests/`, `contracts/`, `docs/`, `README.md` |

## Parallel dispatch guidance
- Wave A (ready now): `00/T001`, `01/T001`, `06/T001`, `07/T001`
- Wave B (after Wave A completions): `00/T002`, `01/T002`, `06/T002`, `07/T002`
- Wave C (fan-out): `02/*`, `09/T001`
- Wave D (parallel search/nav): `03/*` and `04/*`
- Wave E: `05/*` and `09/T002-T004`
- Wave F: `08/*` plus integration closures in `07` and `06`
- Wave G (remediation): `10/T001-T003`, `12/T001-T002`, `13/T001-T004` in parallel by disjoint scope
- Wave H: `10/T004`, `11/*` (server/storage hot-paths)
- Wave I: `14/*` after `11` and `12` complete
- Wave J (review wave 2, disjoint now): `20/T001`, `23/T001`, `25/T001`, `27/T001`
- Wave K: `21/T001`, `28/T001`, `29/T001`
- Wave L: `17/T001`, `18/T001`, `22/T001`, `24/T001`, `26/T001`
- Wave M: `15/T001`, `16/T001`, `19/T001`
- Wave N (final closeout): `30/*`, `31/*`, `32/*`, `33/*` on disjoint scope with adaptive cap
- Wave O (next read-only parity wave): `34/*` after mayor confirmation and capacity availability
- Wave P (vision-to-fact closure): `35/*` and `36/*` in parallel by disjoint scope, then `37/*` as mandatory parity lock
- Wave Q (performance hardening): `39/T001-T003` in parallel by disjoint scope, then `39/T004-T006` after cache and candidate foundations land
- Wave R (residual post-hardening closures): `40/T001`, `42/T001`, and `43/T001` in parallel by disjoint scope, then `40/T002`, `41/*`, `42/T002`, and `43/T002` after the new lookup and freshness foundations land
- Wave S (local watch integration): `44/T001`, `45/T001`, and `45/T003` in parallel by disjoint scope, then `44/T002-T004`, `45/T002`, and `45/T004` after the config and correctness guardrails land
- Wave T (workspace-selection UX): `47/T001` and `47/T002` first, then `47/T003-T005` once serving-mode empty-root startup and attach contracts are stable
- Wave U (TypeScript/TSX parity): `48/*` first, then `49/*`, then `50/*` once runtime symbol and precise parity are stable
- Wave V (Python runtime onboarding): `51/*` as a standalone L1/L2 experiment; defer any Python precise SCIP or semantic parity follow-ons until the runtime slice proves useful

## Non-negotiable acceptance
- Every tool has versioned schema + typed public errors.
- Security tests cover path traversal, regex abuse, origin/auth boundaries, and write-confirmation behavior.
- Benchmarks publish p50/p95/p99 budgets and measured values.
- Provenance traces are emitted for validation playbooks and replayable.
- Post-review critical/high findings are tracked as specs with validated closures before release tagging.
