# Tasks — 36-deep-search-runtime-tools

Meta:
- Spec: 36-deep-search-runtime-tools — Deep Search Runtime Tools
- Depends on: 08-hybrid-retrieval-and-deep-search-harness, 10-mcp-surface-hardening
- Global scope:
  - crates/cli/
  - crates/mcp/
  - crates/mcp/tests/
  - contracts/
  - contracts/tools/v1/
  - benchmarks/
  - docs/overview.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

## Done

- [x] T001: Define deep-search runtime tool contracts and schema wrappers (owner: worker:019cbdf0-dcd7-7df2-92a7-261c732ff5e7) (scope: contracts/tools/v1/, crates/mcp/src/mcp/types.rs, contracts/changelog.md) (depends: -)
  - Started_at: 2026-03-05T12:20:14Z
  - Completed_at: 2026-03-05T12:25:37Z
  - Completion note: Added `deep_search_run`, `deep_search_replay`, and `deep_search_compose_citations` v1 schema docs and typed wrappers/conversion bridges in `crates/mcp/src/mcp/types.rs`; synchronized tool contract docs and changelog.
  - Validation result: `cargo test -p mcp schema_ -- --nocapture` passed (18 tests).
- [x] T002: Add runtime feature-gated handler registration for deep-search tools (owner: worker:019cbdf0-dcd7-7df2-92a7-261c732ff5e7) (scope: crates/cli/, crates/mcp/src/mcp/server.rs) (depends: T001)
  - Started_at: 2026-03-05T12:28:38Z
  - Completed_at: 2026-03-05T12:33:53Z
  - Completion note: Added deterministic server-side runtime gate for deep-search tool registration (default hidden, explicit enable path via `FriggMcpServer::new_with_runtime_options`), registered deep-search tool handlers, and added runtime gate tests; CLI wiring intentionally deferred to avoid cross-scope collision.
  - Validation result: `cargo test -p mcp runtime_gate_tests -- --nocapture` and `cargo test -p mcp --test tool_handlers -- --nocapture` passed.
- [x] T003: Wire handlers to `DeepSearchHarness` and enforce step allowlist (owner: worker:019cbdf0-dcd7-7df2-92a7-261c732ff5e7) (scope: crates/mcp/src/mcp/server.rs, crates/mcp/src/mcp/deep_search.rs, crates/mcp/tests/) (depends: T002)
  - Started_at: 2026-03-05T12:34:31Z
  - Completed_at: 2026-03-05T12:39:46Z
  - Completion note: Replaced deep-search runtime handler stubs with direct harness calls, added deterministic step-tool allowlist enforcement with typed `invalid_params` failures, and expanded runtime integration tests for run/replay/citation compose determinism and invalid-step rejection.
  - Validation result: `cargo test -p mcp deep_search_replay -- --nocapture` and `cargo test -p mcp citation_payloads -- --nocapture` passed.
- [x] T004: Add provenance and budget propagation coverage for deep-search runtime tools (owner: worker:019cbdf0-dcd7-7df2-92a7-261c732ff5e7) (scope: crates/mcp/, crates/mcp/tests/, contracts/errors.md) (depends: T003)
  - Started_at: 2026-03-05T12:40:59Z
  - Completed_at: 2026-03-05T12:47:15Z
  - Completion note: Added deep-search runtime provenance recording with deterministic resource budget/resource usage propagation, extended provenance/security coverage for success and typed `invalid_params` deep-search failures, and documented deep-search runtime error taxonomy mapping updates.
  - Validation result: `cargo test -p mcp provenance -- --nocapture` and `cargo test -p mcp --test security -- --nocapture` passed.
- [x] T005: Add benchmark/doc updates and public-tool-surface sync (owner: worker:019cbdf0-dcd7-7df2-92a7-261c732ff5e7) (scope: benchmarks/, crates/mcp/benches/, contracts/tools/v1/README.md, docs/overview.md) (depends: T004)
  - Started_at: 2026-03-05T12:47:50Z
  - Completed_at: 2026-03-05T12:58:01Z
  - Completion note: Added deep-search runtime benchmark workloads to MCP latency bench suite, synchronized budgets/report/docs/tool-surface narrative for deep-search runtime tools, regenerated benchmark report (`pass=33 fail=0 missing=0`), and confirmed docs-sync/release gate compatibility.
  - Validation result: `cargo bench -p mcp`, `python3 benchmarks/generate_latency_report.py`, `just docs-sync`, and `just release-ready` passed.
