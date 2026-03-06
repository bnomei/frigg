# Tool Surface Gating

<!-- frigg-playbook
{
  "schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "tool-surface-gating",
  "query": "which MCP tools are core versus extended and where is tool surface gating enforced in runtime docs and tests",
  "top_k": 12,
  "allowed_semantic_statuses": ["ok", "disabled", "degraded"],
  "required_witness_groups": [
    {
      "name": "runtime",
      "paths": [
        "crates/cli/src/mcp/tool_surface.rs",
        "crates/cli/src/mcp/server.rs"
      ],
      "required_when": "semantic_ok"
    },
    {
      "name": "tests",
      "paths": ["crates/cli/tests/tool_surface_parity.rs"],
      "required_when": "semantic_ok"
    }
  ],
  "target_witness_groups": [
    {
      "name": "docs",
      "paths": ["contracts/tools/v1/README.md"]
    }
  ]
}
-->

## Search Goal

Identify which MCP tools are part of the default `core` surface, which are `extended`, and where that gating is enforced.

## Why This Playbook Exists

This is the shortest route to answering product-surface questions:

- what is public by default
- what requires explicit runtime enablement
- whether code, docs, and tests agree

## Scope And Assumptions

- Repository scope: current `frigg` workspace
- Expected surface: `core` plus feature-gated deep-search references in docs/code

## Expected Tool Flow

| Step | Tool | Search / intent | Expected return cues |
| --- | --- | --- | --- |
| p01 | `read_file` | inspect `crates/cli/src/mcp/tool_surface.rs` | `ToolSurfaceProfile::Core` and `ToolSurfaceProfile::Extended` plus deep-search tool names |
| p02 | `search_symbol` | search `ToolSurfaceProfile` | enum and impl matches in `tool_surface.rs` |
| p03 | `find_references` | locate `ToolSurfaceProfile` consumers | hits in `crates/cli/src/mcp/server.rs` and `crates/cli/tests/tool_surface_parity.rs` |
| p04 | `search_text` | regex search for `DEEP_SEARCH_TOOL_NAMES|filtered_tool_router|runtime_tool_surface_parity` | runtime gating references in `server.rs` |
| p05 | `read_file` | inspect `contracts/tools/v1/README.md` | separate `core` and `extended_only` sections in the public contract |

## Expected Return Cues

- `tool_surface.rs` should show the extended-only set:
- `deep_search_run`
- `deep_search_replay`
- `deep_search_compose_citations`
- Contract docs should mirror the same split between `core` and `extended_only`.
- Reference search should connect implementation and parity-test coverage.

## Recording Ledger

| Step | Expected status | Recorded params_json | Recorded response summary | Notes |
| --- | --- | --- | --- | --- |
| p01 | `ok` |  |  |  |
| p02 | `ok` |  |  |  |
| p03 | `ok` |  |  |  |
| p04 | `ok` |  |  |  |
| p05 | `ok` |  |  |  |
