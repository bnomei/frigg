# Design — 47-session-workspace-attach-and-stdio-defaults

## Scope
- `crates/cli/src/main.rs`
- `crates/cli/src/settings/`
- `crates/cli/src/mcp/server.rs`
- `crates/cli/src/mcp/types.rs`
- `contracts/tools/v1/`
- `contracts/errors.md`
- `contracts/config.md`
- `README.md`
- `specs/47-session-workspace-attach-and-stdio-defaults/`
- `specs/index.md`

## Problem statement
Frigg currently binds its searchable universe to the startup `--workspace-root` list. That is workable for one repo or for a carefully curated long-lived server, but it creates two practical UX problems:

1. A central HTTP Frigg instance must be manually started with every repository the user might need, even if most sessions only touch one repo.
2. Stdio MCP clients commonly spawn one Frigg process per session. If those sessions all target the same repository, the current default watch behavior risks duplicated watchers and repeated changed-only refresh scheduling around the same repo-local SQLite storage.

The desired v1 outcome is:
- repo-local storage remains in each repository's `.frigg/storage.sqlite3`
- HTTP can start empty and attach repos on demand
- stdio can behave like a one-shot local reader by default
- clients can select a workspace as part of the MCP tool flow instead of treating startup roots as the only selection mechanism

## Non-goals
- No central/global Frigg database shared across repositories.
- No automatic full indexing or background initialization during `workspace_attach`.
- No `workspace_detach` in v1.
- No change to explicit `repository_id` semantics for existing callers.

## Approach

### 1) Split startup roots from attached workspaces
Treat startup `workspace_roots` as an optional bootstrap set for MCP serving mode instead of a mandatory invariant.

- Utility commands (`init`, `verify`, `reindex`) continue to require explicit workspace roots.
- MCP serving mode may start with zero workspace roots.
- `list_repositories` now reflects the process-attached workspace registry, not only the original startup list.

This preserves the CLI utility workflow while allowing HTTP to come up empty and wait for attach calls.

### 2) Add a process-wide attached workspace registry
Add a `WorkspaceRegistry` keyed by canonical root path.

Each attached workspace record stores:
- `repository_id`
- `display_name`
- `root_path`
- `db_path`
- storage readiness summary
- watcher/runtime bookkeeping hooks

The registry is process-wide so:
- repeated `workspace_attach` calls for the same canonical root reuse the existing record
- HTTP sessions see the same attached repo set
- future local-daemon reuse remains possible without changing storage layout

To minimize churn, v1 can keep the current runtime-scoped `repo-001`, `repo-002`, ... ID model using attached-root order within the running process. Clients already refresh IDs via `list_repositories`, so attach-order-scoped IDs are acceptable for this phase.

### 3) Add session-scoped default workspace state
Workspace selection needs to be session-local, not process-global.

Add session state keyed by the MCP session/peer identity:
- `default_repository_id: Option<String>`

Resolution precedence for existing tools becomes:
1. explicit `repository_id`
2. session default repository
3. all attached repositories

This means:
- old clients continue to work unchanged
- new clients can call `workspace_attach(..., set_default=true)` once, then omit `repository_id`
- different HTTP sessions can use different default repos while sharing the same Frigg process

### 4) Add explicit workspace-selection tools
Add two MCP tools:

#### `workspace_attach`
Purpose: register or reuse a repository-local workspace and optionally set it as the session default.

Proposed input:
```json
{
  "path": "/abs/or/nested/path",
  "set_default": true,
  "resolve_mode": "git_root"
}
```

Proposed behavior:
- accept file or directory paths
- canonicalize the effective directory
- if `resolve_mode=git_root` (default), walk ancestors to the enclosing Git root when present
- otherwise attach the canonicalized directory
- inspect `<root>/.frigg/storage.sqlite3`
- return repository metadata plus readiness state
- optionally set the session default

Proposed readiness states:
- `missing_db`
- `uninitialized`
- `ready`
- `error`

`workspace_attach` does not run `reindex`.

#### `workspace_current`
Purpose: expose the session default workspace.

Proposed response:
```json
{
  "repository": {
    "repository_id": "repo-001",
    "display_name": "frigg",
    "root_path": "/Users/bnomei/Sites/frigg"
  },
  "session_default": true
}
```

or:
```json
{
  "repository": null,
  "session_default": false
}
```

No separate `workspace_list` tool is needed in v1 because `list_repositories` already fills that role.

### 5) Stdio bootstrap auto-attach
When Frigg serves stdio and no startup roots are provided:

1. inspect the process working directory
2. resolve to Git root when possible
3. attach that root
4. set it as the session default

If Git-root discovery fails, attach the cwd itself.

This gives stdio an out-of-the-box “spawn in repo, answer from repo-local SQLite, exit” path without extra orchestration.

### 6) Transport-aware watch defaults
Current watch behavior is useful for a shared local daemon, but it is too eager for one-shot stdio sessions.

Change default watch resolution:
- stdio with no explicit CLI/env override -> effective default `off`
- HTTP with no explicit CLI/env override -> keep current `auto`
- explicit CLI/env watch settings still win

This avoids duplicate watchers and redundant changed-only refresh scheduling when many agents spawn stdio Frigg processes against the same repository.

### 7) Error behavior
No new public error code is required in v1.

Use existing typed failures:
- invalid attach path or unsupported payload -> `invalid_params`
- no repositories attached for a tool that needs one -> typed `resource_not_found` with remediation details

Recommended error details for the no-workspace case:
- `attached_repositories`
- `action`
- `hint`

## Storage model
Storage remains unchanged:
- one SQLite file per attached workspace
- path: `<workspace-root>/.frigg/storage.sqlite3`

`workspace_attach` reads storage readiness only. Indexing remains explicit through existing CLI/bootstrap paths until a future spec adds write-capable workspace-preparation tools with confirmation semantics.

## Security and path handling
- Canonicalize attach targets before registry insertion.
- Reuse existing workspace-boundary and storage-path helpers where possible.
- Accepting a file path is convenience only; effective attachment always targets a directory root.
- Attach-time Git-root resolution must not escape canonical ancestor boundaries.

## Validation strategy
- Startup tests for MCP serving mode with zero startup roots.
- Transport-default tests for stdio watch `off` versus HTTP watch `auto`.
- Attach tests covering Git-root resolution, cwd fallback, file-path input, and duplicate attach reuse.
- Session tests covering `workspace_current`, session-default precedence, and explicit `repository_id` override behavior.
- Regression coverage showing stdio can answer one-shot requests from an auto-attached repo without watch startup noise or duplicate scheduling.
