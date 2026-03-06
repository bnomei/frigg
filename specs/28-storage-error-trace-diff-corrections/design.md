# Design — 28-storage-error-trace-diff-corrections

## Scope
- crates/storage/, crates/mcp/, contracts/

## Approach
- Apply a minimal, targeted patch set within the spec scope.
- Reuse existing helpers and typed error pathways where available.
- Add or extend regression tests/benchmarks to lock behavior.

## Data flow and behavior
- Request enters existing API/tool boundary.
- New guardrails/indexing/caching/contract normalization logic executes.
- Response and provenance metadata remain deterministic and typed.

## Risks
- Cross-crate contract drift if docs/tests are not updated together.
- Performance regressions if new safety checks add redundant scans.

## Validation strategy
- cargo test -p storage && cargo test -p mcp playbook_suite
