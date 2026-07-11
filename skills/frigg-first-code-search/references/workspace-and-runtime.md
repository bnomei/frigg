# Workspace And Runtime

## Setup And Scope

- `workspace` shows the current session default and the globally known repository catalog. Each row includes:
  - `repository_id`
  - `display_name`
  - `root_path`
  - session adoption state
  - watcher state
  - optional storage state
- `workspace.repositories` is global known runtime state. Each row includes session adoption state.
- Omitted `repository_id` on repo-aware tools resolves to the session default first, then the remaining adopted repositories.
- If no repository is adopted, repo-aware tools may auto-adopt a sensible default such as the only known startup repository or the server's current repository.

## Workspace Tool

Use `workspace` when the session is detached, when the default repo is wrong, or when you want later calls without `repository_id` to stay local to one repo.

Important `workspace` inputs:
- `path` or `repository_id`
- `set_default`
- `resolve_mode`
  - `git_root`: prefer the enclosing Git root
  - `direct`: use the direct directory

Important `workspace` outputs:
- `repository`
- `session_default`
- `repositories`
- `runtime` — includes `tool_surface_profile` (`core`|`extended`), **`tools_exposed`**, and **`watch_status`** (`reason`, `lease_count`, optional `repository_id` / `detail`, dual-class **`refresh_queue_depth`**, **`pending_dirty_path_count`**, **`oldest_pending_age_ms`**)
- `recommended_action` — session gate next step (`ready`, `adopt_repo`, `wait_watch`, `reindex`, `use_live_disk_for_touched_files`, …) — **primary agent decision**
- `gate_hint` — optional plain-language recovery when the action is non-obvious (especially `reindex`)
- `working_tree_dirty`, `changed_paths_since_snapshot`, `watch_active`, `fresh_enough_for`
- `lexical_ready` / `semantic_ready` — optional substrate flags only; not full health scorecards; do not install generators mid-task from these alone

**`watch_status.reason` values (compact):** `mode_off`, `no_lease`, `debouncing`, `refreshing`, `active` (plus reserved: `retry_backoff`, `blocked`, `notify_degraded`). Use to **explain** `wait_watch`, not to replace the gate action. Queue fields are dual-class only (`manifest_fast` + `semantic_followup`) — no third agent-hot queue; high dirty count / age → path-scoped live reads of touched paths.

**Tool surface honesty / live SSOT:**
- Process: `runtime.tools_exposed` or live `tools/list` for **this** server
- Machine catalog: MCP resource `frigg://policy/tool-surface.json` (`live: true`, `active_tools`, core vs extended) — generated from `PUBLIC_TOOL_NAMES` + profile manifests
- Code: `PUBLIC_TOOL_NAMES` / `manifest_for_tool_surface_profile` in the Frigg crate

**Not authoritative:** Phase 0 / systems inventory freezes, host schema caches, non-public `#[tool]` handlers. Lifecycle tools such as `workspace_index` / `workspace_attach` are not public and never appear in `tools_exposed` / `active_tools`.

**`recommended_action=reindex` is not an MCP tool.** Public Frigg MCP has no reindex/write tool. It means the index substrate is not Ready: run CLI `frigg index` (or operator lifecycle / attach-side ensure). Prefer reading `gate_hint` when present; do not invent `workspace_reindex` or shell-grep the repo as a trust patch.

Repo-aware tools auto-adopt when they can resolve a sensible default or a supplied `repository_id`. Use CLI `frigg init` and `frigg index` for explicit maintenance refreshes.

## Precise Generation

Frigg auto-detects and runs supported precise generators during attach/index when the tools are installed and the repo shape matches.

Current auto-generation families:
- Rust
- Go
- TypeScript / JavaScript
- Python
- PHP
- Kotlin on Gradle/KTS workspaces with Kotlin source files

Manual `.frigg/scip/` drops are still valid when:
- the generator is installed in a layout Frigg does not probe
- the repo needs a manual workflow
- you want to pre-populate artifacts yourself

Repository-local precise config lives at `.frigg/precise.json`. Use it for:
- disabling one generator for one repo
- adding `generator_extra_args`
- excluding paths from filtered generation workspaces
- excluding paths from ingest

