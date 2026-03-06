# Design — 39-performance-memory-hardening

## Scope
- `crates/cli/src/indexer/`
- `crates/cli/src/searcher/`
- `crates/cli/src/mcp/`
- `crates/cli/src/graph/`
- `crates/cli/src/storage/`
- `contracts/`
- `benchmarks/`
- `README.md`

## Problem statement
The current runtime still pays repeated full-repository walks, whole-file reads, and whole-artifact or whole-snapshot buffering in several hot paths:

- search paths still walk repositories and load full file contents per query;
- symbol-corpus and precise-graph caches still require fresh manifest or SCIP discovery work before cache lookup;
- precise navigation helpers scan and clone whole precise maps for point lookups;
- SCIP ingest fully materializes artifact payloads and re-scan-replaces occurrences per document;
- semantic indexing and semantic query paths duplicate chunk text and load full snapshot records for scoring;
- `read_file` line slicing still reads and materializes the entire file.

## Approach
Apply a targeted hardening pass in six slices:

1. Stream manifest hashing and line-slice reads.
2. Replace query-time repository walks with manifest-backed candidate discovery where possible.
3. Add fast cache signatures or cached discovery metadata so cache hits avoid redundant filesystem work.
4. Add precise-graph secondary indexes for symbol/file/relationship lookups and use them in navigation handlers.
5. Add lean semantic storage/query APIs so scoring does not load `content_text` eagerly, and make semantic writes incremental for changed-only reindex.
6. Update tests, docs, and contracts together.

## Architecture changes

### Streaming file I/O
- Manifest hashing will use buffered reads and incremental Blake3 updates.
- `read_file` line-range mode will use a buffered reader over raw bytes, apply lossy UTF-8 conversion per selected line, and enforce `max_bytes` against the sliced response only.

### Manifest-backed query discovery
- Search paths will use the latest persisted manifest snapshot when available to obtain deterministic candidate files.
- If no manifest exists, search will fall back to the current filesystem walk behavior.
- This preserves correctness for uninitialized roots while removing redundant directory walks for indexed repositories.

### Cache fastpaths
- Symbol-corpus and precise-graph caching will separate cache key discovery from heavyweight rebuild work.
- Unchanged repositories and unchanged SCIP directories should hit cache with metadata-only work rather than re-extracting symbols or re-ingesting precise data.

### Precise graph indexing
- Add secondary indexes for:
  - occurrences by `(repository_id, symbol)`
  - occurrences by `(repository_id, path)`
  - relationships by `(repository_id, from_symbol)`
  - relationships by `(repository_id, to_symbol, kind)`
  - precise symbols by repository for navigation ranking
- Apply/replacement operations must update these indexes incrementally.

### Semantic lean loads and incremental writes
- Storage will expose a lean query projection that returns only the fields needed for scoring.
- Query-time semantic search will score from the lean projection and fetch `content_text` only for the top ranked chunk ids used in the final response.
- Changed-only reindex will replace semantic rows for changed/deleted files rather than deleting all rows for the repository.

## Risks
- Indexed lookup paths can drift from existing deterministic ordering if sort points move.
- Incremental semantic replacement can leave stale rows if changed/deleted path bookkeeping is incomplete.
- Streaming line slicing must preserve existing lossy UTF-8 behavior and byte-limit error semantics.

## Validation strategy
- Targeted unit/integration tests for:
  - manifest hashing determinism
  - `read_file` line slicing on large and invalid-UTF8 inputs
  - manifest-backed search candidate discovery fallback behavior
  - precise graph indexed lookups and incremental SCIP apply behavior
  - lean semantic query loading and changed-only semantic replacement
- Full validation: `cargo test -p frigg`
