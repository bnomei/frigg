# Design — 35-semantic-runtime-mcp-surface

## Scope
- crates/cli/
- crates/config/
- crates/embeddings/
- crates/index/
- crates/search/
- crates/storage/
- crates/mcp/
- contracts/
- benchmarks/
- docs/overview.md

## Normative excerpt (in-repo)
- Track C target is hybrid lexical + graph + semantic retrieval with OpenAI/Google provider abstraction.
- Embeddings contract is currently library-only and caller-owned for provider/model/key wiring.
- Existing public MCP runtime surface remains read-only and deterministic.

## Architecture decisions
1. Keep provider internals in `crates/embeddings/`; add runtime composition in CLI/MCP startup.
2. Introduce explicit semantic runtime options (provider, model, strict-mode, enable flag) in composition layer without breaking existing `FriggConfig` defaults.
3. Add a new read-only MCP tool `search_hybrid` instead of changing `search_text` semantics.
4. Preserve existing core tools unchanged and backward-compatible.

## Runtime composition model
- New semantic runtime options are sourced from explicit CLI/env inputs in the composition layer.
- Provider API keys remain environment-sourced (`OPENAI_API_KEY`, `GEMINI_API_KEY`) with fail-fast validation when semantic mode is enabled.
- Startup verifies vector readiness and semantic configuration compatibility before serving semantic-enabled requests.

## Data flow
1. Reindex path emits chunk candidates for semantic embedding.
2. Embedding provider generates vectors (batched, retry policy from `crates/embeddings/`).
3. Vectors are persisted in SQLite vector storage with deterministic provenance metadata (`provider`, `model`, `chunk_id`, `trace_id`).
4. `search_hybrid` executes lexical + graph retrieval, optionally semantic nearest-neighbor retrieval, then calls hybrid ranker.
5. Response includes per-channel scores and source/provenance identifiers; note metadata records degraded-channel behavior when applicable.

## MCP contract shape (`search_hybrid`)
- Input (high-level): `query`, optional `repository_id`, optional `language`, optional `limit`, optional channel weights, optional semantic toggle.
- Output (high-level): deterministic `matches[]` with canonical paths, ranking scores (blended + channel scores), channel source IDs, and `note` metadata.
- Error taxonomy: `invalid_params`, `index_not_ready`, `timeout`, `unavailable`, `internal`.

## Determinism and degradation policy
- Stable sort keys: blended score desc, lexical score desc, graph score desc, semantic score desc, repository/path/order tie-break.
- Degradation policy is explicit in response note:
  - `semantic_status=ok`
  - `semantic_status=degraded` with `reason`
  - `semantic_status=strict_failure` for strict mode failures.

## Validation and rollout
- Unit tests: provider wiring validation, semantic mode gating, deterministic ranking under mixed channels.
- Integration tests: end-to-end `search_hybrid` responses with and without semantic mode, deterministic replay under fixed fixtures.
- Benchmarks: semantic-on/off latency workloads added to MCP and search benchmark reports.
