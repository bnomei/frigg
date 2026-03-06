# Design — 11-mcp-hotpath-caching-and-provenance

## Normative excerpt
- MCP outputs must remain deterministic.
- Provenance persistence is mandatory but should not dominate tool latency.

## Architecture
- `FriggMcpServer` gains cache state:
  - symbol corpus cache keyed by `(repository_id, root_signature)`.
  - precise graph cache keyed by `(repository_id, scip_signature, corpus_signature)`.
  - provenance storage cache keyed by repository target id/path.
- Introduce cheap signatures:
  - `root_signature`: deterministic hash of manifest digest metadata (path + mtime + size + hash).
  - `scip_signature`: deterministic hash over discovered SCIP artifact paths + mtimes + sizes.
- Provenance trace IDs:
  - use UUID v7 for monotonic sortable ids.
- `storage` schema tuning:
  - add indexes for latest snapshot and per-tool provenance queries.

## Data flow
1. Compute signatures for requested repositories.
2. Lookup or rebuild symbol corpus/precise graph cache entries.
3. Serve results with deterministic ordering unchanged.
4. Append provenance via cached storage handle and v7 trace id.

## Acceptance signals
- Repeated `search_symbol`/`find_references` benchmarks improve materially.
- No behavior drift in existing integration tests.
- Storage query plans for latest snapshot/provenance avoid full scans.
