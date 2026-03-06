# Design — 10-mcp-surface-hardening

## Normative excerpt
- Public errors must include canonical `error_code` and machine-readable detail payload.
- Security coverage must include path traversal/workspace boundary and transport boundary checks.

## Architecture
- `crates/cli/src/main.rs`
  - tighten HTTP runtime config: mandatory auth token for HTTP mode.
  - add transport request middleware for origin/host allowlist.
  - apply constant-time bearer token comparison.
- `crates/mcp/src/mcp/server.rs`
  - centralize typed error builders (`invalid_params`, `resource_not_found`, `access_denied`, `internal`).
  - harden `resolve_file_path` to avoid early failure in multi-root absolute-path lookups and remove existence oracle differences.
  - harden `read_file` with effective max clamp and pre-read size checks.
  - wire regex mode to `TextSearcher::search_regex_with_filters`.
  - validate `path_regex` via shared safe-regex validator.
  - remove provenance fallback error-code inference by always emitting canonical data.
- Tests
  - expand `crates/cli/src/main.rs` transport tests.
  - expand `crates/mcp/tests/security.rs` and `crates/mcp/tests/tool_handlers.rs` for new behaviors.

## Data flow changes
1. CLI resolves HTTP runtime; missing token in HTTP mode is a startup error.
2. HTTP middleware validates bearer token and request origin/host.
3. `read_file` computes bounded effective limit and rejects oversized targets before loading full content.
4. `search_text` dispatches literal/regex path explicitly; both use typed error mapping.
5. Provenance receives canonical error codes directly from error builders.

## Acceptance signals
- HTTP mode without token fails startup.
- Regex mode succeeds for valid query and returns typed invalid-params for abusive/invalid regex.
- `read_file` returns uniform access-denied behavior for outside-root targets and supports absolute path under later roots.
- Tool failures include `data.error_code` + `data.retryable` deterministically.
