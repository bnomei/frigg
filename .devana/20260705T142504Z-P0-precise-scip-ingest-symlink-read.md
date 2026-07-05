DEVANA-FINDING: v1
DEVANA-STATE: fixed | P0 | high | security=yes
DEVANA-KEY: crates/cli/src/mcp/server/precise_graph/ingest.rs:285 | precise-scip-ingest-symlink-read

# Precise SCIP ingest reads symlink artifact paths without containment

## Finding

Precise navigation ingests SCIP artifacts by calling `fs::read` on discovered artifact paths without verifying that the resolved file stays inside the adopted workspace root. Symlinked `.frigg/scip` entries or symlink artifacts can load out-of-tree bytes into the precise graph returned to MCP clients.

## Violated Invariant Or Contract

Precise ingest must not read filesystem objects outside the adopted repository root, consistent with other repository-scoped read surfaces.

## Oracle

`collect_scip_artifact_digests` walks candidate directories and records `entry.path()` verbatim. Ingest later uses `fs::read(&artifact_digest.path)` with no canonical containment check. `workspace_precise_excludes_path` uses `strip_prefix(root)` only; paths outside the root are treated as not excluded rather than rejected.

## Counterexample

1. Adopt repo `/repo` with `.frigg/scip/malicious.scip` symlink pointing to `/outside/secret.scip`.
2. `find_references` triggers precise ingest for the repository.
3. Artifact discovery records the symlink path; ingest calls `fs::read` on it.
4. Out-of-tree SCIP bytes are parsed into precise graph data surfaced in navigation output.

## Why It Might Matter

A malicious repository can expose host file contents or attacker-controlled graph metadata through precise navigation without passing `read_file` containment checks.

## Proof

**Dataflow trace:** symlink artifact under repo → digest collection keeps path → `fs::read` follows symlink → precise graph ingest → MCP navigation response.

## Counterevidence Checked

`read_file` and explore enforce canonical containment. Filed search symlink issues cover lexical search reads, not SCIP artifact ingest. Artifact budget limits constrain size, not location.

## Suggested Next Step

Canonicalize artifact paths and reject ingest when the resolved target is outside the workspace root; skip or fail symlinked artifacts that escape the root.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-05: open by Devana. Initial report written from static source inspection across all nine trails (`--all`).
- 2026-07-05: fixed by canonicalizing discovered SCIP artifact paths against the workspace root and filtering escaping symlink targets before ingest/signature construction. Added `precise_artifact_discovery_rejects_symlink_escape`; validation: `cargo fmt --check`; `cargo test -p frigg mcp::server::runtime_gate_tests::precise_generation::precise_artifact_discovery_rejects_symlink_escape`.

DEVANA-KEY: crates/cli/src/mcp/server/precise_graph/ingest.rs:285 | precise-scip-ingest-symlink-read
DEVANA-SUMMARY: fixed | P0 | high | Precise SCIP artifact discovery now filters symlinked artifacts whose canonical target escapes the workspace root before ingest can read them.
