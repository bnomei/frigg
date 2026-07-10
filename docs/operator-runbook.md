# Frigg Operator Runbook

This runbook is primarily **diagnostic** (runtime states and recovery) and does not change Frigg behavior. The first section below is **maintainer process** (surface growth / module splits) for contributors; skip it if you only operate a deployment.

## Contributor hygiene (surface growth + mega-modules)

Product/process notes for maintainers (EXP-when-to-grow-surface **A**, EXP-split-mega-modules **opportunistic B/C/D**). Not agent-facing runtime diagnostics.

### When to grow the public MCP surface

| Prefer | When |
| --- | --- |
| **Skill loop** over existing tools | Agent can already compose symbol → text → hybrid → batch → proof |
| **Internal fix** (policy, ranking, module) | Miss is order/quality; not a missing orchestration primitive |
| **Thin composer** (`impact_bundle` / `search_batch` class) | Multi-primitive scenario needs shared handles + recovery budgets skill cannot share |
| **New public tool** | Last resort; never tool-per-scenario (`review_bundle`, packet MCP tools, host-order “fixes”) |

Public surface grows only when orchestration **cannot** be a skill loop without shared handles/recovery. Ranking/policy growth stays internal and eval-gated (no free Laravel special-case growth on the non-PHP track).

### When to split large modules

Hotspots today include `mcp/server.rs`, `search_tools/hybrid.rs`, `presentation.rs`, large nav files — partly already factored under `server/` and `search_tools/`.

| Rule | Meaning |
| --- | --- |
| **No split for LOC alone** | Split when change sets repeatedly collide (e.g. hybrid present vs pivot assist) |
| **Opportunistic** | Extract pure helpers/tests first when already editing for behavior — not a “refactor week” |
| **One registration manifest** | Keep `PUBLIC_TOOL_NAMES` + `#[tool]` discoverable; thin router OK; do not scatter tool names across crates |
| **Policy tree** | `searcher/policy/` is already stage-split; more folders need ownership docs, not file count |
| **No crate split for hygiene** | Avoid `frigg-mcp` / `frigg-rank` microservice factorization without a monorepo benefit case |

Model new composition modules on small callers of existing impls (`impact_bundle` pattern).

## First checks

1. Confirm the client is connected to the expected Frigg service.
2. Call `workspace_current` to inspect session-local adoption, the session default repository, compact precise status, compact precise status, and runtime status.
3. If the repository is not adopted in the current session, call `workspace_attach` with the intended repository and review its `index_lifecycle` and `precise_lifecycle` fields before debugging search quality.
4. Use `list_repositories` when you need the global known-repository catalog. `workspace_current.repositories` is session-local and can be empty even when `list_repositories` knows about repositories.

## Workspace and index lifecycle

`workspace_current` is the safest starting point because it reports what this MCP session has adopted. Adoption is separate from global repository discovery: a repository can be known globally but unavailable to repo-aware tools in a session until it is attached.

`workspace_attach` defaults to `index_mode=ensure`. In that mode, Frigg refreshes missing or stale lexical and semantic state when possible, waits up to the configured `index_timeout_ms`, and returns `index_lifecycle` so callers can decide whether to search immediately or wait.

| State | Meaning | Operator action |
| --- | --- | --- |
| `ready` | Attach-time index work is complete enough for the reported repository health. `lexical_ready` and `semantic_ready` show which channels are ready. | Proceed with search/navigation. If semantic is not ready but optional semantic search is disabled, this is expected. |
| `refreshing` | Index work is active and the attach call returned before it reached a terminal state. | Wait and re-check `workspace_current`, or call `workspace_attach` again with waiting enabled if the client needs a blocking readiness check. |
| `refresh_queued` | Refresh work was queued but has not started yet, often because another task is active. | Treat results as potentially stale until a later `ready`; inspect `active_tasks` for the in-flight task. |
| `timeout` | Frigg waited for attach-time work but the timeout elapsed. | Increase `index_timeout_ms`, retry after the active task finishes, or run `workspace_index` if stale state persists. |
| `failed` | Attach-time refresh failed. `failure_summary` and `recommended_action` explain the failure when available. | Follow `recommended_action`; common actions are rerun reindex, check environment, or use heuristic mode while repairing precise/semantic inputs. |
| `skipped` | The caller requested `index_mode=skip`, so no attach-time indexing ran. | Use only when stale or missing index state is acceptable. Run `workspace_index` or attach with `ensure` when freshness matters. |
| `stale` | The index is not ready and no refresh is running or queued, and the caller did not request a skip. Typically seen from `workspace_current` when files changed without watch active. | Run `workspace_index`, or attach with `index_mode=ensure`, to refresh the repository. |
| `unavailable` | Frigg could not evaluate index lifecycle for this repository. | Verify the repository is attached and storage is accessible, then retry attach or reindex. |

