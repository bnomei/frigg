# Frigg Operator Runbook

This runbook describes the runtime states operators are most likely to see while running Frigg. It is diagnostic only: it documents current behavior and recovery expectations without changing runtime behavior.

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
| `offline-small` | `local` + `all-MiniLM-L6-v2` | Zero-cloud; still need `FRIGG_SEMANTIC_RUNTIME_ENABLED=true` |
| `cloud-openai` | `openai` + `text-embedding-3-small` | Requires `OPENAI_API_KEY` |
| `cloud-google` | `google` + `gemini-embedding-001` | Requires `GEMINI_API_KEY` |

- Preset `id` is **documentation** (`cli_alias: false`). Set provider+model env/config from `expands_to`. Storage partition identity remains **provider + model strings**, never the preset id alone.
- **Not** CLI flags (B deferred). **Not** new embedding brands (D deferred). **Not** auto local-vs-cloud by key presence (E rejected).
- **`quality_scores: unbenchmarked`** — not a CI leaderboard
- Semantic runtime stays **off by default**; when enabled without a cloud provider, Frigg uses local MiniLM

After changing provider or model, run `frigg index` for a semantic pass.

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
