# Tasks — 31-write-surface-security-gates

Meta:
- Spec: 31-write-surface-security-gates — Future Write Surface Security Gates
- Depends on: 09-security-benchmarks-ops, 10-mcp-surface-hardening, 25-release-gate-execution-hardening
- Global scope:
  - contracts/, docs/security/, scripts/check-release-readiness.sh, crates/mcp/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Document and gate future write-surface confirmation/security policy (owner: worker:019cbd37-5bd4-7fe3-a797-2a8ce6f2df9a) (scope: contracts/, docs/security/, scripts/check-release-readiness.sh) (depends: -)
  - Started_at: 2026-03-05T09:05:13Z
  - Completed_at: 2026-03-05T09:08:05Z
  - Completion note: Added canonical write-surface policy clauses (`confirm` semantics and `confirmation_required`), updated threat model/release checklist markers, and enforced deterministic release-gate checks (`write_surface_policy=v1`) for policy drift.
  - Validation result: Worker + mayor verified `bash scripts/check-release-readiness.sh` passed.
- [x] T002: Add preemptive MCP regression coverage against unsafe write-tool introductions (owner: worker:019cbd37-5bd4-7fe3-a797-2a8ce6f2df9a) (scope: crates/mcp/tests/, crates/mcp/src/mcp/types.rs) (depends: T001)
  - Started_at: 2026-03-05T09:08:53Z
  - Completed_at: 2026-03-05T09:12:25Z
  - Completion note: Added deterministic MCP security/schema regression guards that enforce an explicit read-only tool surface and contract marker expectations, preventing accidental unsafe write-surface introduction without coordinated contract updates.
  - Validation result: Worker + mayor verified `cargo test -p mcp --test security` and `cargo test -p mcp` passed.
