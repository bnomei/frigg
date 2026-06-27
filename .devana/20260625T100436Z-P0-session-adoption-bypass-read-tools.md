DEVANA-FINDING: v1
Priority: P0 | Confidence: high | Security-sensitive: yes | Status: fixed
Location: crates/cli/src/mcp/server/workspace.rs:222 | Slug: session-adoption-bypass-read-tools

# MCP read/search tools bypass session adoption when repository is explicit or path is absolute

## Finding

README documents that `read_file` reads "inside an adopted repository" and that omitted `repository_id` scopes to adopted repos. Two code paths reach globally known startup repositories without checking `adopted_repository_ids`: supplying an explicit `repository_id`, and supplying an absolute path with no `repository_id`.

## Violated Invariant Or Contract

Session adoption (`workspace_attach`) is the access boundary for repo-scoped MCP tools. A detached session must not read, search, or navigate repositories that were never adopted in that session.

## Oracle

README: "`read_file`: read a file safely inside an adopted repository"; "`workspace_detach` removes adoption"; "For repo-aware tools with omitted `repository_id`, Frigg scopes to the session default first, then the remaining adopted repositories." `list_repositories` exposes `session.adopted: false` for non-adopted repos.

## Counterexample

1. `frigg serve --workspace-root /repo-a --workspace-root /repo-b` registers both repos globally.
2. MCP session never calls `workspace_attach` (or adopts only A).
3. `read_file { "repository_id": "<B>", "path": "src/secret.rs" }` resolves B via `registry.workspace_by_repository_id` with no adoption check.
4. Alternatively: `read_file { "path": "/absolute/path/under/B/src/secret.rs" }` uses `known_workspaces()` (all startup roots) instead of `attached_workspaces()`.
5. File content from B is returned while `list_repositories` still shows `adopted: false`.

## Why It Might Matter

On a shared local `frigg serve` instance, any MCP client session can read and search repositories the operator registered at startup without adopting them. This breaks the documented session boundary and enables cross-repository access from a session that appears detached.

## Proof

**Cross-entry mismatch**

- `attached_workspaces_for_repository(Some(id))` at `workspace.rs:222-227` returns `registry.workspace_by_repository_id(&id)` without consulting `adopted_repository_ids`.
- `resolve_file_path` at `workspace_session.rs:906-910` branches to `known_workspaces()` when `requested.is_absolute() && params.repository_id.is_none()`, bypassing adoption entirely.
- Relative paths without `repository_id` correctly flow through `roots_for_repository` → `attached_workspaces_for_repository`, which enforces adoption when no explicit id is given and the adopted set is empty.
- `scoped_read_only_tool_execution_context` (`execution.rs:89`) uses the same helper, so search and navigation tools share the bypass.

## Counterevidence Checked

`resolve_file_path` still canonicalizes paths and enforces `starts_with(root_canonical)` per matched root, blocking `..` traversal within a matched workspace. `workspace_detach` correctly requires adoption (`workspace.rs:164-165`). Security tests exercise path traversal and tool annotations but use attached fixture servers, not detached sessions with explicit non-adopted `repository_id`.

## Suggested Next Step

Gate `attached_workspaces_for_repository` and the absolute-path branch in `resolve_file_path` on `adopted_repository_ids` (or an explicit operator policy flag), and add integration tests for detached sessions against startup-known repos.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. Confirmed both bypass paths. (1) `attached_workspaces_for_repository` (workspace.rs:222) now rejects an explicit `repository_id` (or session default) that is not in `adopted_repository_ids` with a "repository_id is not adopted for this session" error before resolving the workspace. The session default is only ever set on adopt and cleared on detach, so it always passes the gate. (2) `resolve_file_path` (workspace_session.rs:906) now scopes the absolute-path/no-`repository_id` branch to `attached_workspaces()` instead of `known_workspaces()`. `scoped_read_only_tool_execution_context` (execution.rs:89) shares the helper and is fixed transitively. Added regression test `read_file_rejects_non_adopted_repository_for_detached_session` (two startup repos, session adopts only A, asserts explicit-id and absolute-path reads of B are rejected while A stays readable). `cargo check --tests` and the new test pass.
- 2026-06-27: refinement + test-debt follow-up. The original gate string-compared the raw request `repository_id` against `adopted_repository_ids`, but a workspace is addressable by either its stable `repository_id` or its legacy `runtime_repository_id` (e.g. `repo-001`), while adoption is always recorded under the stable id. A session that adopted a repo under its stable id but addressed it by the legacy alias was therefore wrongly rejected. Reworked the gate to resolve the workspace first (`workspace_by_repository_id`, which matches either id form) and then check adoption against the resolved `workspace.repository_id`; the security boundary is unchanged because a non-adopted repository's canonical id is still absent from the adopted set (P0 regression test still passes). Separately, the counterevidence's claim that the fixtures were "attached" was wrong — the integration/lib fixtures never called `workspace_attach`, so the gate left ~130 read/search/navigation tests asserting the not-adopted error. Updated the shared fixtures to model an attached session: `security.rs`/`provenance.rs` adopt via the public `workspace_attach` tool, `tool_handlers.rs` read/nav builders best-effort attach (config builders stay detached for the attach-semantics and strict-provenance fixtures), and the lib `runtime_gate_tests` navigation/freshness tests call `adopt_workspace`. Whole suite green except a pre-existing-on-main `core_search_hybrid_strict_semantic_requires_startup_credentials` failure (unrelated to this report).

DEVANA-KEY: crates/cli/src/mcp/server/workspace.rs:222 | P0 | session-adoption-bypass-read-tools
DEVANA-SUMMARY: Status=fixed | P0 high crates/cli/src/mcp/server/workspace.rs:222 - Explicit repository_id and absolute-path read_file bypassed session adoption; both paths now gated on the adopted set (regression test added).