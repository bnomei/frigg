# Design — 12-search-index-hotpath-and-correctness

## Normative excerpt
- Deterministic ordering is non-negotiable for search and references.
- Security/ops posture requires visibility into skipped files and read failures.

## Architecture
- `crates/search/src/lib.rs`
  - optimize hot loops by streaming match columns and reducing transient allocations.
  - introduce deterministic bounded retention for low-limit workloads.
  - capture deterministic diagnostics for walker/read failures.
- `crates/index/src/lib.rs`
  - retain same-line multiple token matches in heuristic reference upsert key.
  - emit deterministic diagnostics from manifest build walker/read paths.
- `crates/mcp/src/mcp/server.rs`
  - include diagnostic counts/notes where available in tool `note` metadata.

## Acceptance signals
- Existing deterministic ordering tests remain green.
- New tests cover same-line multi-reference preservation and diagnostic surfacing.
- Benchmarks show lower overhead for limited-result workloads.
