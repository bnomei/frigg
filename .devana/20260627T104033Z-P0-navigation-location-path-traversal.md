DEVANA-FINDING: v1
Priority: P0 | Confidence: high | Security-sensitive: yes | Status: fixed
Location: crates/cli/src/mcp/server/navigation_resolution.rs:589 | Slug: navigation-location-path-traversal

# Location-based navigation reads files outside adopted workspace roots

## Finding

`navigation_symbol_query_token_from_location` resolves the caller-supplied `path`
by trimming `./` and backslashes, then joins it to the corpus root and opens the
file with `fs::read_to_string` or `read_source_line_for_navigation`. Unlike
`resolve_file_path`, it never canonicalizes the candidate or checks
`starts_with(root_canonical)`. Relative paths containing `..` and absolute paths
therefore escape the adopted repository and read arbitrary host files during
`go_to_definition`, `find_references`, and call-hierarchy tools that supply
`path`, `line`, and `column`.

## Violated Invariant Or Contract

Navigation location reads must stay within the adopted workspace root, matching the
containment contract enforced for `read_file`, `explore`, and `document_symbols`
via `resolve_file_path` and the security tests in `crates/cli/tests/security.rs`.

## Oracle

`resolve_file_path` rejects paths outside workspace roots with `access_denied`
after canonicalization (`workspace_session.rs` ~945–991). Security tests cover
`read_file` and `explore` traversal but not navigation location reads.
`requested_location_path_for_corpus` only normalizes `./` prefixes and does not
reject `..` segments (contrast `normalize_scip_document_relative_path` in
`scip_support.rs`).

## Counterexample

1. Client adopts repository R at `/workspace/repo`.
2. Client calls `go_to_definition` with `repository_id=R`, `path="../outside.txt"`,
   `line=1`, `column=1`.
3. `requested_location_path_for_corpus` returns `"../outside.txt"`.
4. `corpus.root.join("../outside.txt")` resolves to `/workspace/outside.txt`.
5. `read_source_line_for_navigation` opens and reads that file; token extraction
   returns content to the caller in navigation metadata.

Absolute `path="/etc/passwd"` drops the repo prefix because `Path::join` replaces
the left operand when the right-hand path is absolute.

## Why It Might Matter

An authenticated MCP client (stdio or HTTP bearer) that has legitimately adopted
a repository can read partial contents of files outside that repository during
navigation. This is a confused-deputy path traversal on a read-only tool surface.

## Proof

**Dataflow trace:** MCP `path` param → `navigation_symbol_query_token_from_location`
→ `requested_location_path_for_corpus` (no containment) → `corpus.root.join` →
`fs::read_to_string` / `File::open` → navigation response metadata.

**Cross-entry mismatch:** `read_file` uses `resolve_file_path` containment;
location-based navigation does not.

Related path: `navigation_metadata.rs::generated_follow_up_structural_for_anchor`
uses the same join-without-containment pattern for follow-up structural reads.

## Counterevidence Checked

- Session adoption gates corpora collection; bypass occurs after adoption, not
  before it.
- Manifest indexing rejects `..` in relative paths for indexed symbols, but
  location reads run on the escaped absolute path regardless of corpus membership.
- SCIP ingest path escape was fixed separately; this is caller-supplied navigation
  `path`, not SCIP document ingest.
- HTTP bearer/host allowlist protects transport only, not per-path containment.

## Suggested Next Step

Reuse `resolve_file_path` containment (or a shared helper) before any navigation
location file read; add a security regression test mirroring `security.rs` traversal
cases for `go_to_definition` / `find_references` with `path`+`column`.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report written from static source inspection
  across `outside-in-entrypoints`, `boundaries-oracles`, `dataflow-boundaries`, and
  `security-boundaries` trails.
- 2026-06-27: fixed. Confirmed `navigation_symbol_query_token_from_location`
  (navigation_resolution.rs) joined the caller `path` to `corpus.root` and read the
  file via `fs::read_to_string`/`read_source_line_for_navigation` with no
  containment, so `../` and absolute paths escaped the root. Line 526
  (`resolve_navigation_symbol_query_from_location`) was a HashMap lookup of indexed
  symbols only — safe. Added shared `navigation_path_within_root` helper
  (canonicalize candidate + root, `starts_with` check, mirroring `resolve_file_path`)
  and applied it before the token-extraction reads and before
  `generated_follow_up_structural_for_anchor` (navigation_metadata.rs) follow-up
  reads. Added security regression tests
  `security_go_to_definition_rejects_relative_path_traversal_outside_workspace` and
  `security_go_to_definition_rejects_absolute_path_outside_workspace` (the repo
  defines a colliding `outside_secret_token` symbol so the tests would fail if the
  out-of-tree token were still extracted). Full `--test security` suite green.

DEVANA-KEY: crates/cli/src/mcp/server/navigation_resolution.rs:589 | P0 | navigation-location-path-traversal
DEVANA-SUMMARY: Status=fixed | P0 high crates/cli/src/mcp/server/navigation_resolution.rs:589 - Location-based navigation joined caller `path` without workspace containment and could read files outside adopted roots; fixed with a canonicalizing containment guard plus regression tests.