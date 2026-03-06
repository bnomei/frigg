# Tasks — 43-document-symbols-byte-guards

Meta:
- Spec: 43-document-symbols-byte-guards — `document_symbols` Byte Budget Guards
- Depends on: 34-readonly-ide-navigation-tools, 39-performance-memory-hardening
- Global scope:
  - crates/cli/src/mcp/
  - contracts/
  - benchmarks/
  - specs/43-document-symbols-byte-guards/
  - specs/index.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add `document_symbols` byte-budget precheck and typed over-budget errors (owner: mayor) (scope: crates/cli/src/mcp/, specs/43-document-symbols-byte-guards/) (depends: -)
  - Started_at: 2026-03-06T12:30:00Z
  - Completed_at: 2026-03-06T14:05:00Z
  - Context: `document_symbols` should apply the same server-level read budget discipline as other full-file reads, but without adding a new tool parameter in this spec.
  - Reuse_targets: `read_file` budget error metadata, `server.config.max_file_bytes`
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - DoD: `document_symbols` rejects over-budget files before `read_to_string()`, returns a typed deterministic `invalid_params` payload, and preserves current behavior for in-budget files.
  - Validation: targeted `cargo test -p frigg --test tool_handlers document_symbols`.
  - Escalate if: matching `read_file` error metadata requires a documented contract decision rather than a direct reuse of the existing shape.

- [x] T002: Sync read-only tool docs and benchmark notes if the new guard changes operator guidance (owner: mayor) (scope: contracts/, benchmarks/, specs/43-document-symbols-byte-guards/, specs/index.md) (depends: T001)
  - Completed_at: 2026-03-06T14:05:00Z
  - Context: This task is only required if the implementation adds operator-visible guidance or contract notes beyond the new over-budget error path.
  - Reuse_targets: read-only navigation tool contract docs, spec 39 benchmark note style
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - DoD: contracts and benchmark notes describe the new `document_symbols` budget behavior when needed; otherwise this task records that no public doc delta was required.
  - Validation: manual contract/spec sync pass and benchmark-note update in `benchmarks/mcp-tools.md`.
  - Escalate if: the existing docs never mention `document_symbols` resource behavior and adding the note would create more churn than value.
