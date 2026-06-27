DEVANA-FINDING: v1
Priority: P2 | Confidence: medium | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server/content.rs:209 | Slug: read-match-provenance-as-read-file

# Read Match Is Persisted As Read File Provenance

## Finding

The public `read_match` tool delegates to `read_file_impl` and does not record its own provenance event, so durable provenance records the call as `read_file` and drops the handle-based input contract.

## Violated Invariant Or Contract

Public tool provenance should record the invoked public tool and its input shape.

## Oracle

`read_match` is a public MCP read tool, provenance persists `tool_name`, and workload export trusts persisted tool names and params.

## Counterexample

Call `search_text`, then call `read_match` with the returned `result_handle` and `match_id`. The durable event records `tool_name=read_file` with path and line params, not `read_match` with the handle and match id.

## Why It Might Matter

Workload corpus and recent-provenance consumers misclassify follow-up behavior and lose the causal link between a search result handle and the file slice reopened from it.

## Proof

Cross-entry provenance mismatch: `read_match_impl` resolves the handle and constructs `ReadFileParams` at `crates/cli/src/mcp/server/content.rs:209`, then calls `read_file_impl` at line 236. There is no separate `record_provenance` path for `read_match`, so the only durable row is emitted by the delegated read-file implementation.

## Counterevidence Checked

The delegated read records a real file read and preserves repository/path/line information. It does not preserve the public entrypoint, `result_handle`, or `match_id`.

## Suggested Next Step

Record a `read_match` provenance event around the public handler, or pass an execution context override into the delegated read so the durable row reflects the public tool.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-27: fixed. Confirmed: `read_match_impl` resolved the handle, built `ReadFileParams`, and delegated to `read_file_impl`, whose hardcoded `record_provenance_with_outcome_and_metadata("read_file", ...)` was the only durable row — so `read_match` calls persisted as `read_file`, dropping the public tool name and the `result_handle`/`match_id` input contract. Fix: threaded a `ReadFileProvenanceContext` descriptor through a new `read_file_impl_with_provenance(params, provenance)` (the public `read_file_impl` calls it with `ReadFileProvenanceContext::read_file()`). The descriptor carries the recorded tool name and an `extra_params` object merged into the provenance params. `read_match_impl` now delegates with `ReadFileProvenanceContext::read_match(result_handle, match_id)`, so the single durable row records `tool_name=read_match` with the handle and match id alongside the resolved path/line params — still one provenance event, no double counting. Regression test `read_match_records_read_match_provenance_not_read_file` (runtime_gate_tests/cache_runtime.rs) reindexes a fixture, runs `search_text` to mint a real `result_handle`+`match_id`, calls `read_match`, then asserts exactly one `read_match` provenance row carrying both ids and zero `read_file` rows. `cargo test read_match`/`read_file`/`provenance` green.

DEVANA-KEY: crates/cli/src/mcp/server/content.rs:209 | P2 | read-match-provenance-as-read-file
DEVANA-SUMMARY: Status=fixed | P2 medium crates/cli/src/mcp/server/content.rs:209 - `read_match` calls are persisted as `read_file` provenance, losing the public tool name and result_handle/match_id contract.
