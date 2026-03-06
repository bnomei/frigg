# Tasks — 23-smoke-ops-fresh-binary

Meta:
- Spec: 23-smoke-ops-fresh-binary — Smoke Ops Fresh Binary Enforcement
- Depends on: 09-security-benchmarks-ops
- Global scope:
  - scripts/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement Smoke Ops Fresh Binary Enforcement and lock with regression coverage (owner: worker:019cbace-f43a-79f1-a017-d5adae9f1fbc) (scope: scripts/) (depends: -)
  - Started_at: 2026-03-04T21:44:20Z
  - Completed_at: 2026-03-04T21:46:03Z
  - Completion note: Removed stale-binary fallback; smoke-ops now fails hard on frigg build failure.
  - Validation result: `bash scripts/smoke-ops.sh` passed.