`index_mode=skip` is scoped to lexical and semantic index refresh. It still adopts the repository into the MCP session, returns current health, and may probe or schedule precise-generator execution. Frigg treats ignored `.frigg/` runtime artifacts as outside the source-read-only scope.

Use `workspace_prepare` or `workspace_index` only when you intentionally want to initialize or refresh Frigg state from a client. These tools are confirm-gated because they operate on ignored `.frigg/` state.

## Semantic models (catalog + presets)

Curated embedding-model defaults and storage contract facts live on the MCP policy resource:

- URI: `frigg://policy/semantic-models.json` (`schema_id: frigg.policy.semantic_models.v1`)
- Peer to language tiers at `frigg://policy/support-matrix.json` (do not confuse the two axes)
- **models[]**: provider defaults, native dims, pad-to-projection (store width 1536), offline?, credential env, `reindex_on_change`, known limits
- **presets[]** (soft intent aliases only — EXP-code-presets C):

| Preset id | Expands to | Notes |
| --- | --- | --- |
| `offline-small` | `local` + `all-MiniLM-L6-v2` | Zero-cloud **offline_smoke** MiniLM; still need `FRIGG_SEMANTIC_RUNTIME_ENABLED=true` |
| `cloud-openai` | `openai` + `text-embedding-3-small` | Requires `OPENAI_API_KEY` |
| `cloud-google` | `google` + `gemini-embedding-001` | **Credential peer** when `GEMINI_API_KEY` already present (not preferred cloud default) |
| `openai-compat-selfhost` | `openai_compat` + endpoint + model | Requires full embeddings URL + `FRIGG_OPENAI_COMPAT_API_KEY` |

- Preset `id` is **documentation** (`cli_alias: false`). Set provider+model env/config from `expands_to`. Storage partition identity remains **provider + model strings**, never the preset id alone.
- **Not** CLI flags (B deferred). **Not** brand embedding vendors like Voyage/Cohere (deferred). **Not** auto local-vs-cloud by key presence (E rejected).
- **`quality_scores: curated`** — defaults and contract facts. Early multi-repo playbook validation informed defaults; Frigg does not ship a retained public embedding scoreboard.
- Semantic runtime stays **off by default**; when enabled without a cloud provider, Frigg uses local MiniLM

### Local MiniLM role (`offline_smoke`)

| Fact | Guidance |
| --- | --- |
| Role | Default **offline smoke** / zero-key accelerator when semantic is enabled without keys |
| Model class | General-purpose MiniLM — not a code-specialized embedder; still useful for offline semantic |
| Agent loop | Hybrid with MiniLM still requires exact `search_text` / `search_symbol` pivots before proof |
| Embed envelope | Index-time documents get `path:` + `language:` headers; pure source stays in stored `content_text` for excerpts. Template bumps re-hash chunks → run a **full** `frigg index` (not changed-only) so partitions do not mix old/new envelopes |

Catalog: `quality_tier: offline_smoke` on `local-minilm-l6-v2` / preset `offline-small` in `frigg://policy/semantic-models.json`.

### Google Gemini role (`credential_peer`)

| Fact | Guidance |
| --- | --- |
| Role | Supported **credential-ecosystem peer** when `GEMINI_API_KEY` is already available |
| Positioning | Bring-your-key peer — not Frigg’s preferred cloud default over OpenAI |
| When to choose | Gemini-centric shops / existing Gemini keys |
| When not | OpenAI-only hosts: leave `provider=openai` or `local`; multi-key is never required |
| Client | Keep task types + batch + `output_dimensionality` (already first-class); Vertex enterprise path deferred |
| Catalog quality | Same **curated** bar as other cloud models (no public leaderboard rows) |

Catalog: `quality_tier: credential_peer` on `google-gemini-embedding-001` / preset `cloud-google`.

### OpenAI-compatible endpoints (`provider=openai_compat`)

