# Error Contract Alignment

<!-- frigg-playbook
{
  "schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "error-contract-alignment",
  "query": "invalid_params -32602 public error taxonomy docs contract runtime helper tests",
  "top_k": 16,
  "allowed_semantic_statuses": ["ok", "disabled", "degraded"],
  "required_witness_groups": [
    {
      "name": "runtime",
      "paths": [
        "crates/cli/src/mcp/server.rs",
        "crates/cli/src/mcp/deep_search.rs"
      ]
    },
    {
      "name": "tests",
      "paths": [
        "crates/cli/tests/tool_handlers.rs",
        "crates/cli/tests/deep_search_replay.rs"
      ]
    }
  ],
  "target_witness_groups": [
    {
      "name": "docs",
      "paths": ["contracts/errors.md"]
    },
    {
      "name": "tool-contracts",
      "paths": ["contracts/tools/v1/README.md"]
    }
  ]
}
-->

## Search Goal

Trace a typed error from public docs to runtime helpers to test coverage, using `invalid_params` as the primary case.

## Why This Playbook Exists

Search quality is not only about successful answers. Frigg also promises deterministic typed failures, and those need to be discoverable and auditable across:

- public error taxonomy
- tool-specific contracts
- runtime helper functions
- integration tests

## Scope And Assumptions

- Repository scope: current `frigg` workspace
- Expected surface: `core`
- Focus error class: `invalid_params`

## Expected Tool Flow

| Step | Tool | Search / intent | Expected return cues |
| --- | --- | --- | --- |
| p01 | `read_file` | inspect `contracts/errors.md` | canonical `invalid_params` definition and tool-specific mappings |
| p02 | `search_text` | regex search for `invalid_params|document_symbols|search_structural` | hits in docs, runtime, and tests |
| p03 | `search_symbol` | search `invalid_params` | runtime helper match in `crates/cli/src/mcp/server.rs` |
| p04 | `find_references` | trace `invalid_params` helper usage | multiple call sites in tool handlers |
| p05 | `read_file` | inspect `crates/cli/tests/tool_handlers.rs` around typed-failure tests | test names and assertions reinforce the public contract |

## Expected Return Cues

- The taxonomy doc should map `invalid_params` to JSON-RPC `-32602`.
- Runtime code should expose a shared helper for constructing typed invalid-params failures.
- Tests should include names like unsupported extension, invalid query, or abusive regex rejection.
- A good run should let a reviewer answer both "what fails" and "how Frigg encodes the failure."

## Recording Ledger

| Step | Expected status | Recorded params_json | Recorded response summary | Notes |
| --- | --- | --- | --- | --- |
| p01 | `ok` |  |  |  |
| p02 | `ok` |  |  |  |
| p03 | `ok` |  |  |  |
| p04 | `ok` |  |  |  |
| p05 | `ok` |  |  |  |
