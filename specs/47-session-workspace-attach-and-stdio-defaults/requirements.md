# Requirements — 47-session-workspace-attach-and-stdio-defaults

## Goal
Session Workspace Attach and Stdio One-Shot Defaults

## Functional requirements (EARS)
- WHEN Frigg starts in MCP serving mode without startup workspace roots THEN THE SYSTEM SHALL remain available for `workspace_attach`, `workspace_current`, and `list_repositories` calls instead of failing startup validation.
- WHEN `workspace_attach` is called with a path inside a Git worktree and `resolve_mode=git_root` or the mode is omitted THEN THE SYSTEM SHALL resolve and attach the canonical Git top-level directory as the workspace root.
- IF `workspace_attach` is called with a path that is not inside a Git worktree THEN THE SYSTEM SHALL canonicalize and attach the provided directory as the workspace root.
- IF `workspace_attach` is called with a file path THEN THE SYSTEM SHALL resolve from the file's parent directory before Git-root or directory attachment rules are applied.
- WHEN `workspace_attach` targets an already attached canonical root THEN THE SYSTEM SHALL reuse the existing repository registration and SHALL not create duplicate watcher or registry entries for that root.
- WHEN `workspace_attach` succeeds with `set_default=true` THEN THE SYSTEM SHALL set the attached repository as the default workspace for the calling MCP session.
- WHEN `workspace_current` is called THEN THE SYSTEM SHALL return the calling session's default workspace metadata, or `null` when no default workspace is selected.
- WHEN `list_repositories` is called THEN THE SYSTEM SHALL return all repositories currently attached to the running process, including workspaces attached after startup.
- WHEN a read, search, or navigation tool is called without `repository_id` and the calling session has a default workspace THEN THE SYSTEM SHALL scope that request to the default workspace.
- WHEN a read, search, or navigation tool is called without `repository_id` and the calling session has no default workspace THEN THE SYSTEM SHALL preserve fan-out behavior across all attached workspaces.
- IF a read, search, or navigation tool is called while no workspaces are attached THEN THE SYSTEM SHALL return a typed error instructing the client to call `workspace_attach` or start Frigg with `--workspace-root`.
- WHEN Frigg starts in stdio transport without explicit startup workspace roots THEN THE SYSTEM SHALL resolve the process working directory to a Git root when possible, attach it, and set it as the default workspace for that stdio session.
- IF stdio startup working directory is not inside a Git worktree THEN THE SYSTEM SHALL canonicalize and attach the working directory itself as the default workspace.
- WHEN `workspace_attach` inspects an attached workspace THEN THE SYSTEM SHALL report repo-local storage readiness from `<workspace-root>/.frigg/storage.sqlite3` without forcing reindex or any write operation.
- WHEN Frigg starts in stdio transport and watch mode is not explicitly set by CLI flag or env var THEN THE SYSTEM SHALL default built-in watch mode to `off`.
- WHEN Frigg starts in HTTP transport and watch mode is not explicitly set by CLI flag or env var THEN THE SYSTEM SHALL preserve the current `auto` watch behavior.

## Non-functional requirements
- Repo-local storage SHALL remain per workspace under `<workspace-root>/.frigg/storage.sqlite3`; v1 SHALL NOT introduce a central cross-repository database.
- Session workspace attach v1 SHALL remain backward-compatible for callers that already pass explicit `repository_id`.
- Session workspace attach v1 MAY keep `repository_id` values runtime-scoped; clients SHALL continue refreshing repository IDs from `list_repositories`.
- `workspace_attach` v1 SHALL be read-mostly: it may inspect storage readiness metadata, but SHALL NOT run `init`, `verify`, `reindex`, or any write-heavy indexing path automatically.
