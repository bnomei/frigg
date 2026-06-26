DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/indexer/reindex/execution.rs:98 | Slug: projection-version-repair-skip

# Changed-Only Reindex Skips Stale Projection Versions

## Finding

When changed-only reindex reuses an existing snapshot, retrieval projections are rebuilt only if families are missing, not if existing heads have stale heuristic versions.

## Violated Invariant Or Contract

A snapshot is projection-complete for the current runtime only when each required family exists with the current `heuristic_version`, because consumers reject version-mismatched heads.

## Oracle

Projection loaders filter heads by current heuristic version before using persisted rows.

## Counterexample

After a runtime upgrade, a repository has snapshot `S` with all projection families present, but `path_relation` has an old heuristic version. A changed-only reindex with no file changes reuses `S`, sees no missing families, and skips rebuilding. Search later rejects the stale persisted head and falls back to lower-fidelity runtime projection.

## Why It Might Matter

The durable index can remain stale across upgrades even after reindex, causing search quality or relation evidence to silently degrade.

## Proof

Contract mismatch: the reuse branch at `crates/cli/src/indexer/reindex/execution.rs:98` calls `missing_retrieval_projection_families_for_repository_snapshot`. That query selects only `family` at `crates/cli/src/storage/projection_store/bundle.rs:519`. Consumers require matching versions, for example path relation filters `head.heuristic_version == PATH_RELATION_PROJECTION_HEURISTIC_VERSION` at `crates/cli/src/searcher/projection_service/loaders/families.rs:528`.

## Counterevidence Checked

New snapshots rebuild projections, and missing families are repaired. Loaders protect readers from stale rows, but the repair predicate never treats stale-version heads as repair work.

## Suggested Next Step

Change the reindex repair predicate to compare required families and current heuristic versions, then rebuild any family with a missing or stale head.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. Confirmed the bug: the reuse branch's repair predicate only checked family presence (`missing_retrieval_projection_families_for_repository_snapshot` selects `family` alone), so a head with a stale `heuristic_version` was treated as repair-complete while consumers (e.g. path-relation loader) reject the version-mismatched head and silently fall back. Fix: added a single source of truth `required_retrieval_projection_versions()` in `crates/cli/src/searcher/retrieval_projection/mod.rs` returning the 7 `(family, heuristic_version)` pairs (re-exported from `searcher::mod`), and a new storage predicate `stale_or_missing_retrieval_projection_families_for_repository_snapshot` in `crates/cli/src/storage/projection_store/bundle.rs` that reads `SELECT family, heuristic_version FROM retrieval_projection_head` and flags any family whose present version differs from (or is absent vs) the expected version. Wired into both the changed-only reindex reuse branch (`crates/cli/src/indexer/reindex/execution.rs:98`) and the watch startup freshness check (`crates/cli/src/watch/repository.rs`). Regression test `stale_or_missing_retrieval_projection_families_flags_version_mismatch` in `crates/cli/src/storage/tests/manifest.rs` seeds a v1 head and asserts: expected v1 → empty, expected v2 → ["path_anchor_sketch"], missing path_witness → ["path_witness"]. `cargo test -p frigg retrieval_projection` (8 passed) and `cargo test -p frigg watch::` (20 passed) green.

DEVANA-KEY: crates/cli/src/indexer/reindex/execution.rs:98 | P2 | projection-version-repair-skip
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/indexer/reindex/execution.rs:98 - Changed-only reindex skips rebuilding projection families whose heads exist but have stale heuristic versions rejected by consumers.
