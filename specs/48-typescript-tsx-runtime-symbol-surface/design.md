# Design — 48-typescript-tsx-runtime-symbol-surface

## Scope
- crates/cli/src/indexer/mod.rs
- crates/cli/src/mcp/server.rs
- crates/cli/src/mcp/types.rs
- crates/cli/src/searcher/mod.rs
- crates/cli/tests/tool_handlers.rs
- crates/cli/tests/provenance.rs
- contracts/
- docs/overview.md
- README.md

## Normative excerpt (in-repo)
- `docs/phases.md` sets TypeScript as the secondary language target and calls for quick L2 support before later L3 precision work.
- `specs/04-symbol-graph-heuristic-nav/design.md` explicitly says TypeScript is not yet wired into runtime symbol extraction.
- `contracts/tools/v1/README.md` and `contracts/errors.md` still describe `document_symbols` and `search_structural` as Rust/PHP-only surfaces.

## Architecture decisions
1. Treat `.ts` and `.tsx` as one public language family, `typescript`. Public filters accept `typescript`, `ts`, and `tsx`; note metadata may include extension or parser variant when useful, but the contract-level language name stays `typescript`.
2. Parser selection is extension-aware:
   - `.ts` uses `tree_sitter_typescript::LANGUAGE_TYPESCRIPT`
   - `.tsx` uses `tree_sitter_typescript::LANGUAGE_TSX`
3. Extend `SymbolKind` only where the current vocabulary cannot represent ubiquitous TypeScript declarations. If mutable bindings need distinct output semantics, add a new `variable` kind rather than misclassifying `let` or `var` as `const`.
4. Stable symbol ID generation keeps the existing shape: hash logical language, kind, canonical path, extracted name, and span. TSX files remain distinct through path and span even though the logical language family is shared with `.ts`.
5. Prefer explicit named declarations over synthetic names. Identifier-bound variable declarations are in scope; destructuring bindings and fully dynamic computed names stay out of scope unless they can be named deterministically without inventing helper syntax.
6. Private `#field` members may appear in `document_symbols` and `search_symbol`, but high-confidence reference/navigation parity for non-identifier tokens is expected to come from precise SCIP support in spec 49.

## Runtime integration plan

### Symbol corpus eligibility
- Extend `SymbolLanguage::from_path()` so `.ts` and `.tsx` enter:
  - direct single-file extraction helpers
  - manifest-backed source-path collection
  - warm-corpus rebuilds and path-local symbol indexes
- Keep repository-relative path normalization unchanged.

### Parser and symbol extraction
- Add TypeScript/TSX parser dispatch inside the existing extraction path.
- Implement TypeScript AST mapping for the declaration set promised by the requirements:
  - `module` / `internal_module` -> `module`
  - `class_declaration` / `abstract_class_declaration` -> `class`
  - `interface_declaration` -> `interface`
  - `enum_declaration` -> `enum`
  - `type_alias_declaration` -> `type_alias`
  - `function_declaration` / `generator_function_declaration` -> `function`
  - `method_definition` -> `method`
  - `public_field_definition` -> `property`
  - identifier-bound `lexical_declaration` / `variable_declaration` -> `const` or `variable`
- Reuse existing name extraction helpers wherever possible; add targeted helpers only for property identifiers, private identifiers, string member names, and identifier-bound variable declarators.

### Navigation and structural search
- `document_symbols` should accept `.ts` and `.tsx` as supported extensions and keep the current flat deterministic outline shape.
- `search_structural` should accept the logical language `typescript` plus `ts` and `tsx` aliases, then choose the concrete parser per file extension while keeping result notes stable.
- `search_symbol` and location-based navigation should benefit automatically once TypeScript/TSX symbols enter the corpus and path-local indexes.
- Heuristic references should reuse the existing token and containing-symbol workflow for identifier-like symbols; precise SCIP support in spec 49 closes the gap for symbol names that are not lexical identifiers.

### Language filter normalization
- Extend runtime language normalization to accept `typescript`, `ts`, and `tsx` in every current public `language` filter path that narrows runtime results.
- Do not create a separate public `tsx` language family. The goal is one logical TypeScript surface with extension-aware parsing underneath.

## Validation and rollout
- Add deterministic indexer unit tests for TS and TSX extraction with inline fixtures.
- Add integration coverage for `search_symbol`, heuristic `find_references`, location-based `go_to_definition`, `document_symbols`, and `search_structural`.
- Update public docs/contracts in the same change set, but do not make precise SCIP or semantic-runtime claims that belong to specs 49 and 50.
