# Deep Search Replay And Citations

<!-- frigg-playbook
{
  "schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "deep-search-replay-and-citations",
  "query": "how does Frigg turn a multi-step suite playbook fixture into a deterministic trace artifact replay and citations",
  "top_k": 16,
  "allowed_semantic_statuses": ["ok", "disabled", "degraded"],
  "required_witness_groups": [
    {
      "name": "runtime",
      "paths": ["crates/cli/src/mcp/deep_search.rs"],
      "required_when": "semantic_ok"
    },
    {
      "name": "tests",
      "paths": [
        "crates/cli/tests/playbook_suite.rs",
        "crates/cli/tests/deep_search_replay.rs",
        "crates/cli/tests/citation_payloads.rs"
      ],
      "required_when": "semantic_ok"
    },
    {
      "name": "fixtures",
      "paths": [
        "fixtures/playbooks/deep-search-suite-core.playbook.json",
        "fixtures/playbooks/deep-search-suite-core.expected.json"
      ],
      "required_when": "semantic_ok"
    }
  ],
  "target_witness_groups": [
    {
      "name": "docs",
      "paths": ["benchmarks/deep-search.md"]
    }
  ]
}
-->

## Search Goal

Understand how Frigg turns a multi-step search workflow into a deterministic trace artifact, a replay check, and a citation-bearing answer payload.

## Why This Playbook Exists

This playbook tests whether a user can discover:

- the playbook input shape
- the allowed step tool set
- how replay/diff works
- how source-backed citation payloads are composed

## Scope And Assumptions

- Repository scope: current `frigg` workspace
- Expected surface: `extended` for runtime tools, but useful static answers are still discoverable from code/docs without enabling the runtime profile

## Expected Tool Flow

| Step | Tool | Search / intent | Expected return cues |
| --- | --- | --- | --- |
| p01 | `read_file` | inspect `fixtures/playbooks/deep-search-suite-core.playbook.json` | deterministic playbook shape with `playbook_id` and ordered `steps` |
| p02 | `search_symbol` | search `DeepSearchHarness` | struct and impl matches in `crates/cli/src/mcp/deep_search.rs` |
| p03 | `read_file` | inspect `crates/cli/src/mcp/deep_search.rs` | allowed step tools, trace artifact schema, replay helpers, citation composition |
| p04 | `search_text` | regex search for `deep_search_run|deep_search_replay|deep_search_compose_citations` | hits in contracts, runtime server, benchmark docs, and tests |
| p05 | `read_file` | inspect `benchmarks/deep-search.md` or `crates/cli/tests/playbook_suite.rs` | benchmark and deterministic suite expectations for replayability |

## Expected Return Cues

- The internal playbook structure should contain `playbook_id` and `steps`.
- The trace artifact should use `frigg.deep_search.trace.v1`.
- The citation payload should use `frigg.deep_search.answer.v1`.
- The allowed step tool set in the harness should currently be:
- `list_repositories`
- `read_file`
- `search_text`
- `search_symbol`
- `find_references`
- Benchmarks and tests should reinforce deterministic replay, not just one-off success.

## Recording Ledger

| Step | Expected status | Recorded params_json | Recorded response summary | Notes |
| --- | --- | --- | --- | --- |
| p01 | `ok` |  |  |  |
| p02 | `ok` |  |  |  |
| p03 | `ok` |  |  |  |
| p04 | `ok` |  |  |  |
| p05 | `ok` |  |  |  |