Use when embeddings speak the OpenAI HTTP protocol but are **not** official OpenAI (vLLM, LM Studio, Azure-compatible deployments, internal gateways).

```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=openai_compat
export FRIGG_SEMANTIC_RUNTIME_OPENAI_COMPAT_ENDPOINT=http://127.0.0.1:1234/v1/embeddings
export FRIGG_OPENAI_COMPAT_API_KEY=<token-or-dummy>
export FRIGG_SEMANTIC_RUNTIME_MODEL=<backend-model-id>   # when not text-embedding-3-small
frigg index
```

| Setting | Role |
| --- | --- |
| Endpoint | **Required** full embeddings **POST** URL (not a bare host) |
| API key | Bearer: `FRIGG_OPENAI_COMPAT_API_KEY`, else `OPENAI_API_KEY` |
| Model | Free string; defaults to `text-embedding-3-small` protocol default — set to the backend id |
| Storage partition | `provider=openai_compat` + model (distinct from `openai`) |

Endpoint is **not** part of the storage key: two different URLs with the same model string share one partition. Reindex after changing model (and treat endpoint swaps carefully if vector spaces differ).

After changing provider or model, run `frigg index` for a semantic pass.

### Why the DB pads (and what “dimensions” mean)

Semantic vectors live in one sqlite-vec table with a **fixed** column width
(`DEFAULT_VECTOR_DIMENSIONS` = **1536**): every row is `embedding float[1536]`.

| Concept | Meaning |
| --- | --- |
| **Real / native dimensions** | What the model actually outputs (or Frigg requests from the API) **before** any store pad. Catalog field: `native_dimensions` on each model. **Never** the padded length. |
| **Projection dimensions** | Store schema width (1536). Catalog field: top-level `projection_dimensions`. |
| **Pad** | If native &lt; projection, Frigg zero-fills on write/query so the vector fits the table. `pad_to_projection: true` only then. Pad is **storage interoperability**, not a quality upgrade. |

**Why pad at all?** One vector index can hold multiple providers without a separate table per native width. Local MiniLM is 384-d; OpenAI-small is 1536-d. Without pad, short models could not share the fixed-width schema. Oversize vectors are rejected.

**Cosine / partitions:** Similarity is only meaningful **within** a `(repository_id, provider, model)` partition under matched pad policy. MiniLM-padded rows are not mixed with OpenAI rows in one head. Switching provider/model requires a semantic reindex (`frigg index`); partitions do not auto-heal.

**Agent-facing JSON** (`frigg://policy/semantic-models.json`): model rows expose **real** `native_dimensions` only (e.g. MiniLM **384**). They do **not** report 1536 as the model size when the model is padded. Store width is only `projection_dimensions` + `pad_to_projection`.

## Hybrid graph channel vs navigation graph

Hybrid `search_hybrid` can score paths via a **graph channel**. That channel shares
relation vocabulary with MCP navigation (`SymbolGraph`, `RelationKind` in
`crates/cli/src/graph`) but is a **separate runtime pipeline**:

| Surface | What it is | What it is not |
| --- | --- | --- |
| Hybrid graph channel | Ranking-time expansion from lexical seeds (durable path projections and/or ephemeral file analysis) | Not `incoming_calls` / `go_to_definition` / SCIP call hierarchy |
| MCP navigation tools | Target resolution → precise and/or heuristic nav edges | Not hybrid fusion scores |

Agent-facing honesty (EXP-nav-hybrid-graph-channel):

- Compact `ranking_note` may include `hybrid graph is ranking signal (not nav call edges)` when graph contributed.
- Per-match `graph_mode` when graph sources exist: `projection` | `heuristic_symbol_graph` | `heuristic_implementation` | `unknown` (lowest-confidence wins if mixed).
- Full `response_mode` channel metadata for `graph_precise` includes `pipeline: hybrid_ephemeral` + note.
- After hybrid graph neighbors: prove with `search_symbol` / `find_references` / `incoming_calls`, not rank-1 alone.

Post-edit: hybrid projections and precise nav caches can both go stale independently — check workspace freshness for both search and nav.

## Semantic degraded mode

Semantic retrieval is optional. When disabled or unavailable, Frigg still searches with lexical, path/witness, graph, symbol, and structural evidence. `search_hybrid` reports semantic participation in its execution note and channel health.

