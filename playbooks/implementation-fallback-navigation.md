# Implementation Fallback Navigation

<!-- frigg-playbook
{
  "schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "implementation-fallback-navigation",
  "query": "find EmbeddingProvider implementations and fallback when precise navigation data is missing",
  "top_k": 8,
  "allowed_semantic_statuses": ["ok", "disabled", "degraded"],
  "required_witness_groups": [
    {
      "name": "fallback-runtime",
      "paths": [
        "crates/cli/src/mcp/server.rs",
        "crates/cli/src/mcp/types.rs"
      ]
    },
    {
      "name": "implementation-runtime",
      "paths": ["crates/cli/src/embeddings/mod.rs"],
      "required_when": "semantic_ok"
    }
  ],
  "target_witness_groups": [
    {
      "name": "tests",
      "paths": ["crates/cli/tests/tool_handlers.rs"]
    }
  ]
}
-->

## Search Goal

Find all `EmbeddingProvider` implementations and verify what the user sees when precise code-navigation data is present versus absent.

## Why This Playbook Exists

This is a compact test of a major code-search promise: semantic navigation should work when precise data exists, and the system should still produce something useful when it does not.

## Scope And Assumptions

- Repository scope: current `frigg` workspace
- Expected surface: `core`
- `find_implementations` may legitimately return zero matches when precise SCIP data is absent; that fallback path should be documented, not treated as a surprise

## Expected Tool Flow

| Step | Tool | Search / intent | Expected return cues |
| --- | --- | --- | --- |
| p01 | `search_symbol` | search `EmbeddingProvider` | trait plus implementation-related symbol matches in `crates/cli/src/embeddings/mod.rs` |
| p02 | `find_implementations` | resolve `EmbeddingProvider` implementations | ideal outcome: concrete impl matches; fallback outcome: empty with `precise_absent` note metadata |
| p03 | `search_structural` | search Rust impl nodes in `crates/cli/src/embeddings/mod.rs`, narrowed to `EmbeddingProvider` impls | structural fallback finds `OpenAiEmbeddingProvider`, `GoogleEmbeddingProvider`, and `DummyProvider` impl lines even when precise navigation is missing |
| p04 | `document_symbols` | inspect `crates/cli/src/embeddings/mod.rs` | deterministic outline of trait, structs, and impl blocks |
| p05 | `read_file` | read `crates/cli/src/embeddings/mod.rs` if a final human check is needed | bounded canonical file content for the exact impl area |

## Expected Return Cues

- `search_symbol` should show the `EmbeddingProvider` trait and the concrete impl blocks for `OpenAiEmbeddingProvider`, `GoogleEmbeddingProvider`, and `DummyProvider`.
- `find_implementations` should either return those impls or explain why it could not through precise/fallback metadata.
- `search_structural` is the deterministic escape hatch when precise implementation lookup is unavailable.
- If the structural query needs tuning, the backup text probe is literal `impl EmbeddingProvider for`.

## Recording Ledger

| Step | Expected status | Recorded params_json | Recorded response summary | Notes |
| --- | --- | --- | --- | --- |
| p01 | `ok` |  |  |  |
| p02 | `ok` or empty-with-note |  |  |  |
| p03 | `ok` |  |  |  |
| p04 | `ok` |  |  |  |
| p05 | `ok` |  |  |  |