## Semantic Runtime

Semantic retrieval is optional and runtime-configured. When enabled, it participates in reindex and watch-driven refresh, so it can call the embedding provider automatically over time.

For explicit semantic troubleshooting only, inspect `workspace.runtime`.

## Practical Guidance

- `workspace` is a **gate, not a preamble**: call it when adoption is uncertain, paths look wrong, zeros are surprising, multi-repo default may be wrong, or post-edit freshness matters. Skip it when the first search already hits expected paths and the index is ready.
- If a tool says it cannot resolve a repository, call `workspace` with `path=<repo root or any file inside it>`.
- Use `workspace` to see the session default and runtime tasks when debugging wrong-repo or freshness issues.
- After edits: check workspace freshness, then either re-search with Frigg or use **path-scoped** live reads for touched files only — never treat live-disk as a license for repo-wide shell grep.
- Do not reuse a pre-edit `result_handle` / `read_match` pair for touched paths after a freshness transition; re-run the originating search/navigation tool for a new pair. `read_match` also fail-closes with `STALE_PROOF_ANCHOR` (and no bytes) if its revision-bound source changed, was deleted, or cannot be verified. Watch may drop only dirty-path anchors; reindex and unknown dirty sets wipe the repo's handles.
- `read_file` is the explicit current-live-content path. It is appropriate when historical proof is not needed, but it does not refresh a stale proof pair.
- Branch on gate: `ready` → Frigg; `use_live_disk_for_touched_files` → touched paths only; `wait_watch` → wait for watch (read `runtime.watch_status`); `reindex` → CLI `frigg index` / operator (not MCP).
- Health vocab: prefer `recommended_action` + recovery zeros; use `lexical_ready`/`semantic_ready` only as secondary substrate signals (never full generator scorecards mid-task).
- Progressive disclosure: skill scenarios are agent SSOT; `frigg://policy/*` resources are machine/host secondary surfaces — keep aligned but not a second full skill.
- **Transport dual mode (hosts choose):**
  - **HTTP** (`frigg serve`, adopt-managed `type: http` → `http://127.0.0.1:37444/mcp`): full freshness contract — watch leases, shared caches, multi-client attach.
  - **Stdio** (client spawns `frigg`): valid single-client/ephemeral contract; transport default is **`WatchMode::Off`** (often `watch_status.reason=mode_off`). Explicit Auto/On can enable watch on stdio. Not broken product — do not claim HTTP-style post-edit freshness or blame ranking for stdio-without-watch staleness. Use `runtime.profile` + `watch_status` together.
  - Prefer HTTP as soon as a second client or subagent shares the repo. Managed adopt **writes** HTTP MCP server entries (`type: http`); hand-written stdio `command` entries stay diverged unless `--force`.
- Harness-specific MCP registration/schema flukes and tool-order preferences are outside Frigg's product contract (**FUT-003**); trust `runtime.tools_exposed` / live `tools/list` over stale host schema caches.
- **Subagent / Task bridge:** Parent can call Frigg while a child spawn intermittently has no Frigg on live `tools/list` (often `Tool not found` on a known Frigg tool) even when host schema files exist. That is **intermittent host registration**, not “subagents never have MCPs” and not hybrid/ranking failure. Invented/phantom names or extended-only tools missing while other Frigg tools work are surface honesty, not inheritance.
  - **Probe-on-spawn:** before Frigg-first routing in a new Task/subagent, confirm Frigg via live host `tools/list` (or a successful Frigg call) — not on-disk descriptors and not the parent session’s tool list. If missing → shell fallback + note harness gap once; do not claim Frigg-first compliance for that spawn.
  - Prefer **HTTP `frigg serve`** when several *registered* clients/subagents should share one runtime. HTTP does not invent child registration.
  - Do **not** invent Frigg `bridge_health` / `subagent_mcp_status` tools — if registration failed, the call never reaches Frigg. Do **not** make Frigg CI depend on host Task MCP inheritance.
- Use CLI `frigg index` only intentionally, usually when you explicitly want to refresh repository-derived data.
