DEVANA-FINDING: v1
DEVANA-STATE: fixed | P0 | high | security=yes
DEVANA-KEY: crates/cli/src/mcp/server/presentation.rs:300 | search-text-symlink-escape

# search_text context excerpts bypass symlink workspace boundary

## Finding

`read_file` rejects symlinked paths whose canonical target lies outside the adopted workspace root, but `search_text` with `context_lines > 0` reads match excerpts through `workspace.root.join(path)` without canonical containment checks. A symlink file indexed under a repo-relative path can leak bytes from host files outside the workspace.

## Violated Invariant Or Contract

Read paths that return file bytes to MCP clients must enforce the same workspace-root containment contract tested for `read_file` and `explore` in `crates/cli/tests/security.rs`.

## Oracle

`security_read_file_rejects_symlink_escape_inside_workspace` and `security_extended_explore_enforces_workspace_boundary` show the intended boundary. `resolve_file_path` canonicalizes and rejects escapes; `expand_text_match_excerpt` does not.

## Counterexample

1. Adopt workspace `/repo` containing `src/leak.txt` → symlink to `/outside/secret.txt` with content `SECRET_TOKEN`.
2. Manifest indexes `src/leak.txt` as an in-repo file (`follow_symlinks: false` still records symlink file entries).
3. `search_text({ query: "SECRET", context_lines: 2 })` matches `src/leak.txt`.
4. `expand_text_match_excerpt` joins `/repo/src/leak.txt` and calls `file_content_snapshot_for_workspace`, which `fs::read`s through the symlink.
5. Response excerpt contains `SECRET_TOKEN` instead of `access_denied`.

## Why It Might Matter

MCP clients receive host file contents outside authorized workspace roots from a tool surface advertised as read-only and boundary-checked. Same join-without-canonicalize pattern also appears in corpus-driven reads (`symbol_index.rs`, `search_tools/inspect.rs`) for indexed symlink paths.

## Proof

Dataflow trace: indexed symlink path in search hit → `expand_text_match_excerpt` (`presentation.rs:300-307`) → `file_content_snapshot_for_workspace` (`runtime_cache.rs:131`) → unbounded `fs::read` without `resolve_file_path` canonical containment → excerpt returned to client.

## Counterevidence Checked

- `read_file`/`explore` use `resolve_file_path` canonical checks; this path bypasses them.
- Manifest walk does not follow symlink directories, but symlink file nodes remain indexed paths.
- SCIP ingest path normalization does not apply to heuristic search excerpt reads.

## Suggested Next Step

Route all MCP file-byte reads through `resolve_file_path` or an equivalent canonical root guard before `fs::read`, and add a `security_search_text_rejects_symlink_escape` integration test mirroring the existing `read_file` symlink tests.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-05: open by Devana. Initial report written from static source inspection.
- 2026-07-05: fixed by routing `search_text` context excerpt reads through `resolve_file_path` before reading bytes. Added a direct presenter regression that injects a search hit for the symlinked repo path and verifies context excerpt expansion rejects the canonical escape with `access_denied`, plus an integration smoke test showing public `search_text` does not expose the symlinked target. Validation: `cargo fmt --check`; `cargo test -p frigg mcp::server::presentation::tests::search_text_excerpt_rejects_symlink_escape_hit`; `cargo test -p frigg --test security security_search_text_rejects_symlink_escape`. Review: `OVERALL: GREEN` from read-only reviewer.

DEVANA-KEY: crates/cli/src/mcp/server/presentation.rs:300 | search-text-symlink-escape
DEVANA-SUMMARY: fixed | P0 | high | search_text context excerpt expansion now rejects symlinked indexed paths whose canonical target escapes the workspace root.
