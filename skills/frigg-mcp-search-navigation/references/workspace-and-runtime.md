# Workspace And Runtime

## Setup And Scope

- `list_repositories` shows the globally known repository catalog. Each row includes:
  - `repository_id`
  - `display_name`
  - `root_path`
  - session adoption state
  - watcher state
  - optional storage health
  - optional index health
- `list_repositories` is global runtime state. `workspace_current.repositories` is session-local adoption state.
- Omitted `repository_id` on repo-aware tools resolves to the session default first, then the remaining adopted repositories.

## Attach And Current

Use `workspace_attach` when the session is detached, when the default repo is wrong, or when you want later calls without `repository_id` to stay local to one repo.

Important `workspace_attach` inputs:
- `path` or `repository_id`
- `set_default`
- `resolve_mode`
  - `git_root`: prefer the enclosing Git root
  - `direct`: use the direct directory

Important `workspace_attach` outputs:
- `repository`
- `resolved_from`
- `resolution`
- `action`
  - `attached_fresh`
  - `reused_workspace`
- `session_default`
- `storage`
- `precise`

Use `workspace_current` when you need the session-local picture:
- current default repository
- all session-adopted repositories
- top-level `precise` summary
- runtime task status

Use `workspace_detach` when you want to remove one adopted repository from the current session. Omitting `repository_id` detaches the current session default.

Read the top-level `precise` block before assuming navigation should be precise. The compact fields are the ones to look at first:
- `state`
- `failure_tool`
- `failure_class`
- `failure_summary`
- `recommended_action`
- `generation_action`

## Write Tools

`workspace_prepare` and `workspace_reindex` are the write-style tools.

Shared rules:
- both accept `path` or `repository_id`
- both accept `set_default`
- both accept `resolve_mode`
- both require `confirm=true`

Use `workspace_prepare` when the repo still needs Frigg state initialized or explicitly refreshed from the client.

Use `workspace_reindex` when you want a full or changed refresh and care about the resulting counts:
- `snapshot_id`
- `files_scanned`
- `files_changed`
- `files_deleted`
- `diagnostics_count`

## Precise Generation

Frigg now auto-detects and runs supported precise generators during attach/reindex when the tools are installed and the repo shape matches.

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

Check repository health before assuming semantic help is available:
- `health.semantic`
- `workspace_current.runtime`

## Practical Guidance

- If the tool says “call `workspace_attach` first”, do that rather than retrying search tools blindly.
- Use `workspace_current` before debugging poor nav quality; it will usually tell you whether you are missing lexical state, semantic state, or precise state.
- Use `workspace_reindex` only intentionally. Normal freshness should come from watch mode.
