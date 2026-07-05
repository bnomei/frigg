DEVANA-FINDING: v1
DEVANA-STATE: fixed | P0 | high | security=yes
DEVANA-KEY: crates/cli/src/mcp/server/navigation_tools/references.rs:203 | navigation-heuristic-symlink-read

# Heuristic find_references reads symlink targets without root containment

## Finding

Heuristic `find_references` loads candidate source files with `fs::read_to_string(path)` on corpus paths without canonical containment checks. A symlinked source path inside an adopted repository can cause out-of-tree file bytes to be ingested into reference resolution and returned to MCP clients.

## Violated Invariant Or Contract

Navigation source reads must stay within adopted workspace roots, matching the containment guarantees enforced for `read_file` and location-based navigation.

## Oracle

`load_heuristic_references` reads `candidate_source_paths` directly. `heuristic_reference_candidate_paths` can include absolute paths from `target_corpus.source_paths`. Tests in `crates/cli/tests/security.rs` enforce symlink containment for `read_file`, but heuristic navigation bypasses `resolve_file_path` / `navigation_path_within_root` before reading source bytes.

## Counterexample

1. Adopt repo `/repo` containing symlink `src/link.rs` → `/outside/secret.rs` with sensitive content.
2. Manifest/index lists the symlink path as a source file.
3. Call `find_references` on a symbol that pulls `src/link.rs` into `candidate_source_paths`.
4. `fs::read_to_string` follows the symlink and ingests `/outside/secret.rs` into heuristic resolver output surfaced in tool results.

## Why It Might Matter

A malicious or compromised repository can exfiltrate host file contents through navigation tools even when `read_file` rejects the same symlink path.

## Proof

**Dataflow trace:** adopted repo symlink path → heuristic candidate selection → `fs::read_to_string` without canonical root check → reference resolver ingests external bytes → MCP response.

## Counterevidence Checked

`search_text` / `search_hybrid` ripgrep symlink issues are filed separately. Precise navigation uses SCIP artifacts, not this heuristic read loop. Metadata/size budget checks do not validate canonical target location.

## Suggested Next Step

Canonicalize each candidate source path and reject reads whose resolved target is outside the corpus root, mirroring `read_file` containment.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-05: open by Devana. Initial report written from static source inspection across all nine trails (`--all`).
- 2026-07-05: fixed by rejecting heuristic reference source candidates whose canonical target escapes the repository root before metadata or source reads. Validation: `cargo fmt --check`; `cargo test -p frigg mcp::server::navigation_tools::references::tests::load_heuristic_references_rejects_symlink_escape_source`.

DEVANA-KEY: crates/cli/src/mcp/server/navigation_tools/references.rs:203 | navigation-heuristic-symlink-read
DEVANA-SUMMARY: fixed | P0 | high | Heuristic find_references now rejects symlinked source paths whose canonical targets escape the workspace before source bytes can enter navigation results.
