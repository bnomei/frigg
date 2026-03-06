# Tasks — 10-mcp-surface-hardening

Meta:
- Spec: 10-mcp-surface-hardening — MCP Surface Hardening
- Depends on: 07-mcp-server-and-tool-contracts, 09-security-benchmarks-ops
- Global scope:
  - crates/cli/
  - crates/mcp/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Enforce HTTP auth/origin boundaries and constant-time bearer verification (owner: worker:019cba95-92ce-7241-ab77-ab461157c2ed) (scope: crates/cli/src/main.rs) (depends: -)
  - Started_at: 2026-03-04T20:41:46Z
  - Completed_at: 2026-03-04T20:53:50Z
  - Completion note: HTTP mode now requires auth token; host/origin allowlist middleware added; constant-time bearer comparison implemented.
  - Validation result: `cargo test -p frigg transport_` passed (10 tests).
- [x] T002: Harden `read_file` path resolution and bounded IO behavior (owner: worker:019cba95-92ce-7241-ab77-ab461157c2ed) (scope: crates/mcp/src/mcp/server.rs, crates/mcp/tests/security.rs, crates/mcp/tests/tool_handlers.rs) (depends: -)
  - Started_at: 2026-03-04T20:41:46Z
  - Completed_at: 2026-03-04T20:53:50Z
  - Completion note: Multi-root absolute path resolution fixed; outside-root denial made uniform; effective max-bytes clamp + pre-read size checks added.
  - Validation result: `cargo test -p mcp security` and `cargo test -p mcp --test tool_handlers` passed.
- [x] T003: Wire regex search path and safe `path_regex` validation in MCP layer (owner: worker:019cba95-92ce-7241-ab77-ab461157c2ed) (scope: crates/mcp/src/mcp/server.rs, crates/mcp/tests/tool_handlers.rs) (depends: -)
  - Started_at: 2026-03-04T20:41:46Z
  - Completed_at: 2026-03-04T20:53:50Z
  - Completion note: `pattern_type=regex` now executes regex search path; `path_regex` now enforces safe regex budget validation.
  - Validation result: `cargo test -p mcp --test tool_handlers` and `cargo test -p searcher regex_search` passed.
- [x] T004: Unify typed error envelope mapping and provenance error-code capture (owner: worker:019cba95-92ce-7241-ab77-ab461157c2ed) (scope: crates/mcp/src/mcp/server.rs, crates/mcp/tests/tool_handlers.rs) (depends: T001, T002, T003)
  - Started_at: 2026-03-04T20:41:46Z
  - Completed_at: 2026-03-04T20:53:50Z
  - Completion note: Centralized typed error builders ensure canonical `error_code` + `retryable`; provenance now records explicit codes and uses `missing_error_code` sentinel instead of implicit `internal` inference.
  - Validation result: `cargo test -p mcp --test tool_handlers` passed.