| Semantic status | Meaning | Operator action |
| --- | --- | --- |
| `ok` | Semantic retrieval participated successfully. | No action needed. |
| `disabled` | Semantic runtime is not enabled or this query did not request semantic recall. | Expected unless semantic search was intended; enable semantic runtime and reindex if needed. |
| `unavailable` | Semantic retrieval could not run, for example because provider configuration or stored semantic state is missing. | Check semantic environment variables and run `frigg reindex` or `workspace_index` after enabling semantic search. |
| `degraded` | Semantic retrieval was requested but fell back or returned partial/no semantic evidence while other channels remained usable. This is not the same as a hard failure. | Treat the answer as lexical/graph-grounded. Inspect `semantic_reason`, provider credentials, rate limits, and semantic index freshness. Reindex after provider recovery if stored embeddings are stale. |
| `filtered` | Semantic results were filtered out by query or channel policy. | Usually no operator action; adjust query or filters if semantic recall was expected. |
| `strict_failure` error | Strict semantic mode converts semantic provider failures into user-visible errors. | Disable strict mode for graceful fallback, or repair the provider/configuration before retrying. |

A degraded semantic state means Frigg remains usable, but natural-language recall may be weaker. Operators should distinguish this from `failed` lifecycle states, where an indexing or generation task did not complete successfully.

## Precise partial and failure states

Precise navigation comes from optional SCIP artifacts and best-effort automatic generators. **Precise is an optional accelerator, not the product core.** Frigg keeps working without precise data by using Tree-sitter, source-backed heuristics, lexical search, and structural tools. Agents should treat heuristic `NavigationMode` as valid Frigg. Installing `scip-*` / language-server toolchains is a **host/environment** concern — Frigg does not own or ship generator installers. Prefer non-blocking attach: search and heuristic nav remain usable while generation runs in the background; do not block the default agent loop on generators.

`workspace_current.precise`, `workspace_attach.precise`, and `workspace_attach.precise_lifecycle` are the compact operator surfaces. Explicit diagnostics expose lower-level `health.scip`, `health.precise_ingest`, and `health.precise_generators` details. The generator scorecard includes discovery state, tool/version, expected artifact path, repo-local writes/executions/patch risk, last generation duration, artifact counts/bytes, failure class, and recommended action.

| Precise state | Meaning | Operator action |
| --- | --- | --- |
| `ok` | Precise artifacts were available and ingested with usable coverage. | Prefer precise navigation tools; no action needed. |
| `partial` | Some precise data was ingested, but coverage is incomplete. Navigation may mix precise hits with heuristic fallback. | Use precise results when present, but verify with `read_file`, `search_symbol`, or structural search. Check sampled ingest failures and generator output if missing areas matter. |
| `failed` | Precise generation or ingest failed. `failure_tool`, `failure_class`, `failure_summary`, and `recommended_action` identify the likely cause when available. | Follow the recommended action: install missing tools, check environment, rerun reindex, or use heuristic mode until upstream tool failures are fixed. |
| `unavailable` | No usable precise source is available for this repository or language. | This is expected for unsupported layouts or missing optional artifacts. Use heuristic/source-backed navigation or provide `.frigg/scip/` artifacts. |

Precise lifecycle phases describe generation timing separately from the compact state: `running` and `timeout` mean generation may still be in progress or incomplete, while `failed` means it reached a terminal failure. `wait_for_precise=true` on `workspace_attach` and `workspace_index` waits for a terminal phase when possible; `wait_for_precise=false` skips only that wait and leaves generator scheduling behavior unchanged.

## Watch retries

Built-in watch mode runs behind **`frigg serve` (HTTP)** and keeps adopted repositories fresh while sessions hold watcher leases. Full post-edit freshness is an **HTTP + watch lease** contract. **Stdio** sessions are a valid single-client mode; transport defaults set **`WatchMode::Off`** for stdio, so they often report `watch_status.reason=mode_off` — that is expected, not a ranking defect. Explicit `--watch-mode auto|on` can enable watch on stdio; multi-client freshness still prefers HTTP + leases. Prefer path-scoped live-disk after edits on default stdio, or run shared work over loopback HTTP. A file event queues a changed-only manifest refresh after the debounce delay. When semantic runtime is enabled, a successful manifest refresh may queue a semantic follow-up refresh.

If a watch refresh fails, Frigg logs `built-in watch mode refresh failed; retry scheduled`, marks the refresh pending again, and waits `watch.retry_ms` before retrying. The default retry delay is controlled by `--watch-retry-ms` or `FRIGG_WATCH_RETRY_MS`.

