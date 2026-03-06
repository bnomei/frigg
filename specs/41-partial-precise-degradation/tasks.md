# Tasks — 41-partial-precise-degradation

Meta:
- Spec: 41-partial-precise-degradation — Partial Precise Degradation Instead Of Corpus-Wide Drop
- Depends on: 05-scip-precision-ingest, 24-find-references-resource-budgets, 34-readonly-ide-navigation-tools, 39-performance-memory-hardening
- Global scope:
  - crates/cli/src/mcp/
  - crates/cli/src/graph/
  - contracts/
  - benchmarks/
  - specs/41-partial-precise-degradation/
  - specs/index.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Retain successful precise ingest state and add explicit precise coverage mode metadata (owner: mayor) (scope: crates/cli/src/mcp/, crates/cli/src/graph/, specs/41-partial-precise-degradation/) (depends: -)
  - Completed_at: 2026-03-06T14:05:00Z
  - Context: The current path records artifact failures and then clears all precise data for the corpus. The new implementation must keep successful precise data and record whether the cached graph is `full`, `partial`, or `none`.
  - Reuse_targets: existing `PreciseIngestStats`, `CachedPreciseGraph`, SCIP ingest failure samples
  - Autonomy: standard
  - Risk: high
  - Complexity: medium
  - DoD: successful precise records remain cached after mixed artifact ingest; cached metadata exposes an explicit precise coverage mode; no handler interprets partial-mode empty precise state as authoritative yet.
  - Validation: targeted `cargo test -p frigg --test tool_handlers` mixed-artifact coverage.
  - Escalate if: retaining partial precise state requires a breaking redesign of existing note fields or graph storage invariants.

- [x] T002: Make read-only navigation/reference handlers partial-aware and sync public metadata (owner: mayor) (scope: crates/cli/src/mcp/, contracts/, benchmarks/, specs/41-partial-precise-degradation/, specs/index.md) (depends: T001)
  - Completed_at: 2026-03-06T14:05:00Z
  - Context: In partial precise mode, positive precise hits are useful, but empty precise lookups are not authoritative. All read-only precise-aware handlers must share the same fallback rule.
  - Reuse_targets: `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`
  - Autonomy: standard
  - Risk: high
  - Complexity: medium
  - DoD: all precise-aware handlers branch on the shared partial-mode rule; response notes describe degraded precise coverage deterministically; contracts and benchmark guidance are updated if note semantics change.
  - Validation: targeted handler tests for partial precise hits and empty partial precise lookups, plus `cargo test -p frigg --test tool_handlers`.
  - Escalate if: one or more tools cannot express partial precise coverage without a contract version bump.
