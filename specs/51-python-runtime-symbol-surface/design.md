# Design — 51-python-runtime-symbol-surface

## Scope
- Cargo.toml
- crates/cli/Cargo.toml
- crates/cli/src/indexer/mod.rs
- crates/cli/src/mcp/server.rs
- crates/cli/src/mcp/types.rs
- crates/cli/src/searcher/mod.rs
- crates/cli/tests/tool_handlers.rs
- crates/cli/tests/provenance.rs
- contracts/
- docs/overview.md
- docs/phases.md
- README.md

## Normative excerpt (in-repo)
- `docs/phases.md` defines language support levels (`L0` through `L3`) and the onboarding checklist: file detection, symbol coverage, reference strategy, fixtures, and benchmarks.
- The current runtime symbol path still stops at Rust/PHP, and current public contracts describe `document_symbols` and `search_structural` as Rust/PHP-only.
- Hybrid ranking already treats `.py` as runtime-like path evidence, but Python is not wired into symbol extraction, runtime language filters, or semantic chunking.

## Architecture decisions
1. Add `tree-sitter-python` as a workspace dependency and wire it into the CLI crate. Python support should come from tree-sitter parsing and existing Rust runtime infrastructure, not from invoking Python tooling.
2. Treat `.py` as one logical public language family, `python`. Public filters accept `python` and `py`; no additional public split is introduced in this spec.
3. Scope this spec to `.py` only. `.pyi`, notebooks, generated stubs, and packaging/import-resolution workflows are explicitly out of scope.
4. Symbol extraction should cover the Python declaration shapes most useful for current Frigg tools:
   - module/file -> `module`
   - `class_definition` -> `class`
   - `function_definition` and `async_function_definition` -> `function` or `method` depending on container
   - decorated definitions -> underlying class/function symbol with the declared name
   - simple identifier-bound module constants and class attributes -> `const`, `property`, or a new `variable` kind only if current vocabulary proves insufficient
5. Stable symbol IDs keep the current shape: hash logical language, kind, canonical path, extracted name, and span.
6. Heuristic references reuse the current identifier-token and containing-symbol flow. Import aliases, dynamic attribute access, and runtime metaprogramming remain best-effort within L2.
7. `search_hybrid` may accept `python|py` for lexical and graph narrowing in this spec, but semantic chunking and semantic parity remain out of scope and must be documented as such.

## Runtime integration plan

### Parser and symbol extraction
- Extend `SymbolLanguage::from_path()` so `.py` enters:
  - single-file extraction helpers
  - manifest-backed source-path collection
  - warm symbol-corpus rebuilds and path-local symbol indexes
- Add Python parser dispatch with `tree-sitter-python`.
- Map the Python AST nodes required by the requirements to the existing symbol model, adding a new public kind only if module-level mutable bindings cannot be represented honestly.

### Navigation and structural search
- `document_symbols` should accept `.py` as a supported extension and keep the current flat deterministic outline shape.
- `search_structural` should accept `python` and `py` aliases and run strict tree-sitter query mode over Python files.
- `search_symbol` and location-based navigation should work automatically once Python symbols enter the corpus and per-path indexes.
- Heuristic references should reuse existing lexical token scanning for identifier-like symbols; the public contract should remain explicit that Python support in this spec is L2, not precise.

### Public contract updates
- Extend runtime language normalization in MCP/search layers to accept `python|py` where a public `language` filter already exists.
- Update operator docs and tool contracts to say Python is supported for runtime L1/L2 surfaces covered by this spec only.
- Keep precise SCIP, semantic indexing, and watch/benchmark parity out of scope so the public docs stay honest.

## Validation and rollout
- Add deterministic indexer unit tests for Python symbol extraction and structural search with inline fixtures.
- Add integration coverage for `search_symbol`, heuristic `find_references`, location-based `go_to_definition`, `document_symbols`, and `search_structural`.
- Update docs/contracts in the same change set, with explicit notes that Python precise SCIP and semantic parity remain future work if the runtime L1/L2 slice proves worthwhile.
