# Program handoff

Last updated: 2026-03-05T14:00:59Z

## Current focus
- Planning wave added for vision-to-fact closure:
  - `35-semantic-runtime-mcp-surface`
  - `36-deep-search-runtime-tools`
  - `37-public-surface-parity-gates`

## Reservations (in progress scopes)
- (none)

## In progress tasks
- (none)

## Blockers
- (none)

## Next ready tasks
- (none)

## Notes
- `35/T006` completed by `worker:019cbdf0-d773-7c50-bf56-c76dd031e8f6` and mayor-validated: semantic runtime benchmark workloads added/synced across `search_latency` and `mcp_tool_latency` with budget/docs/report updates; release gate summary is now `benchmark_summary pass=37 fail=0 missing=0`.
- Program closeout: all tasks in specs `35`, `36`, and `37` are now marked done with required validations green.
- `37/T004` completed and mayor-validated: runtime/schema/docs parity reconciled across core and extended profiles, with parity and release gates green (`tool-surface-parity status=pass`, `benchmark_summary pass=33 fail=0 missing=0`).
- `35/T005` completed and mayor-validated: semantic replay determinism coverage added across `searcher` semantic enabled/degraded/strict paths, plus deep-search partial-channel replay and citation error-step deterministic coverage; validations passed (`cargo test -p mcp deep_search_replay`, `cargo test -p mcp citation_payloads`, `cargo test -p mcp --test tool_handlers`, `cargo test -p searcher hybrid_ranking_semantic_`).
- Reclaimed `37/T004` due worker unresponsiveness and completed mayor-side to preserve momentum.
- 30/T001 completed and validated: citation hygiene gate added and wired into docs-sync + release-ready.
- 31/T001-T002 completed and validated: write-surface policy contracts + release gate enforcement + MCP regression guards added.
- 32/T001-T002 completed and validated: sqlite-vec runtime pin/readiness hardening and strict startup/smoke gating added.
- 33/T001-T002 completed and validated: regex trigram/bitmap prefilter shipped and benchmark/docs/budget/report sync completed.
- Latest benchmark report summary: `pass=30 fail=0 missing=0` (post-`T007` spec-34 benchmark wave).
- `T001` and `T002` are completed and mayor-validated.
- `T003-T006` are completed and mayor-validated (`cargo test -p mcp`, `cargo test -p mcp --test tool_handlers`, `cargo test -p indexer structural_search_ -- --nocapture`, `cargo test -p graph precise_navigation_symbol_selection_is_deterministic -- --nocapture`).
- `T007` completed and mayor-validated (`cargo bench -p mcp`; `python3 benchmarks/generate_latency_report.py` => `summary pass=30 fail=0 missing=0`).
- `T008` completed and mayor-validated (`just docs-sync`; `just release-ready` => `benchmark_summary pass=30 fail=0 missing=0`).
