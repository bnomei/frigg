# Design — 43-document-symbols-byte-guards

## Scope
- `crates/cli/src/mcp/`
- `contracts/`
- `benchmarks/`
- `specs/43-document-symbols-byte-guards/`
- `specs/index.md`

## Problem statement
`document_symbols` currently resolves the file path and then reads the entire source string directly. Unlike `read_file`, it does not enforce `max_file_bytes` before allocating the content buffer. That leaves a predictable allocation spike on large files even though the MCP server already has a read-budget contract elsewhere.

## Approach
Add an explicit metadata-based precheck to `document_symbols` and keep the public tool contract narrow:

1. Resolve the file path exactly as today.
2. Stat the file before reading it.
3. Reject files above `server.config.max_file_bytes` with a typed deterministic `invalid_params` payload.
4. Preserve the existing outline extraction path for in-budget files.

## Architecture changes

### Budget source
- Reuse the existing server-level `max_file_bytes` configuration rather than adding a new `document_symbols` request parameter.
- Keep the error metadata aligned with the existing `read_file` budget shape where practical.

### Extraction path
- Continue to use the existing language gate and tree-sitter extraction flow for in-budget files.
- Do not change supported language behavior or add lossy decoding behavior in this spec.

## Risks
- Error payload drift versus `read_file` can create avoidable contract inconsistency if not aligned intentionally.
- Future callers may want an override parameter; this spec intentionally does not add one.

## Validation strategy
- Regression tests for an over-budget `document_symbols` request returning the expected typed error shape.
- Regression tests proving in-budget files still return the same deterministic symbol outline.
- Contract/docs coverage if the public read-only tool documentation mentions byte-budget semantics.
