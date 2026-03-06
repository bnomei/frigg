# Design — 34-readonly-ide-navigation-tools

## Scope
- crates/mcp/src/mcp/server.rs
- crates/mcp/src/mcp/types.rs
- crates/mcp/tests/
- crates/mcp/benches/tool_latency.rs
- crates/graph/src/lib.rs
- crates/index/src/lib.rs
- contracts/tools/v1/
- contracts/errors.md
- contracts/changelog.md
- benchmarks/mcp-tools.md
- benchmarks/budgets.v1.json
- benchmarks/latest-report.md
- docs/overview.md

## Normative excerpt (in-repo)
- `docs/overview.md`: Frigg server stays deterministic; client-side agents perform planning loops.
- `contracts/tools/v1/README.md`: MCP tool identity is contract-bound and currently read-only public surface; new tools must be schema-versioned.
- `contracts/errors.md`: new tools must map failures to canonical deterministic error codes.

## Proposed read-only MCP additions
1. `go_to_definition`
2. `find_declarations`
3. `find_implementations`
4. `incoming_calls`
5. `outgoing_calls`
6. `document_symbols`
7. `search_structural` (tree-sitter query mode for Rust/PHP in v1)

Item (4) in the “should add next” set is implemented as two explicit tools for symmetry and simpler client routing.

## Contract shape (high level)
- All new tools are read-only/idempotent MCP tools.
- All tool responses include deterministic path/location records using repository-relative canonical paths.
- `note` payloads include precision/fallback metadata where relevant.
- Error mapping uses existing taxonomy: `invalid_params`, `resource_not_found`, `timeout`, `index_not_ready`, `internal`.

## Data flow

### 1) `go_to_definition`
- Resolve target symbol by either:
  - explicit symbol query, or
  - source location (`path`,`line`,`column`) to enclosing/overlapping symbol.
- Attempt precise definition lookup from SCIP occurrences (`is_definition`).
- Fall back to deterministic symbol corpus + graph heuristics when precise data is absent.
- Return sorted definition locations with precision metadata.

### 2) `find_declarations`
- Reuse symbol-resolution path used by `go_to_definition`.
- Treat declaration anchors as definition anchors in v1 for Rust/PHP (no separate declaration AST class exposed yet).
- Preserve deterministic sorting and precision metadata.

### 3) `find_implementations`
- Prefer precise relationships (`implementation`, `type_definition`) from ingested SCIP relationships.
- Fall back to graph relations (`implements`, `extends`) from heuristic graph.
- Return symbol + location payloads with precision and fallback_reason metadata.

### 4) `incoming_calls` / 5) `outgoing_calls`
- Resolve target symbol deterministically.
- Use symbol graph adjacency filtered to `calls` relation first.
- Include relation kind, caller/callee symbol data, and source location anchors where available.
- If precise call data is unavailable, annotate heuristic mode explicitly.

### 6) `document_symbols`
- Resolve file path through current repository/path contract.
- Use existing Rust/PHP symbol extraction pipeline on that file.
- Return stable sorted symbol outline with spans and kinds.
- For unsupported file extensions return typed `invalid_params` (or empty deterministic result policy if chosen in implementation decision log).

### 7) `search_structural`
- v1 mode: tree-sitter query execution for Rust/PHP only.
- Inputs: `query`, optional `language`, optional `repository_id`, optional `path_regex`, optional `limit`.
- Safety: enforce query length and runtime limits; compile/validation failures are typed `invalid_params`.
- Results: deterministic sorted matches with path + range + excerpt.

## Reuse-first plan
- Reuse `search_symbol` / `find_references` symbol target-selection and provenance patterns in `crates/mcp/src/mcp/server.rs`.
- Reuse `SymbolGraph` adjacency/relationship helpers:
  - `outgoing_adjacency`, `incoming_adjacency`
  - `precise_occurrences_for_symbol`, `precise_relationships_from_symbol`
- Reuse index extraction primitives:
  - `extract_symbols_from_source` and existing Rust/PHP parser setup.
- Reuse existing safe regex/path filter and budget guard patterns for new query-style tools.

## Determinism and ordering
- Define per-tool stable sort keys (repository_id, path, line, column, symbol, kind).
- Preserve canonical path normalization before output.
- Ensure repeated calls produce byte-equivalent JSON ordering for identical inputs.

## Benchmarks and acceptance
- Add MCP tool latency workloads for all new tools in `crates/mcp/benches/tool_latency.rs`.
- Add/update budgets in `benchmarks/budgets.v1.json`.
- Update benchmark docs/report sync artifacts.

## Risks
- `search_structural` complexity and query safety may exceed v1 bounds if unrestricted.
- Implementation/declaration semantics vary by language; v1 must document exact behavior clearly.
- Precise-vs-heuristic blending can drift if metadata contract is underspecified.

## Open decisions (to close during implementation)
- `go_to_definition` input contract: require symbol, location, or allow both (with precedence rules).
- `document_symbols` response shape: flat list only vs optional hierarchical tree in v1.
- `search_structural` query dialect: strict tree-sitter query only vs optional future Comby mode.
