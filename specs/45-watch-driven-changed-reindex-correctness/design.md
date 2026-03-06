# Design — 45-watch-driven-changed-reindex-correctness

## Scope
- `crates/cli/src/indexer/`
- `crates/cli/src/searcher/`
- `crates/cli/src/storage/`
- `crates/cli/src/watch.rs`
- `crates/cli/tests/`
- `README.md`
- `specs/45-watch-driven-changed-reindex-correctness/`
- `specs/index.md`

## Problem statement
Adding a built-in watcher only helps if the background refresh path preserves the same safety and freshness guarantees as the explicit CLI-driven `reindex --changed` flow. The server must never serve half-committed data, reintroduce ignored corpus noise, or let docs disappear from the active corpus because trigger handling and query serving drift apart.

## Correctness model
This spec keeps the current indexing contract intact:

1. A watch-triggered refresh calls `reindex_repository(..., ReindexMode::ChangedOnly)`.
2. Changed-only reindex still rebuilds the current manifest for the whole repository root.
3. If nothing changed, it keeps reusing the previous snapshot id.
4. If anything changed, it writes a new full manifest snapshot for that root.
5. Semantic indexing still advances unchanged rows forward to the new snapshot and replaces changed/deleted paths.
6. Queries continue reading from the latest successful committed snapshot until the new snapshot is fully available.

The watcher is therefore a scheduler, not a new indexing engine.

## Freshness and serving invariants

### Latest-successful snapshot serving
Background watch-triggered refreshes must not expose partial state. Search and MCP reads should continue using the last successful snapshot until the new changed-only run commits. Existing manifest-freshness validation remains the guard against stale persisted snapshot reuse.

### File lifecycle cases
Correctness must hold for:
- file modification
- file deletion
- file rename or move within a root

After commit of the replacement snapshot:
- manifest-backed candidate discovery must reflect the new file set
- semantic rows for superseded paths must no longer contribute results
- stale snapshot fallback behavior must stay unchanged

### Docs visibility and ignored noise
The repo already needs explicit `docs/` visibility even when Git ignore rules would hide it. Watch-triggered refreshes must preserve that rule. The watcher and the ingestion/search layers must also keep excluding `.git/`, `.frigg/`, and `target/` from active corpus use and from noisy self-trigger scheduling.

## Multi-root behavior
- Each workspace root is diffed only against its own latest persisted snapshot.
- Roots share one serialized background reindex executor.
- A failure in one root does not block query serving or later refresh attempts for other roots.
- A root that fails remains dirty and retries later, but other clean or newly dirty roots continue through the same serialized executor.

## Deferred alternatives
This spec explicitly excludes a future dirty-path manifest patcher that would carry forward prior manifest rows and patch only changed/deleted paths. That can be a later spec once the integrated watcher proves valuable and the current changed-only scheduler path is stable.

## Risks
- Background refreshes can look correct in unit tests but still regress on delete/rename edges if manifest and semantic advancement are not validated together.
- Trigger filtering and corpus filtering can drift apart if watcher ignores and ingestion ignores are not asserted together.
- Multi-root fairness can regress if one repeatedly failing root monopolizes the serialized executor.

## Validation strategy
- Regression tests for modify/delete/rename behavior after watcher-triggered changed-only refreshes.
- Regression tests proving ignored noise roots do not enter the active corpus or trigger loops.
- Regression tests proving gitignored `docs/` remain searchable after watch-triggered refreshes.
- End-to-end validation where a watch-triggered change is followed by the strict hybrid playbook suite.
