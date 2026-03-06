# Tasks — 48-typescript-tsx-runtime-symbol-surface

Meta:
- Spec: 48-typescript-tsx-runtime-symbol-surface — TypeScript/TSX Runtime Symbol Surface
- Depends on: 34-readonly-ide-navigation-tools, 40-symbol-resolution-indexes, 42-manifest-freshness-validation
- Global scope:
  - crates/cli/src/indexer/mod.rs
  - crates/cli/src/mcp/
  - crates/cli/src/searcher/mod.rs
  - crates/cli/tests/
  - contracts/
  - docs/overview.md
  - README.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- [ ] T001: Extend TypeScript/TSX language detection, parser dispatch, and symbol extraction (owner: unassigned) (scope: crates/cli/src/indexer/mod.rs) (depends: -)
  - Context: TypeScript is already the roadmap's secondary language target, and `tree-sitter-typescript` is already present in the crate. The missing work is deterministic runtime wiring: file detection still stops at Rust/PHP, parser dispatch is Rust/PHP-only, and no TypeScript declaration mapping exists in the symbol extractor.
  - Reuse_targets: `SymbolLanguage`, `extract_symbols_from_source`, `node_name_text`, `stable_symbol_id`, `symbol_definition_order`
  - Autonomy: high
  - Risk: medium
  - Complexity: medium
  - DoD: `.ts` and `.tsx` are recognized as supported source files; parser dispatch is extension-aware; the extractor covers the declaration set promised by the requirements; deterministic TS/TSX unit coverage exists.
  - Validation: `cargo test -p frigg symbols_`, `cargo test -p frigg structural_search_`
  - Escalate if: a new public `SymbolKind` value is required and the downstream contract/doc impact is unclear.

- [ ] T002: Wire TypeScript/TSX through runtime symbol corpora and read-only navigation/query tools (owner: unassigned) (scope: crates/cli/src/mcp/, crates/cli/src/searcher/mod.rs) (depends: T001)
  - Context: runtime surfaces still hard-code Rust/PHP support in language validation, `document_symbols`, and `search_structural`; TypeScript aliases are rejected in current `language` filter normalization paths.
  - Reuse_targets: `collect_repository_symbol_corpus`, `parse_symbol_language`, `NormalizedLanguage`, `document_symbols`, `search_structural`, `search_symbol`, `resolve_navigation_target`
  - Autonomy: high
  - Risk: medium
  - Complexity: medium
  - DoD: TypeScript/TSX files enter warm symbol corpora; `document_symbols` and `search_structural` accept them; public `language` filters normalize `typescript|ts|tsx`; note and error payloads remain deterministic.
  - Validation: `cargo test -p frigg --test tool_handlers`
  - Escalate if: alias-only support is insufficient and a schema-level public language split between `typescript` and `tsx` appears necessary.

- [ ] T003: Add TypeScript/TSX runtime regression matrix and provenance coverage (owner: unassigned) (scope: crates/cli/tests/, crates/cli/src/indexer/mod.rs) (depends: T001, T002)
  - Context: current regression coverage is Rust/PHP-heavy. Full runtime onboarding needs deterministic tests for extraction, symbol search, heuristic references, location-based navigation, structural search, and at least one provenance-emitting follow-up path.
  - Reuse_targets: existing inline language fixtures in `indexer`, `tool_handlers.rs` navigation/search helpers, `provenance.rs` event-shape assertions
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: deterministic TS/TSX regression coverage exists for symbol extraction, `search_symbol`, heuristic `find_references`, location-based `go_to_definition`, `document_symbols`, `search_structural`, and one provenance payload path.
  - Validation: `cargo test -p frigg --test tool_handlers`, `cargo test -p frigg --test provenance`, `cargo test -p frigg`
  - Escalate if: inline fixtures become too noisy and a dedicated mini repo under `fixtures/repos/` is needed.

- [ ] T004: Sync TypeScript/TSX runtime contracts and operator docs (owner: unassigned) (scope: contracts/, docs/overview.md, README.md, specs/index.md, specs/48-typescript-tsx-runtime-symbol-surface/) (depends: T002, T003)
  - Context: public docs currently claim Rust/PHP-only support for `document_symbols` and `search_structural`, and they do not describe TypeScript alias normalization in runtime filter paths.
  - Reuse_targets: `contracts/tools/v1/README.md`, `contracts/errors.md`, `contracts/changelog.md`, `docs/overview.md`, `README.md`
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - DoD: public docs/contracts/changelog reflect actual TypeScript/TSX runtime support, supported extension/error details are updated, and the program index records the new spec without reopening completed specs.
  - Validation: `cargo test -p frigg schema_`, `just docs-sync`
  - Escalate if: documentation language would overstate precise SCIP or semantic support that is intentionally deferred to specs 49 and 50.

## Done

- (none)
