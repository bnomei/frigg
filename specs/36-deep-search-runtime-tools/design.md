# Design — 36-deep-search-runtime-tools

## Scope
- crates/cli/
- crates/mcp/
- crates/mcp/tests/
- contracts/
- contracts/tools/v1/
- benchmarks/
- docs/overview.md

## Normative excerpt (in-repo)
- Deep-search harness APIs (`DeepSearchHarness`) already exist in `crates/mcp/src/mcp/deep_search.rs`.
- Current contract marks deep-search harness as internal/test-only and not part of runtime `tools/list`.
- Runtime MCP surface must stay deterministic and read-only.

## Architecture decisions
1. Add opt-in runtime feature gate for deep-search MCP tools (`enable_deep_search_tools`).
2. Reuse existing `DeepSearchHarness` logic to avoid duplicate execution semantics.
3. Keep tools read-only by returning artifacts in response payloads; no server-side artifact writes from MCP tools.

## Proposed MCP tool set
1. `deep_search_run`
- Input: `playbook` object (inline).
- Output: `trace_artifact`.

2. `deep_search_replay`
- Input: `playbook`, `expected_trace_artifact`.
- Output: `matches`, `diff`, `replayed_trace_artifact`.

3. `deep_search_compose_citations`
- Input: `trace_artifact`, optional `answer`.
- Output: typed citation payload (`claims`, `citations`).

## Handler flow
1. Validate feature gate and input schemas.
2. Instantiate harness from active `FriggMcpServer`.
3. Execute harness method.
4. Emit deterministic provenance and typed error mapping.
5. Return response payload with canonical JSON ordering guarantees.

## Safety model
- Step tool allowlist is fixed to exposed read-only tools.
- Unsupported tool names fail fast with `invalid_params`.
- Existing per-tool resource budgets still apply because harness calls server handlers.

## Determinism model
- Step order is playbook order.
- Response serialization uses canonical JSON generation already used by harness code.
- Citation IDs are generated sequentially from deterministic step/match traversal.

## Validation and rollout
- Tool schema parity tests for new deep-search tools.
- Integration tests for enable/disable gating, unsupported step behavior, and deterministic replay.
- Bench workloads for deep-search tool execution latency and replay overhead.
