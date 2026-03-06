# Tasks — 40-symbol-resolution-indexes

Meta:
- Spec: 40-symbol-resolution-indexes — Deterministic Indexed Symbol Resolution
- Depends on: 34-readonly-ide-navigation-tools, 39-performance-memory-hardening
- Global scope:
  - crates/cli/src/mcp/
  - crates/cli/src/indexer/
  - contracts/
  - benchmarks/
  - specs/40-symbol-resolution-indexes/
  - specs/index.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add deterministic symbol-corpus lookup indexes and switch navigation target resolution to indexed lookup (owner: mayor) (scope: crates/cli/src/mcp/, crates/cli/src/indexer/, specs/40-symbol-resolution-indexes/) (depends: -)
  - Started_at: 2026-03-06T12:30:00Z
  - Completed_at: 2026-03-06T14:05:00Z
  - Context: The remaining audited navigation hot paths are warm-corpus symbol scans. The new implementation must preserve existing precedence rules for stable-id exact, name exact, case-insensitive exact, and location-based nearest-preceding selection.
  - Reuse_targets: `RepositorySymbolCorpus`, `navigation_symbol_target_rank`, `symbols_by_relative_path`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: `RepositorySymbolCorpus` stores the required lookup indexes; `resolve_navigation_symbol_target()` and location-based resolution stop scanning the full corpus for indexed query shapes; deterministic ordering and typed errors remain unchanged.
  - Validation: targeted `cargo test -p frigg --test tool_handlers` plus location-resolution regression coverage.
  - Escalate if: preserving exact legacy ordering requires a public contract change or an unavoidable response-note change.

- [x] T002: Optimize `search_symbol` with indexed candidate narrowing and preserve infix fallback semantics (owner: mayor) (scope: crates/cli/src/mcp/, benchmarks/, contracts/, specs/40-symbol-resolution-indexes/, specs/index.md) (depends: T001)
  - Completed_at: 2026-03-06T14:05:00Z
  - Context: `search_symbol` currently scans every symbol, then sorts the matched set. The new path may narrow exact and prefix buckets through indexes, but it must still preserve rank `0-3` semantics and deterministic tie-breaks.
  - Reuse_targets: `symbol_name_match_rank`, existing `SearchSymbolResponse` note shape, spec 39 benchmark conventions
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: `search_symbol` uses indexed candidate narrowing for exact/prefix-capable ranks, falls back to deterministic infix handling only when needed, and includes updated benchmark/contracts notes if any operator-visible guidance changes.
  - Validation: targeted `cargo test -p frigg --test tool_handlers` coverage for exact/case/prefix/infix ordering.
  - Escalate if: indexed prefix handling cannot preserve current result order without materially higher memory overhead than the existing corpus representation.
