# Design — 42-manifest-freshness-validation

## Scope
- `crates/cli/src/mcp/`
- `crates/cli/src/searcher/`
- `contracts/`
- `benchmarks/`
- `specs/42-manifest-freshness-validation/`
- `specs/index.md`

## Problem statement
The current persisted-manifest fast path validates only that snapshot paths still exist before reusing snapshot digests. That keeps the no-walk path cheap, but it leaves two correctness gaps:

- symbol-corpus reuse can trust stale snapshot metadata when files changed in place;
- manifest-backed search candidate discovery can claim a snapshot-backed fast path even though the snapshot no longer matches the filesystem.

The net effect is a loose freshness contract rather than a deterministic “fast when current, rebuild when stale” rule.

## Approach
Introduce an explicit metadata-only freshness validator for persisted manifest snapshots:

1. Resolve each snapshot entry to its active filesystem path.
2. Compare current `exists`, `size_bytes`, and `mtime_ns` against the stored digest metadata.
3. If every entry validates, reuse the validated metadata set as the canonical manifest digest set for signatures, candidate paths, and cache keys.
4. If any entry fails validation, discard snapshot fast-path reuse for that repository and rebuild live manifest metadata through the existing manifest builder.

## Architecture changes

### Shared freshness validator
- Add a helper that returns validated current digests, not just source paths.
- Make the helper usable by both symbol-corpus fast paths and manifest-backed search candidate discovery.

### Symbol-corpus caching
- Compute the corpus root signature from the validated digest set.
- Avoid reusing stale in-memory corpus entries when the persisted snapshot is present but no longer matches on-disk metadata.

### Search candidate discovery
- Reuse the same freshness contract when manifest snapshots seed candidate files for text/regex/hybrid search.
- Preserve current live fallback behavior for repositories with no manifest snapshot.

## Risks
- Per-entry `fs::metadata()` validation increases stat work on hot paths.
- Filesystems with coarse or unstable mtimes can create false invalidations if comparison rules are too strict.
- Snapshot validation logic can drift between MCP and search paths if the helper is not shared.

## Validation strategy
- Regression tests for missing, renamed, and edited-in-place files proving stale snapshots rebuild live metadata instead of reusing the fast path.
- Regression tests proving unchanged snapshots still take the metadata-only reuse path.
- Benchmark or timing evidence showing the validator avoids source parsing and full file reads even when it rejects stale snapshots.
