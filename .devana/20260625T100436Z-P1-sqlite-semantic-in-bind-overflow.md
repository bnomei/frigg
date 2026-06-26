DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: invalid
Location: crates/cli/src/storage/semantic_store_read.rs:685 | Slug: sqlite-semantic-in-bind-overflow

# Semantic vector membership queries exceed SQLite bind-variable limit under default hybrid search

## Finding

After KNN vector scan, `load_allowed_semantic_chunk_ids_for_snapshot_on_connection` builds a single dynamic `IN (?,?,…)` clause with `5 + N` bind parameters and no batching. Default hybrid search drives `scan_limit` up to 4096, far above SQLite's default `SQLITE_MAX_VARIABLE_NUMBER` of 999, causing prepare/execute failure and semantic channel outage on populated indexes.

## Violated Invariant Or Contract

Hybrid semantic search must complete vector top-k plus snapshot membership filtering without internal SQL failure. SQLite statements must stay within host parameter limits.

## Oracle

SQLite default variable ceiling is 999. `SQLITE_VEC_MAX_KNN_LIMIT` and hybrid pooling constants (`semantic.rs:237-242`, `frigg_config.rs:13`) routinely produce scan sizes ≥ 1200 under default `max_search_results = 200`.

## Counterexample

1. Index a repository with ≥1000 semantic chunks.
2. Run `search_hybrid` with semantic enabled and default config.
3. `semantic_candidate_limit = max(200×6, 24) = 1200`; `semantic_vector_query_limit = min(1200×4, 4096) = 4096`.
4. `scan_limit = min(capped_limit × 4, 4096)` at `semantic_store_read.rs:596-597` yields up to 4096 KNN rows.
5. Membership query uses `5 + 4096 ≈ 4101` bind params (`semantic_store_read.rs:685-717`).
6. `conn.prepare` fails with "too many SQL variables"; surfaced as `FriggError::Internal`.

## Why It Might Matter

Semantic and hybrid search fail deterministically on medium-to-large codebases with default settings, degrading the primary retrieval path to lexical-only or total tool failure rather than returning fewer results.

## Proof

**Counterexample value**

`N = 4096` chunk IDs from KNN → `5 + N = 4101` bind parameters vs SQLite limit 999.

**Control-flow trace**

`search_semantic_channel_hits` → `load_semantic_vector_topk_*` → `load_allowed_semantic_chunk_ids_*` monolithic `IN` with no partition loop.

## Counterevidence Checked

`delete_vector_rows_for_chunk_ids` uses per-row deletes (safe). Tests seed only handfuls of chunks (`searcher/tests/semantic.rs`). `limit == 0` early-return does not apply to normal hybrid queries. Preview/payload loaders at `semantic_store_read.rs:735-884` also build unbounded `IN` lists.

## Suggested Next Step

Batch membership, preview, and payload queries into chunks of ≤990 IDs, or cap KNN scan size to stay under the variable ceiling before building `IN` clauses.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: invalid. The finding assumes SQLite's variable ceiling is 999, but that default only held before SQLite 3.32.0 (2020). This project pins `rusqlite = { version = "0.40.0", features = ["bundled"] }` (workspace Cargo.toml:44; both crates/cli deps use `rusqlite.workspace = true`), so it always links the bundled SQLite — currently 3.51.3 (libsqlite3-sys 0.38) — whose default `SQLITE_MAX_VARIABLE_NUMBER` is 32766. The bind count is bounded by `SQLITE_VEC_MAX_KNN_LIMIT = 4096` (storage/mod.rs:68): the membership query binds at most `5 + 4096 = 4101` params and the preview/payload loaders at most `2 + ~4096`. 4101 << 32766, so `conn.prepare` never hits "too many SQL variables". The monolithic IN clause is real but does not fail at any reachable scan size. No code change. (If the project ever switches off `bundled` to a system SQLite older than 3.32, batching into ≤990-id chunks would become necessary.)

DEVANA-KEY: crates/cli/src/storage/semantic_store_read.rs:685 | P1 | sqlite-semantic-in-bind-overflow
DEVANA-SUMMARY: Status=invalid | P1 high crates/cli/src/storage/semantic_store_read.rs:685 - Premise outdated: bundled SQLite 3.51.3 caps variables at 32766, and max bind count is ~4101 (KNN limit 4096), so no overflow occurs.