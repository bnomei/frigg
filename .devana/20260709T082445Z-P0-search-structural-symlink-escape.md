DEVANA-FINDING: v1
DEVANA-STATE: fixed | P0 | high | security=yes
DEVANA-KEY: crates/cli/src/mcp/server/search_tools/inspect.rs:448 | search-structural-symlink-escape

# search_structural re-reads corpus paths without symlink containment

## Finding

`search_structural` iterates `RepositorySymbolCorpus.source_paths` and opens each path with `fs::read_to_string(source_path)` without `navigation_path_within_root` (or equivalent canonicalize-under-root). The returned tool path is a lexical `relative_display_path` of the stored corpus path, so a symlink that escapes the adopted root is still labeled as an in-repo path while excerpts/captures contain out-of-tree bytes.

The same unguarded pattern remains on related hot paths: PHP route heuristic in `go_to_definition` (`fs::read_to_string(path)` over `corpus.source_paths`), call-hierarchy implementation source cache, and symbol-corpus PHP/Rust evidence re-reads. `find_references` was hardened for this class; structural/corpus re-readers were not.

## Violated Invariant Or Contract

Any MCP read of a repository path must resolve under the adopted root after symlink resolution, matching `resolve_file_path`, `navigation_path_within_root`, and the fixed heuristic-references guard.

## Oracle

Neighboring fixed paths: `navigation_tools/references.rs` uses `navigation_path_within_root` before source reads; `candidate_universe` drops successful outside-symlink candidates; `read_file` uses `resolve_file_path` canonicalize-under-root. Implicit safety expectation for all MCP source surfaces.

## Counterexample

1. Adopt repo `/repo` where `src/auth.rs` is indexed in `source_paths`.
2. Replace with symlink: `ln -sfn /path/outside/secret.rs /repo/src/auth.rs`.
3. Call `search_structural` with a Tree-sitter query that matches content in the outside file.
4. Response path remains `src/auth.rs` while `excerpt`/captures include outside file content.

Because `search_structural` re-reads disk on every call from cached `source_paths`, the symlink swap is immediately exploitable without waiting for reindex.

## Why It Might Matter

A malicious or compromised repository can exfiltrate host file contents through structural search and related navigation helpers even when `read_file` and `find_references` reject the same symlink path. Class: security / private-data exposure.

## Proof

**Dataflow trace:** adopted repo symlink path stored in `corpus.source_paths` → `search_structural_impl` loops source paths (`inspect.rs` ~428–460) → `fs::read_to_string(source_path)` follows symlink → structural match excerpt/capture returned under lexical display path → MCP client receives out-of-tree bytes.

**Cross-entry mismatch:** `find_references` guards with `navigation_path_within_root`; `search_structural` / route heuristic do not.

## Counterevidence Checked

- `navigation-heuristic-symlink-read` (fixed) only covers `find_references` candidate loads, not `search_structural`.
- `search-text-symlink-escape` / `search-symlink-escape` cover lexical search presentation/candidates, not structural corpus re-read.
- `precise-scip-ingest-symlink-read` covers SCIP artifacts, not source corpus paths.
- `inspect_syntax_tree` / user-supplied `path` go through `resolve_file_path` (safe); the bug is the corpus-driven bulk re-read path.
- Display-path strip_prefix does not re-check canonical containment.

Strongest false-positive reason: “index only stores in-root paths.” Ruled out because (1) a post-index symlink swap still re-reads live disk, and (2) index-time symlink following can also embed outside content into later tool responses without a containment check at read time.

## Suggested Next Step

Before every corpus/source open for structural and related navigation re-reads, require `navigation_path_within_root(root, path)` (or shared helper used by search candidates) and skip on failure.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-09: open by Devana. Initial report written from static source inspection during exhaustive `--all` hunt.
- 2026-07-09: fixed by requiring `navigation_path_within_root` before corpus/source re-reads in search_structural, go_to route heuristic, call-hierarchy body cache, symbol-corpus evidence re-read, and precise occurrence source load. Regression: `navigation_path_within_root_rejects_symlink_escape`.

DEVANA-KEY: crates/cli/src/mcp/server/search_tools/inspect.rs:448 | search-structural-symlink-escape
DEVANA-SUMMARY: fixed | P0 | high | search_structural re-reads corpus source paths without symlink root containment, so escaping symlinks can exfiltrate host file contents labeled as in-repo paths.
