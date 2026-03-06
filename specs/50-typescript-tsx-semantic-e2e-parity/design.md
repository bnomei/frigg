# Design — 50-typescript-tsx-semantic-e2e-parity

## Scope
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

## Normative excerpt (in-repo)
- `specs/35-semantic-runtime-mcp-surface` already defines the semantic runtime contract and `search_hybrid` behavior.
- `specs/45-watch-driven-changed-reindex-correctness` already defines changed-only and watch correctness expectations for manifest, semantic, and docs-visible state.
- Frigg's roadmap and docs-sync rules require docs, contracts, and benchmark claims to stay live as capabilities expand.

## Architecture decisions
1. Classify `.ts` and `.tsx` semantic chunks under one logical language name, `typescript`, and reuse the existing chunk identity and replacement model. No TypeScript-specific semantic table or MCP tool variant is introduced.
2. Reuse the same changed-only and watch invalidation path for TypeScript/TSX as existing semantic languages. The semantic eligibility predicate must align with the runtime language-family normalization added in spec 48.
3. Keep provenance, deep-search, and replay payload shapes unchanged. TypeScript/TSX parity must fit the current tool identities, note metadata, and citation payload structure.
4. Add benchmark workloads only where TypeScript/TSX materially change parser cost, chunk-generation behavior, or ranking inputs. Do not duplicate every existing workload blindly.
5. Preserve the "no Node runtime dependency" principle. Frigg's support comes from tree-sitter parsing, SCIP artifacts, and existing semantic providers, not from invoking `tsc` or a JavaScript toolchain at runtime.

## End-to-end parity plan

### Semantic indexing and retrieval
- Extend semantic chunk eligibility so `.ts` and `.tsx` files produce chunk candidates during full and changed-only reindex.
- Keep the logical chunk language stable as `typescript` so hybrid-search filtering and stored semantic metadata align.
- Allow `search_hybrid` language filters to accept `typescript`, `ts`, and `tsx` without semantic-channel drift between lexical, graph, and semantic paths.

### Changed-only and watch lifecycle
- Reuse the existing manifest diff and watch scheduling flow.
- Ensure TypeScript/TSX edits, renames, and deletions invalidate:
  - warm symbol corpora
  - precise overlays whose documents no longer match the manifest
  - semantic chunks and embeddings tied to removed or changed files
- Add stale-result regression coverage so changed-only and watch paths cannot leave outdated TypeScript/TSX hits behind.

### Provenance, replay, and deep-search
- Prove that TypeScript/TSX-backed `search_symbol`, `find_references`, `go_to_definition`, and `search_hybrid` calls emit the same deterministic provenance envelope already used for other languages.
- Add replay or playbook coverage that exercises TypeScript/TSX tool responses through the existing deep-search trace model.
- Keep citation composition unchanged; TypeScript/TSX parity must show up as ordinary source-backed evidence, not a new citation mode.

### Benchmarks and gates
- Add representative TypeScript/TSX workloads to search and MCP latency benches where language-specific cost or ranking behavior changes.
- Update benchmark budgets, generated reports, and release-readiness inputs in the same change set.
- Keep docs/contracts synchronized so public support claims match the validated workload matrix.

## Validation and rollout
- Add integration suites for semantic-on, semantic-off, degraded, and strict-failure paths with TypeScript/TSX content.
- Add changed-only/watch regression suites for TypeScript/TSX stale-result invalidation.
- Add provenance/deep-search replay coverage for TypeScript/TSX-backed traces.
- Update benchmark/report artifacts and docs/contracts together so release gates stay truthful.
