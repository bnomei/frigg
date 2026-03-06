# Tasks — 49-typescript-tsx-precise-scip-parity

Meta:
- Spec: 49-typescript-tsx-precise-scip-parity — TypeScript/TSX Precise SCIP Parity
- Depends on: 05-scip-precision-ingest, 41-partial-precise-degradation, 48-typescript-tsx-runtime-symbol-surface
- Global scope:
  - crates/cli/src/graph/mod.rs
  - crates/cli/src/storage/mod.rs
  - crates/cli/src/mcp/server.rs
  - crates/cli/tests/
  - fixtures/scip/
  - contracts/
  - docs/overview.md
  - README.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- [ ] T001: Create a TypeScript/TSX SCIP fixture matrix and artifact guidance pack (owner: unassigned) (scope: fixtures/scip/) (depends: -)
  - Context: the precise ingest path is already generic, but there is no validated TS/TSX fixture pack proving definitions, references, relationships, partial failures, or JSX-oriented cases.
  - Reuse_targets: existing SCIP matrix fixtures, malformed artifact patterns, `fixtures/scip/README.md`
  - Autonomy: high
  - Risk: medium
  - Complexity: medium
  - DoD: the fixture pack covers TS and TSX definitions/references, implementation/call relationships, private identifiers, overload/declaration anchors, at least one malformed artifact, and enough README guidance to use the fixtures deterministically in tests.
  - Validation: `cargo test -p frigg scip_`
  - Escalate if: the available fixture format cannot express one of the required TS/TSX cases without introducing new artifact conventions.

- [ ] T002: Harden precise ingest and canonical path matching for TS/TSX documents (owner: unassigned) (scope: crates/cli/src/graph/mod.rs, crates/cli/src/storage/mod.rs) (depends: T001)
  - Context: precise storage is language-agnostic, but TS/TSX paths still need explicit validation against runtime corpus expectations, incremental replacement semantics, and partial-aware retention rules.
  - Reuse_targets: existing SCIP parse/map/apply flow, incremental replacement helpers, partial precise retention behavior from spec 41
  - Autonomy: high
  - Risk: medium
  - Complexity: medium
  - DoD: ingest and incremental replacement pass for TS/TSX fixtures, canonical path matching works for `.ts` and `.tsx`, and unaffected precise rows remain intact across partial updates and failures.
  - Validation: `cargo test -p frigg scip_`, `cargo test -p frigg precision_precedence_`
  - Escalate if: TS/TSX SCIP symbol strings or document path semantics require a storage-contract change instead of test-driven hardening.

- [ ] T003: Add precise-first TS/TSX navigation and reference regression coverage (owner: unassigned) (scope: crates/cli/src/mcp/server.rs, crates/cli/tests/tool_handlers.rs) (depends: T002)
  - Context: the tool handlers already prefer precise results when available, but there is no TS/TSX validation matrix proving that behavior across references, definitions, declarations, implementations, and call hierarchy tools.
  - Reuse_targets: existing precise precedence tests in `tool_handlers.rs`, current note metadata assertions, resource-budget degradation helpers
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: integration tests prove precise precedence and partial-degradation metadata for TS/TSX across `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls`.
  - Validation: `cargo test -p frigg --test tool_handlers`
  - Escalate if: any tool lacks stable note fields needed to distinguish TypeScript/TSX precise results from heuristic fallback behavior.

- [ ] T004: Sync TypeScript/TSX precise-coverage contracts and operator docs (owner: unassigned) (scope: contracts/, docs/overview.md, README.md, specs/49-typescript-tsx-precise-scip-parity/) (depends: T003)
  - Context: public precise-coverage guidance currently describes the generic SCIP path but does not say what TS/TSX artifact placement, partial-degradation behavior, or validated tool paths are covered.
  - Reuse_targets: `contracts/errors.md`, `contracts/changelog.md`, `contracts/tools/v1/README.md`, `docs/overview.md`, `README.md`
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - DoD: public docs/contracts describe validated TypeScript/TSX precise coverage, artifact placement, and degradation behavior without overstating external generator guarantees.
  - Validation: `cargo test -p frigg schema_`, `just docs-sync`
  - Escalate if: the desired docs language would imply support for an external SCIP generator workflow the repo has not validated.

## Done

- (none)
