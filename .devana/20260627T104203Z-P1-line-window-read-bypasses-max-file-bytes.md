DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server/content.rs:126 | Slug: line-window-read-bypasses-max-file-bytes

# Line-window reads load entire files without max_file_bytes pre-check

## Finding

`read_file` with `line_start` / `line_end`, `read_match`, and `explore` all load
file content through `file_content_snapshot_for_workspace`, which always `fs::read`s
the full file. The `max_file_bytes` gate runs only for whole-file reads (no line
range): when `has_line_range` is true, `pre_read_bytes` stays `None` and the size
check is skipped before the full read and normalized buffer allocation.

## Violated Invariant Or Contract

`max_file_bytes` should bound memory use before reading a file on read-only MCP
tools, not only the size of the returned slice.

## Oracle

Whole-file `read_file` path stats the file and rejects when `metadata.len() >
max_bytes` before loading (`content.rs` ~126–152). Line-window and `read_match`
paths skip that gate then call `file_content_snapshot_for_workspace`, which
unconditionally `fs::read`s (`runtime_cache.rs` ~147–153).

## Counterexample

- Workspace file size 50 MiB; server `max_file_bytes` default 2 MiB
- Client calls `read_file(path, line_start=1, line_end=10)` or `read_match` on a
  prior search handle
- Server loads ~50 MiB raw bytes plus normalized copy (~100 MiB peak), returns a
  small line window
- Cache may refuse entries above 32 MiB, but allocation already occurred

## Why It Might Matter

A client can force large memory use on the read-only MCP surface using bounded
line-window requests, causing process pressure or OOM without exceeding the
configured byte limit on the response.

## Proof

**Dataflow trace:** MCP params with line range → `has_line_range=true` → skip stat
gate → `file_content_snapshot_for_workspace` → `fs::read` entire file → slice for
response.

**Cross-entry mismatch:** whole-file `read_file` enforces `max_file_bytes` pre-read;
line-window `read_file`, `read_match`, and `explore` do not.

## Counterevidence Checked

- `explore` bounds scan output via `max_file_bytes` after load, not before
- File content window cache budget can evict large entries post-allocation
- Adoption and path containment gates still apply; this is a size-bound gap, not
  path escape

## Suggested Next Step

Stat or bound-read before `fs::read` for all snapshot paths, or use a streaming
line reader that does not load the whole file for line-window requests.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence
prefix. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes
below with evidence checked.

## Status Notes

- 2026-06-27: open by Devana. Initial report from static inspection across
  `boundaries-oracles` and `outside-in-entrypoints` trails.
- 2026-06-27: fixed. Confirmed `read_file_impl_with_provenance` (content.rs) only
  sets `pre_read_bytes` (and runs the stat gate) when `!has_line_range`; line-window
  reads, `read_match`, and presentation reads then funnel through
  `file_content_snapshot_for_workspace` (runtime_cache.rs), whose two `fs::read`
  calls loaded the full file unconditionally before slicing. Fixed at the chokepoint:
  added `read_file_content_bytes_bounded`, which stats the file and rejects with
  `invalid_params` (`file exceeds max_file_bytes=N`, data `bytes`/`max_file_bytes`)
  when the on-disk size exceeds `config.max_file_bytes`, then reads. Both `fs::read`
  sites in `file_content_snapshot_for_workspace` now go through it, so all three
  read-only callers are bounded. The cap is `config.max_file_bytes` (the memory
  cap), NOT the per-request `max_bytes` (slice budget), so the existing
  `core_read_file_line_range_can_bypass_full_file_size_limit` behavior (small
  per-request max_bytes still returns a slice of a small file) is preserved. Whole-
  file reads already gate earlier against the smaller clamped `max_bytes`, so they
  never reach this cap. Added regression test
  `core_read_file_line_range_rejects_file_exceeding_max_file_bytes_before_read`.
  `tool_handlers` (128 pass; the one unrelated pre-existing failure
  `core_search_hybrid_strict_semantic_requires_startup_credentials` fails on the
  base commit too — semantic-credentials env test) and `security` (14) suites green.

DEVANA-KEY: crates/cli/src/mcp/server/content.rs:126 | P1 | line-window-read-bypasses-max-file-bytes
DEVANA-SUMMARY: Status=fixed | P1 high crates/cli/src/mcp/server/content.rs:126 - Line-window read_file, read_match, and presentation reads loaded entire files before slicing; fixed by stat-gating against max_file_bytes inside file_content_snapshot_for_workspace (the shared chokepoint) plus a regression test.