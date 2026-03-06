# Tasks — 30-citation-hygiene-gate

Meta:
- Spec: 30-citation-hygiene-gate — Citation Hygiene Gate
- Depends on: 13-contract-and-doc-drift-closure, 27-doc-contract-sync-wave2
- Global scope:
  - scripts/, docs/overview.md, docs/security/release-readiness.md, Justfile

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement deterministic citation hygiene checker and gate wiring (owner: worker:019cbd37-5bd4-7fe3-a797-2a8ce6f2df9a) (scope: scripts/, docs/overview.md, docs/security/release-readiness.md, Justfile) (depends: -)
  - Started_at: 2026-03-05T08:57:48Z
  - Completed_at: 2026-03-05T09:03:54Z
  - Completion note: Added deterministic offline citation hygiene gate (`scripts/check-citation-hygiene.sh`), wired it into docs-sync and release-readiness, and updated overview/release docs to reflect enforced citation hygiene closure.
  - Validation result: Mayor-verified `bash scripts/check-citation-hygiene.sh`, `bash scripts/check-doc-sync.sh`, and `bash scripts/check-release-readiness.sh` all passed.
