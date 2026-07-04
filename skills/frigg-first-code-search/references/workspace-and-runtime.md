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
- `runtime`

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

- If a tool says it cannot resolve a repository, call `workspace` with `path=<repo root or any file inside it>`.
- Use `workspace` to see the session default and runtime tasks only when debugging poor navigation quality.
- Use CLI `frigg index` only intentionally, usually when you explicitly want to refresh repository-derived data.
