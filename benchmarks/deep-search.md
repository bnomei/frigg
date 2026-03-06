# Deep Search Evaluation Benchmarks (`v1`)

## Scope

This document defines deterministic acceptance metrics for replayable deep-search workflows executed through MCP playbooks.
It is also the benchmark contract for how Frigg turns a playbook fixture into a `DeepSearchTraceArtifact`, deterministic replay checks, and citation composition outcomes.

Suite assets:

- `fixtures/playbooks/deep-search-suite-core.playbook.json`
- `fixtures/playbooks/deep-search-suite-core.expected.json`
- `fixtures/playbooks/deep-search-suite-partial-channel.playbook.json`
- `fixtures/playbooks/deep-search-suite-partial-channel.expected.json`

Validation command:

```bash
cargo test -p frigg --test playbook_suite
```

## Surface status

The deep-search playbook runner (`crates/cli/src/mcp/deep_search.rs`) remains the internal harness used by `cargo test -p frigg --test playbook_suite`.
The same module now also backs the optional public runtime deep-search MCP tools in `tools/list` when `FRIGG_MCP_TOOL_SURFACE_PROFILE=extended`:

- `deep_search_run`
- `deep_search_replay`
- `deep_search_compose_citations`
- Those tools are benchmarked here for fixture execution, trace artifact replayability, and source-backed citation payload determinism.

Metrics in this document still focus on internal replayability and fixture conformance; public tool API compatibility is defined by `contracts/tools/v1/README.md`.

## Acceptance Metrics

1. `suite_execution_pass_rate`
- Definition: `passed_playbooks / total_playbooks`
- Target: `1.0` (all suite playbooks pass)

2. `deterministic_replay_rate`
- Definition: `deterministic_playbooks / total_playbooks`, where determinism means two consecutive runs produce equal `DeepSearchTraceArtifact` values.
- Target: `1.0`

3. `typed_error_conformance`
- Definition: `expected_error_steps_with_matching_codes / expected_error_steps`
- Target: `1.0`
- Required example covered by suite: `search_text` with `pattern_type=regex` must return typed not-ready behavior (`mcp_code=INTERNAL_ERROR`, `error_code=index_not_ready`).

4. `metadata_coverage`
- Definition: `steps_requiring_note_with_note_present / steps_requiring_note`
- Target: `1.0`
- Scope: `search_symbol` and `find_references` suite steps flagged with `require_note=true`.

## Expected Output Contract

Each `*.expected.json` fixture uses this contract:

- `suite_schema`: expected schema version (`frigg.deep_search.playbook_suite.v1`)
- `playbook_id`: must match playbook artifact `playbook_id`
- `steps[]`: deterministic per-step assertions:
  - `step_id`
  - `tool_name`
  - `status`: `ok` or `err`
  - optional `mcp_code`/`error_code` for error steps
  - optional `min_matches`/`min_repositories` bounds
  - optional `require_note` for metadata-bearing responses

## Repeatability Guidance

- Keep playbooks scoped to deterministic fixtures (`fixtures/repos/manifest-determinism`).
- Avoid environment-sensitive assertions (for example absolute filesystem paths in response payloads).
- Add new suite cases by introducing paired `*.playbook.json` and `*.expected.json` files and extending `crates/cli/tests/playbook_suite.rs`.
