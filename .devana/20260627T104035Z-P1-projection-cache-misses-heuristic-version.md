DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: wontfix
Location: crates/cli/src/searcher/types.rs:254 | Slug: projection-cache-misses-heuristic-version

# In-process projection caches omit heuristic_version dimension

## Finding

`HybridPathWitnessProjectionCacheKey` keys in-memory projection caches by
`(repository_id, root, snapshot_id)` only. DB loads for the same families gate on
matching `heuristic_version` and `row_count` before trusting stored rows
(`projection_service/loaders/families.rs`). A cache hit returns immediately without
re-checking version, so a reindex that refreshes retrieval projections under the
same `snapshot_id` (or before `invalidate_repository` runs) can serve pre-refresh
witness, anchor-sketch, surface-term, or adjacency data to hybrid search and graph
navigation.

## Violated Invariant Or Contract

Cached retrieval projections must match the authoritative DB head for
`(repository_id, snapshot_id, family, heuristic_version)`; cache keys must include
every dimension the DB loader uses to accept rows.

## Oracle

DB loader path for `path_witness` (~112–137) filters
`head.heuristic_version == PATH_WITNESS_PROJECTION_HEURISTIC_VERSION` before
decode/insert. Cache lookup (~73–80) returns on key match with no version check.
`projected_graph_adjacency_cache` (~50–77 in `query.rs`) returns cached adjacency
without comparing the freshly loaded `relations` slice.

## Counterexample

1. Process loads projections for `(repo, root, snapshot-S)` at heuristic version V1;
   caches populated.
2. Reindex refreshes retrieval projection bundle for the same snapshot-S at version
   V2 (code bump or repair) and commits to SQLite.
3. `invalidate_repository` is skipped or delayed (e.g. stale watch completion per
   sibling report, or CLI reindex without MCP invalidation callback).
4. Next `search_hybrid` call hits in-process cache key `(repo, root, snapshot-S)` →
   returns V1 witnesses/anchors while DB holds V2 rows.

## Why It Might Matter

Hybrid ranking, path-witness recall, and graph-channel navigation can return
wrong excerpts, adjacency, or witness lines until process restart or explicit
invalidation, despite a successful reindex.

## Proof

**Dataflow trace:** DB write (new heuristic_version) → cache get by
`(repo, root, snapshot_id)` only → hybrid search sink uses stale projections.

**Contract mismatch:** DB head validation includes `heuristic_version`; cache key
does not.

## Counterevidence Checked

- `repository_cache_invalidation_callback` calls
  `searcher_projection_store_service.invalidate_repository` on normal watch-success
  completions; gap remains when invalidation is skipped or bypassed.
- `stale_or_missing_retrieval_projection_families_for_repository_snapshot` can
  detect DB skew but does not run on cache hits.
- Per-family SQLite replace transactions are atomic; the bug is read-side cache
  dimension omission, not partial DB writes.

## Suggested Next Step

Add `heuristic_version` (and optionally `row_count`) to
`HybridPathWitnessProjectionCacheKey`, or re-validate head metadata on cache hit.
Invalidate adjacency cache when upstream `path_relation` family reloads.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report written from static source inspection
  across `cache-persistence` and `dataflow-boundaries` trails.
- 2026-06-27: wontfix (false positive). The counterexample is not constructible:
  * `snapshot_id` is a BLAKE3 content hash (`deterministic_snapshot_id`,
    indexer/manifest.rs:433) over repository_id + each file's path/size/mtime/blake3
    content hash. It IS the content identity, so different content always yields a
    different `snapshot_id`; the same `snapshot_id` always means identical content.
  * Projection rows are a deterministic function of (candidate path set,
    heuristic_version) — e.g. `build_path_witness_projection_records_from_paths`
    takes only paths.
  * All `*_HEURISTIC_VERSION` values are compile-time `const` (path_witness=1,
    anchor_sketch=2, etc.), so a process has exactly one heuristic_version for its
    lifetime; the caches (`projection_service.rs` `RwLock<BTreeMap>`) are in-process
    and do not survive a restart.
  Consequently a given `(repository_id, root, snapshot_id)` always maps to identical
  projection rows within any one process, so a cache hit cannot serve stale data.
  The report's "same snapshot-S at version V2 (code bump)" requires a new binary →
  new process → empty cache; a "repair" reuses the same const heuristic_version and
  same content, producing identical rows. The DB-side `heuristic_version`/`row_count`
  gating exists because the *DB* persists across binary upgrades — the ephemeral
  in-process cache does not need that dimension. The invalidation-skipped angle
  reduces to a stale *unused* entry under an old snapshot_id (memory retention, not
  correctness, since queries key by the current snapshot_id); the genuine
  invalidation gap was the sibling report
  `watch-epoch-skips-cache-invalidation` (now fixed). The adjacency cache (query.rs)
  is likewise keyed by snapshot_id and consistent with its same-snapshot
  `relations`. No code change.

DEVANA-KEY: crates/cli/src/searcher/types.rs:254 | P1 | projection-cache-misses-heuristic-version
DEVANA-SUMMARY: Status=wontfix | P1 high crates/cli/src/searcher/types.rs:254 - False positive: snapshot_id is a content hash and heuristic_version is a per-process compile-time const, so the in-process projection caches (keyed by repo/root/snapshot_id) can never serve stale rows within a process; the DB-side version gating only matters because the DB persists across binary upgrades.