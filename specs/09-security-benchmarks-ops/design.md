# Design — 09-security-benchmarks-ops

## Normative excerpt (from `docs/overview.md`)
- Security gates are non-optional: sandboxing, regex limits, write confirmation, transport safety.
- Performance budgets must be tracked per rollout slice and tool type.
- Operational workflows must be explicit and reproducible.

## Architecture
- Security test vectors in `docs/security/` and automated tests in crate test suites.
- Benchmark reports in `benchmarks/` with fixed workload definitions.
- Operational command validation in `scripts/` and CLI integration tests.

## Gate model
- Security gate: all abuse/path tests pass.
- Performance gate: latency/throughput budgets met or explicitly waived with rationale.
- Operability gate: command workflows deterministic and documented.
