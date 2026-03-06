# Hybrid Search Context Retrieval

<!-- frigg-playbook
{
  "schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "hybrid-search-context-retrieval",
  "query": "search_hybrid semantic_status semantic_reason strict_failure unavailable note metadata docs contract",
  "top_k": 16,
  "allowed_semantic_statuses": ["ok", "disabled", "degraded"],
  "required_witness_groups": [
    {
      "name": "runtime",
      "paths": [
        "crates/cli/src/searcher/mod.rs",
        "crates/cli/src/mcp/server.rs"
      ]
    }
  ],
  "target_witness_groups": [
    {
      "name": "docs",
      "paths": [
        "contracts/errors.md",
        "contracts/tools/v1/README.md"
      ]
    },
    {
      "name": "tests",
      "paths": [
        "crates/cli/tests/tool_handlers.rs",
        "crates/cli/src/searcher/mod.rs"
      ]
    }
  ]
}
-->

## Search Goal

Answer a broad question about Frigg's semantic retrieval behavior: how does `search_hybrid` communicate normal operation, degradation, or strict semantic failure?

## Why This Playbook Exists

This scenario tests whether Frigg can surface:

- the relevant implementation
- the contract-level promise
- the typed failure semantics
- the metadata a caller would need to explain why retrieval degraded

## Scope And Assumptions

- Repository scope: current `frigg` workspace
- Expected surface: `core`
- Semantic runtime may be disabled in local runs; that is an acceptable and useful outcome

## Expected Tool Flow

| Step | Tool | Search / intent | Expected return cues |
| --- | --- | --- | --- |
| p01 | `search_hybrid` | query `semantic runtime strict failure note metadata` with semantic enabled if possible | either ranked matches with semantic note metadata or a deterministic semantic-disabled/degraded note |
| p02 | `search_text` | regex search for `semantic_status|semantic_reason|strict_failure` | hits in contract docs, tests, or implementation |
| p03 | `read_file` | inspect `contracts/errors.md` | `search_hybrid` maps strict semantic failure to typed `unavailable` |
| p04 | `search_symbol` | search for `search_hybrid` | implementation match in `crates/cli/src/mcp/server.rs` |
| p05 | `read_file` | inspect `contracts/tools/v1/README.md` or the implementation file | response metadata and deterministic behavior are described in one place |

## Expected Return Cues

- Successful `search_hybrid` responses should include a `note` field with semantic metadata.
- In the current local repo state, a realistic degraded run is:
- `semantic_enabled: false`
- `semantic_status: "disabled"`
- `semantic_reason: "semantic runtime disabled in active configuration"`
- The error contract should state that strict semantic failure maps to canonical `unavailable`.
- The overall story should be traceable across docs, runtime code, and tests.

## Recording Ledger

| Step | Expected status | Recorded params_json | Recorded response summary | Notes |
| --- | --- | --- | --- | --- |
| p01 | `ok` or typed semantic note |  |  |  |
| p02 | `ok` |  |  |  |
| p03 | `ok` |  |  |  |
| p04 | `ok` |  |  |  |
| p05 | `ok` |  |  |  |
