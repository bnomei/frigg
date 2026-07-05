DEVANA-FINDING: v1
DEVANA-STATE: fixed | P0 | high | security=yes
DEVANA-KEY: crates/cli/src/searcher/scan_engine.rs:270 | search-symlink-escape

# search_text reads symlink targets outside workspace roots

## Finding

`read_file` and `explore` reject paths whose canonical target lies outside adopted workspace roots, but the lexical search pipeline reads file bytes via `fs::read` on `workspace.root.join(relative_path)` without an equivalent containment check. A symlink inside the workspace that points outside the root can leak out-of-tree file content through `search_text` match excerpts and native scan reads.

## Violated Invariant Or Contract

Repository read tools must not return bytes from paths outside adopted workspace roots. `crates/cli/tests/security.rs` enforces this for `read_file` (relative `..`, absolute outside, symlink escape) but the search path bypasses the same guard.

## Oracle

Integration tests in `crates/cli/tests/security.rs` (`security_read_file_rejects_symlink_escape_outside_workspace`, lines 608–652) document the expected boundary for read surfaces. Search uses a separate read path in `scan_engine.rs` and `presentation.rs`.

## Counterexample

1. Adopt workspace at `/repo`.
2. Create `/outside/secret.txt` containing `OUTSIDE_SECRET`.
3. Inside repo: `ln -s /outside/secret.txt src/leak.txt`.
4. Call `search_text({ query: "OUTSIDE_SECRET", repository_id: "<repo>", context_lines: 1 })`.
5. `read_file({ path: "src/leak.txt" })` returns `access_denied`; `search_text` returns a match whose excerpt includes `OUTSIDE_SECRET`.

## Why It Might Matter

Any MCP client with access to the local server (loopback HTTP is unauthenticated by design) could retrieve secrets or source from paths the operator believed were blocked by workspace containment.

## Proof

**Dataflow trace:** MCP `search_text` → `present_search_text_response` / native scan → `expand_text_match_excerpt` (`presentation.rs:300-301`) joins `workspace.root` + `found.path` without canonical containment → `file_content_snapshot_for_workspace` → `fs::read` (`runtime_cache.rs:114`, `scan_engine.rs:270`) follows the symlink to `/outside/secret.txt`.

**Cross-entry mismatch:** `resolve_file_path` (`workspace_session.rs:1128-1132`) canonicalizes and applies component-wise `starts_with`; search pipeline does not.

## Counterevidence Checked

- Manifest walk defaults `follow_symlinks: false` (`indexer/mod.rs:123`), so symlinks are indexed as in-repo paths, not followed at walk time; reads at query time still follow links.
- `hard_excluded_runtime_path` and ignore rules do not block arbitrary symlinks.
- No `security_search_text_rejects_symlink_escape` test exists.

Strongest false-positive reason: symlinked paths might be excluded from the candidate universe. Checked `scan_engine.rs:55-56` — candidates use `candidate.absolute_path` from manifest/live walk; symlink entries are included as normal files.

## Suggested Next Step

Route all search-time file reads through `resolve_file_path` (or a shared canonical `starts_with` helper) before `fs::read`. Add a security integration test mirroring `security_read_file_rejects_symlink_escape_outside_workspace` for `search_text`.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-05: open by Devana. Initial report written from static source inspection across all nine trails.
- 2026-07-05: fixed by filtering lexical search candidates whose canonical target escapes the repository root before either native scan or ripgrep sees them. Added `contained_search_candidate_files_drops_symlink_escape`; validation: `cargo fmt --check`; `cargo test -p frigg searcher::candidate_universe::tests::contained_search_candidate_files_drops_symlink_escape`; `cargo test -p frigg --test security security_search_text_rejects_symlink_escape`.

DEVANA-KEY: crates/cli/src/searcher/scan_engine.rs:270 | search-symlink-escape
DEVANA-SUMMARY: fixed | P0 | high | Lexical search candidates whose canonical target escapes the repository root are filtered before native scan or ripgrep can read them.
