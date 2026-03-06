# Design — 04-symbol-graph-heuristic-nav

## Normative excerpt (from `docs/overview.md`)
- Practical strategy: tree-sitter + ctags-style extraction now, precise SCIP override later.
- Language support levels: L1 symbol, L2 heuristic references, L3 precise references.

## Architecture
- `crates/index/` handles symbol extraction per language.
- `crates/graph/` stores symbol nodes and relationship edges (`DEFINED_IN`, `REFERS_TO`, etc.).
- `crates/mcp/` exposes `search_symbol` + heuristic `find_references` using graph/index APIs.

## Language coverage
- Rust: parser-backed symbol extraction baseline.
- PHP: parser-backed symbol extraction baseline.
- TypeScript: not currently wired in runtime symbol extraction; tracked as future onboarding.

## Heuristic reference strategy
- Candidate references from word-boundary and import-context search.
- Confidence scoring attached to heuristic output.
