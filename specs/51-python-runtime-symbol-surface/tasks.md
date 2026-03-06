# Tasks — 51-python-runtime-symbol-surface

Meta:
- Spec: 51-python-runtime-symbol-surface — Python Runtime Symbol Surface
- Depends on: 34-readonly-ide-navigation-tools, 40-symbol-resolution-indexes, 42-manifest-freshness-validation
- Global scope:
  - Cargo.toml
  - crates/cli/Cargo.toml
  - crates/cli/src/indexer/mod.rs
  - crates/cli/src/mcp/
  - crates/cli/src/searcher/mod.rs
  - crates/cli/tests/
  - contracts/
  - docs/overview.md
  - docs/phases.md
  - README.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- [ ] T001: Add `tree-sitter-python` and implement Python parser and symbol extraction coverage (owner: unassigned) (scope: Cargo.toml, crates/cli/Cargo.toml, crates/cli/src/indexer/mod.rs) (depends: -)
  - Context: Python is not currently part of the runtime symbol surface. There is no parser dependency, `.py` is excluded from `SymbolLanguage::from_path()`, and the symbol extractor has no Python AST mapping today.
  - Reuse_targets: `SymbolLanguage`, `extract_symbols_from_source`, `node_name_text`, `stable_symbol_id`, `symbol_definition_order`
  - Autonomy: high
  - Risk: medium
  - Complexity: medium
  - DoD: `tree-sitter-python` is wired into the workspace and CLI crate, `.py` is recognized as a source language, Python symbol extraction covers the declaration set promised by the requirements, and deterministic extraction tests exist.
  - Validation: `cargo test -p frigg symbols_`, `cargo test -p frigg structural_search_`
  - Escalate if: Python declaration coverage requires a new public `SymbolKind` value and the downstream contract impact is unclear.

- [ ] T002: Wire Python through runtime symbol corpora and public tool-language filters (owner: unassigned) (scope: crates/cli/src/mcp/, crates/cli/src/searcher/mod.rs) (depends: T001)
  - Context: runtime surfaces currently hard-code Rust/PHP in `document_symbols`, structural-search language parsing, and general language-filter normalization. Python needs to enter those public filter and validation paths without implying precise or semantic parity.
  - Reuse_targets: `collect_repository_symbol_corpus`, `parse_symbol_language`, `NormalizedLanguage`, `document_symbols`, `search_structural`, `search_symbol`, `resolve_navigation_target`
  - Autonomy: high
  - Risk: medium
  - Complexity: medium
  - DoD: Python files enter warm symbol corpora, `document_symbols` and `search_structural` accept `.py`, public filters normalize `python|py`, and note/error payloads remain deterministic.
  - Validation: `cargo test -p frigg --test tool_handlers`
  - Escalate if: supporting Python in `search_hybrid` language filters would create misleading semantic expectations that need a contract change or explicit note field.

- [ ] T003: Add Python runtime regression matrix and provenance coverage (owner: unassigned) (scope: crates/cli/tests/, crates/cli/src/indexer/mod.rs) (depends: T001, T002)
  - Context: Python support needs deterministic regression coverage for extraction, symbol search, heuristic references, location-based navigation, structural search, and at least one provenance-emitting follow-up path.
  - Reuse_targets: existing inline language fixtures in `indexer`, `tool_handlers.rs` navigation/search helpers, `provenance.rs` event-shape assertions
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: deterministic Python regression coverage exists for symbol extraction, `search_symbol`, heuristic `find_references`, location-based `go_to_definition`, `document_symbols`, `search_structural`, and one provenance payload path.
  - Validation: `cargo test -p frigg --test tool_handlers`, `cargo test -p frigg --test provenance`, `cargo test -p frigg`
  - Escalate if: inline fixtures are no longer maintainable and a dedicated Python mini repo under `fixtures/repos/` is needed.

- [ ] T004: Sync Python runtime L1/L2 contracts, roadmap notes, and operator docs (owner: unassigned) (scope: contracts/, docs/overview.md, docs/phases.md, README.md, specs/index.md, specs/51-python-runtime-symbol-surface/) (depends: T002, T003)
  - Context: the public docs currently do not position Python on the language roadmap and still describe the symbol/navigation runtime as Rust/PHP-only. The update needs to be explicit that Python is runtime L1/L2 only in this first spec.
  - Reuse_targets: `contracts/tools/v1/README.md`, `contracts/errors.md`, `contracts/changelog.md`, `docs/overview.md`, `docs/phases.md`, `README.md`
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - DoD: public docs/contracts/changelog reflect Python runtime L1/L2 support accurately, roadmap language notes stay honest, and the program index records the new spec without implying Python precise SCIP or semantic parity.
  - Validation: `cargo test -p frigg schema_`, `just docs-sync`
  - Escalate if: docs language would imply `.pyi`, notebooks, precise SCIP, or semantic-runtime parity that this spec does not actually deliver.

## Done

- (none)
