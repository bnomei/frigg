# Design — 40-symbol-resolution-indexes

## Scope
- `crates/cli/src/mcp/`
- `crates/cli/src/indexer/`
- `contracts/`
- `benchmarks/`
- `specs/40-symbol-resolution-indexes/`
- `specs/index.md`

## Problem statement
The current warm-corpus symbol-resolution paths still do linear work over all extracted symbols:

- `search_symbol` scans every symbol in every scoped corpus, ranks matches, sorts the entire matched set, and truncates at the end;
- symbol-based navigation target resolution scans every symbol to find stable-id or exact-name candidates;
- location-based resolution scans every symbol even though the corpus already stores a path index.

This leaves a scale-sensitive hot path in the read-only MCP surface even after the broader performance pass in spec 39.

## Approach
Add deterministic symbol-corpus lookup indexes and move the most common resolution paths onto them:

1. Extend `RepositorySymbolCorpus` with lookup structures for:
   - stable id to symbol index;
   - exact symbol name to symbol indexes;
   - ASCII-folded symbol name to symbol indexes;
   - repository-relative path to symbol indexes sorted by line/column for nearest-preceding lookup.
2. Rewrite symbol-based target resolution to probe indexes in precedence order:
   - stable-id exact;
   - exact name;
   - case-insensitive exact name.
3. Rewrite location-based target resolution to fetch only symbols for the requested path and then choose the nearest preceding symbol deterministically.
4. Optimize `search_symbol` in rank order:
   - use exact and prefix-capable indexes to build rank `0-2` candidates without corpus-wide scans;
   - preserve a bounded deterministic infix fallback for rank `3` candidates only when the higher-ranked buckets do not already satisfy the requested limit.

## Architecture changes

### Corpus index layout
- Store indexes as symbol-position references, not cloned `SymbolDefinition` values.
- Keep the canonical backing `symbols: Vec<SymbolDefinition>` as the single source of truth.
- Normalize names using the same ASCII case-folding semantics already used by `symbol_name_match_rank()` and `navigation_symbol_target_rank()`.

### `search_symbol` execution
- Continue to apply the existing rank and tie-break contract.
- Build ranked output from indexed candidate buckets in deterministic order.
- Only perform an infix scan when required to satisfy rank `3` semantics.

### Navigation target execution
- For `symbol` input, route candidate discovery through indexed lookup instead of full scans.
- For `path` plus `line` input, reuse `symbols_by_relative_path` and keep the path-local lists sorted for binary-search or nearest-preceding selection.
- Preserve existing error contracts and `target_selection` note semantics.

## Risks
- Additional indexes increase symbol-corpus resident memory.
- Prefix and infix handling can drift from current ordering if candidate dedupe is not explicit.
- Path-local nearest-preceding selection must preserve current stable tie-breaks when multiple symbols share the same line.

## Validation strategy
- Regression tests proving indexed search and navigation return the same ordered results as the current implementation for exact, case-insensitive, prefix, and infix queries.
- Regression tests for location-based resolution on files with multiple candidate symbols on the same line and across nested scopes.
- Benchmark or timing coverage demonstrating reduced work on warm-corpus symbol search and navigation resolution.
