# Requirements — 10-mcp-surface-hardening

## Scope
Harden MCP HTTP transport and file/text tool input handling for security, typed errors, and deterministic behavior.

## EARS requirements
- When MCP HTTP transport is enabled, the Frigg server shall require a non-blank bearer token before serving `/mcp`.
- When an MCP HTTP request has an unauthorized `Origin` or `Host` value, the Frigg server shall reject the request with a typed access-denied response.
- When `read_file` is called, the Frigg server shall enforce an effective byte limit of `min(request.max_bytes, config.max_file_bytes)` before full payload materialization.
- If `read_file` resolves a path outside all allowed workspace roots, then the Frigg server shall return `access_denied` without exposing file-existence differences.
- When `read_file` resolves an absolute path and multiple workspace roots are configured, the Frigg server shall evaluate all roots before rejecting access.
- When `search_text` is requested with `pattern_type=regex`, the Frigg server shall execute safe bounded regex search instead of returning `index_not_ready`.
- When `search_text.path_regex` is provided, the Frigg server shall enforce regex budget limits equivalent to text-query safe regex validation.
- When any public MCP tool returns an error, the Frigg server shall include canonical `error_code` and `retryable` fields in `error.data`.
