# HTTP Auth Entrypoint Trace

<!-- frigg-playbook
{
  "schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "http-auth-entrypoint-trace",
  "query": "where is the optional HTTP MCP auth token declared enforced and documented",
  "top_k": 8,
  "allowed_semantic_statuses": ["ok", "disabled", "degraded"],
  "required_witness_groups": [
    {
      "name": "runtime",
      "paths": ["crates/cli/src/main.rs"],
      "required_when": "semantic_ok"
    },
    {
      "name": "docs",
      "paths": ["README.md"],
      "required_when": "semantic_ok"
    }
  ],
  "target_witness_groups": [
    {
      "name": "runtime-guardrails",
      "paths": ["crates/cli/src/main.rs"]
    },
    {
      "name": "docs",
      "paths": ["README.md"]
    }
  ]
}
-->

## Search Goal

Find where Frigg's optional HTTP MCP auth token is declared, enforced, and documented.

## Why This Playbook Exists

This is a practical onboarding and incident-response question. A developer should be able to answer:

- where the token enters the CLI/runtime configuration
- where authorization is checked in the HTTP path
- which docs describe the expected client behavior

## Scope And Assumptions

- Repository scope: current `frigg` workspace
- Expected surface: `core`
- This playbook should succeed without deep-search tools

## Expected Tool Flow

| Step | Tool | Search / intent | Expected return cues |
| --- | --- | --- | --- |
| p01 | `search_text` | regex search for `mcp_http_auth_token|FRIGG_MCP_HTTP_AUTH_TOKEN|AUTHORIZATION|UNAUTHORIZED` | matches in `crates/cli/src/main.rs` and `README.md` |
| p02 | `document_symbols` | inspect `crates/cli/src/main.rs` | outline includes `HttpRuntimeConfig`, `resolve_http_runtime_config`, `serve_http`, and `bearer_auth_middleware` |
| p03 | `read_file` | inspect `crates/cli/src/main.rs` | file includes CLI args, env var wiring, runtime guardrails, and HTTP middleware/auth handling |
| p04 | `search_text` | regex search for `POST /mcp|--mcp-http-auth-token` in docs | matches in `README.md` |
| p05 | `search_symbol` | search for `FriggMcpServer` or nearby HTTP entrypoint symbols | public server symbol match in MCP runtime code |

## Expected Return Cues

- `search_text` should return canonical repository-relative paths.
- `document_symbols` should expose the HTTP/auth helpers before a human opens the file.
- `read_file` on `crates/cli/src/main.rs` should show `mcp_http_auth_token: Option<String>`, `env = "FRIGG_MCP_HTTP_AUTH_TOKEN"`, and helpers such as `resolve_http_runtime_config`, `serve_http`, and `bearer_auth_middleware`.
- README hits should mention `POST /mcp` and the auth-token flag or env var.
- A strong run should also surface the remote-bind guardrails tested near `transport_rejects_remote_bind_without_auth_token` and `transport_accepts_remote_bind_with_override_and_auth_token`.

## Recording Ledger

| Step | Expected status | Recorded params_json | Recorded response summary | Notes |
| --- | --- | --- | --- | --- |
| p01 | `ok` |  |  |  |
| p02 | `ok` |  |  |  |
| p03 | `ok` |  |  |  |
| p04 | `ok` |  |  |  |
| p05 | `ok` |  |  |  |
