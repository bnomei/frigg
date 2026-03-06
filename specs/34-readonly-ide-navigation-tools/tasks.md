# Tasks — 34-readonly-ide-navigation-tools

Meta:
- Spec: 34-readonly-ide-navigation-tools — Read-only IDE Navigation Tools
- Depends on: 04-symbol-graph-heuristic-nav, 05-scip-precision-ingest, 07-mcp-server-and-tool-contracts, 22-tool-path-semantics-unification
- Global scope:
  - crates/mcp/
  - crates/graph/
  - crates/index/
  - contracts/
  - benchmarks/
  - docs/overview.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add MCP schema/type contracts for new read-only navigation tools (owner: worker:019cbd51-b3ea-70a2-80ca-f2aeb149dfe1) (scope: contracts/tools/v1/, crates/mcp/src/mcp/types.rs, contracts/changelog.md) (depends: -)
  - Started_at: 2026-03-05T10:11:47Z
  - Completed_at: 2026-03-05T10:55:54Z
  - Completion note: Added v1 schema docs plus typed request/response contracts for all seven new read-only IDE navigation tools and expanded public read-only registry/parity coverage.
  - Validation result: Mayor verified `cargo test -p mcp schema_ -- --nocapture` and full `cargo test -p mcp` passed.

- [x] T002: Implement shared symbol-target resolution helpers for navigation tools (owner: worker:019cbd51-b3ea-70a2-80ca-f2aeb149dfe1) (scope: crates/mcp/src/mcp/server.rs, crates/index/src/lib.rs, crates/graph/src/lib.rs) (depends: T001)
  - Started_at: 2026-03-05T10:16:48Z
  - Completed_at: 2026-03-05T10:23:42Z
  - Completion note: Added shared symbol target resolver utilities in `mcp`/`index`/`graph` and refactored `find_references` to consume them with deterministic precedence ordering and no observed tool-handler behavior drift.
  - Validation result: Mayor verified `cargo test -p mcp --test tool_handlers`, `cargo test -p indexer navigation_symbol_target_rank_is_stable_and_precedence_ordered`, and `cargo test -p graph precise_navigation_symbol_selection_is_deterministic` passed.

- [x] T003: Implement `go_to_definition` and `find_declarations` handlers with precision metadata (owner: worker:019cbd51-b3ea-70a2-80ca-f2aeb149dfe1) (scope: crates/mcp/src/mcp/server.rs, crates/mcp/tests/, crates/graph/src/lib.rs) (depends: T002)
  - Started_at: 2026-03-05T10:24:45Z
  - Completed_at: 2026-03-05T10:54:08Z
  - Completion note: Implemented both handlers with deterministic precise-first resolution, heuristic fallback notes, and tool-handler coverage for precise/fallback behavior.
  - Validation result: Mayor verified `cargo test -p mcp --test tool_handlers` and `cargo test -p mcp`.

- [x] T004: Implement `find_implementations`, `incoming_calls`, and `outgoing_calls` handlers (owner: worker:019cbd51-b3ea-70a2-80ca-f2aeb149dfe1) (scope: crates/mcp/src/mcp/server.rs, crates/mcp/tests/, crates/graph/src/lib.rs, crates/index/src/lib.rs) (depends: T002)
  - Started_at: 2026-03-05T10:24:45Z
  - Completed_at: 2026-03-05T10:54:08Z
  - Completion note: Implemented deterministic implementation and call-hierarchy handlers with relation metadata, precise-first resolution, and fallback behavior coverage.
  - Validation result: Mayor verified `cargo test -p mcp --test tool_handlers`, `cargo test -p graph precise_navigation_symbol_selection_is_deterministic -- --nocapture`, and `cargo test -p mcp`.

- [x] T005: Implement `document_symbols` file-outline tool (owner: worker:019cbd51-b3ea-70a2-80ca-f2aeb149dfe1) (scope: crates/mcp/src/mcp/server.rs, crates/mcp/tests/, crates/index/src/lib.rs) (depends: T001)
  - Started_at: 2026-03-05T10:38:52Z
  - Completed_at: 2026-03-05T10:54:08Z
  - Completion note: Implemented `document_symbols` with canonical path handling, supported-language enforcement, deterministic output ordering, and integration tests.
  - Validation result: Mayor verified `cargo test -p mcp --test tool_handlers` and `cargo test -p mcp`.

- [x] T006: Implement `search_structural` v1 (tree-sitter query mode, Rust/PHP) with safety limits (owner: worker:019cbd51-b3ea-70a2-80ca-f2aeb149dfe1) (scope: crates/mcp/src/mcp/server.rs, crates/mcp/tests/, crates/index/src/lib.rs, contracts/errors.md) (depends: T001)
  - Started_at: 2026-03-05T10:38:52Z
  - Completed_at: 2026-03-05T10:54:08Z
  - Completion note: Implemented bounded Rust/PHP structural search with deterministic capture ordering, typed invalid/unsupported errors, and contract-aligned handler + indexer support.
  - Validation result: Mayor verified `cargo test -p indexer structural_search_ -- --nocapture`, `cargo test -p mcp --test tool_handlers`, and `cargo test -p mcp`.

- [x] T007: Extend MCP benchmark workloads/budgets/docs for new tool surface (owner: worker:019cbd51-b9e0-7720-acb1-fe685de60bb0) (scope: crates/mcp/benches/tool_latency.rs, benchmarks/mcp-tools.md, benchmarks/budgets.v1.json, benchmarks/latest-report.md) (depends: T003, T004, T005, T006)
  - Started_at: 2026-03-05T10:55:54Z
  - Completed_at: 2026-03-05T11:06:18Z
  - Completion note: Extended `mcp_tool_latency` workloads for all seven new read-only IDE tools, added matching budgets, synced MCP benchmark methodology docs, and regenerated the latest benchmark report.
  - Validation result: Mayor verified `cargo bench -p mcp` and `python3 benchmarks/generate_latency_report.py` with `summary pass=30 fail=0 missing=0`.

- [x] T008: Final docs sync for tool surface and deterministic behavior guarantees (owner: worker:019cbd51-bd60-7433-8fe9-55acc03ebba7) (scope: docs/overview.md, contracts/tools/v1/README.md, contracts/changelog.md) (depends: T007)
  - Started_at: 2026-03-05T11:06:18Z
  - Completed_at: 2026-03-05T11:19:12Z
  - Completion note: Synced overview and contracts docs with the final 12-tool read-only runtime surface, added deterministic behavior/limit guarantees for the seven new IDE navigation tools, and aligned changelog + benchmark/readiness narrative.
  - Validation result: Mayor verified `just docs-sync` and `just release-ready` passed (`benchmark_summary pass=30 fail=0 missing=0`).