If the failure cause is `database is locked`, another Frigg process, test, or stale in-flight refresh is holding SQLite's writer lock on `.frigg/storage.sqlite3`. Frigg waits up to `FRIGG_SQLITE_BUSY_TIMEOUT_MS` before surfacing that error; the default is 30000 ms. For test-heavy local development, increase that value or disable built-in watch mode while tests are writing the same repository storage.

| Watch condition | Meaning | Operator action |
| --- | --- | --- |
| Debouncing | A file event was observed and Frigg is waiting for the debounce window before refreshing. | Wait for the debounce interval; this is normal during active edits. |
| Refreshing | A manifest or semantic follow-up refresh is running. | Search may briefly reflect the previous snapshot. Re-check after the task finishes. |
| Retrying | The previous refresh failed and the scheduler has a retry deadline. | Check logs for the failure cause, verify storage/provider/tool availability, and wait for the next retry or run a manual `workspace_index` after fixing the cause. |
| Re-run requested | Another event arrived while a refresh was active. | Frigg will queue another refresh after the active one succeeds. No manual action unless the queue never drains. |

Turn watch mode off with `--watch-mode off` when an external watcher already maintains Frigg state, to avoid duplicate refresh work.

## MCP registration vs Frigg product faults (FUT-003)

Classify before filing a Frigg product bug:

| Symptom | Class | Owner | Operator action |
| --- | --- | --- | --- |
| Frigg absent from live `tools/list` in a Task/subagent while parent still has Frigg (often surfaces as `Tool not found` on a known Frigg tool) | **Harness MCP registration / inheritance** | Host harness (Claude Task, Cursor agent, etc.) | (1) Re-probe live tools on the child; shell/Grep fallback is correct for that spawn. (2) Fix host MCP inheritance / child MCP config so the child actually receives Frigg. **Not** a ranking, hybrid, or index bug. |
| Schema/descriptor files under host `mcps/frigg/tools/` exist but agent cannot call Frigg | Same — **schema on disk ≠ runtime registration** | Host | Do not treat descriptors as proof Frigg is live; re-check `tools/list`. |
| HTTP connect refused / timeout to `127.0.0.1:37444/mcp` | Transport / process | Operator | Start `frigg serve`; verify port and adopt MCP URL. |
| Frigg tools present; weak/wrong hits | Search / index / ranking | Frigg product | Use this runbook’s workspace, semantic, and precise sections. |
| Grep-first while Frigg tools are registered | Host tool-order preference | Host / soft policy | Soft hooks and skill only; Frigg does not hide Grep (see harness policy pack). |
| `Tool not found` for an invented / extended-only name while other Frigg tools work | Surface honesty / profile | Agent / config | Use live `tools/list` / `runtime.tools_exposed`; not FUT-003 inheritance. |

**Wontfix product (FUT-003):** Guaranteeing third-party subagent MCP inheritance, controlling built-in Grep order, or failing Frigg CI because a host Task tool flaked. (Dogfood hosts may include Grok-class agents; the boundary is harness reliability, not a single vendor.)

**When multi-client work matters:** After each spawn has Frigg registered, prefer loopback HTTP + `frigg serve` so clients share one runtime. Shared HTTP does **not** create registration on a child that never received Frigg tools.

Agents should **probe-on-spawn** (skill language): verify Frigg via live `tools/list` (or a successful Frigg call) for *this* Task/subagent — not parent tools or on-disk descriptors.

## Quick diagnosis map

- Search results are stale: inspect `workspace_current`, then attach with `index_mode=ensure`; if lifecycle is `refreshing`, `refresh_queued`, or `timeout`, wait or run `workspace_index`.
- Natural-language recall is weak but text search works: inspect semantic status. `degraded` means fallback is active; repair provider/configuration or semantic freshness if semantic recall is required.
- Definition/reference jumps are incomplete: inspect `workspace_current.precise` and `health.precise_ingest`. `partial` means verify hits and fill missing SCIP coverage if needed.
- Precise tools fail outright: inspect `failure_class` and `recommended_action`, then install tools, fix environment, or rerun reindex.
- Watch does not appear current: check whether the repository is adopted by an active session, then inspect logs for retry-scheduled messages and the configured retry/debounce intervals.
- `Tool not found` / Frigg missing only on Task children: see **MCP registration vs Frigg product faults** above — harness inheritance, not search quality.
