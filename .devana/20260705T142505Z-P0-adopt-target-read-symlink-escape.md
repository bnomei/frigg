DEVANA-FINDING: v1
DEVANA-STATE: fixed | P0 | high | security=yes
DEVANA-KEY: crates/cli/src/cli_runtime/commands/adopt/mod.rs:271 | adopt-target-read-symlink-escape

# frigg adopt reads symlinked adopt targets outside the workspace

## Finding

`classify_target_action` reads adopt target files with `fs::read_to_string(root.join(target.path()))` before any workspace write-path containment check. When the target path is a symlink pointing outside the repository, adopt classification merges against external file contents.

## Violated Invariant Or Contract

Adopt plan/apply must not read configuration content from outside the workspace root. Write paths already use `resolve_workspace_relative_write_path`; read paths should enforce the same boundary.

## Oracle

`resolve_entry_write_path` canonicalizes and rejects escapes (including symlink parent cases tested in adopt tests). `classify_target_action` and `apply_plan_entries` read via `root.join(target.path())` without resolving the symlink target against the root.

## Counterexample

1. Untrusted repo contains symlink `.cursor/mcp.json` → `/home/user/.cursor/mcp.json` on the host.
2. Operator runs `frigg adopt` in the repo.
3. `classify_target_action` reads the external MCP config via the symlink for classification/merge decisions.
4. External secrets/config content influence adopt output even if the subsequent write is blocked.

## Why It Might Matter

Running adopt on an untrusted repository can leak host configuration contents into adopt logic and generated plans without the operator intending to expose those files.

## Proof

**Dataflow trace:** symlinked adopt target path → `fs::read_to_string` follows symlink → external config bytes → classify/merge logic.

## Counterevidence Checked

Write-side symlink escape is tested and guarded. MCP attach path traversal issues are filed separately. Target-missing paths do not read.

## Suggested Next Step

Resolve adopt target paths canonically and reject targets whose resolved location is outside the workspace root before reading contents.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-05: open by Devana. Initial report written from static source inspection across all nine trails (`--all`).
- 2026-07-05: fixed by resolving existing adopt target read paths canonically and rejecting targets whose resolved location escapes the workspace before classification or apply reads. Validation: `cargo fmt --check`; `cargo test -p frigg symlink_target_escape`; `cargo test -p frigg cli_runtime::commands::adopt::tests`; `cargo test -p frigg --test adopt`.

DEVANA-KEY: crates/cli/src/cli_runtime/commands/adopt/mod.rs:271 | adopt-target-read-symlink-escape
DEVANA-SUMMARY: fixed | P0 | high | frigg adopt now rejects symlinked target files whose canonical read path escapes the workspace before their contents can influence planning or apply logic.
