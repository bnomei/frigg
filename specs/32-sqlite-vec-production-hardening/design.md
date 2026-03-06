# Design — 32-sqlite-vec-production-hardening

## Scope
- crates/storage/src/lib.rs
- crates/embeddings/src/lib.rs
- crates/cli/src/main.rs
- crates/storage/ tests in lib.rs
- crates/embeddings/ tests
- scripts/smoke-ops.sh
- contracts/storage.md
- contracts/changelog.md
- docs/overview.md

## Approach
- Enforce sqlite-vec registration in storage runtime connection path.
- Add version-pin enforcement using a deterministic required-version policy.
- Wire strict vector readiness checks into CLI server startup before serving MCP.
- Expand tests and smoke checks for version mismatch and strict backend expectations.

## Data flow and behavior
- Connection open -> sqlite-vec registration check -> vec version probe -> backend selection/readiness.
- Server startup aborts early on strict readiness violations.
- Release/smoke artifacts prove backend and readiness semantics.

## Risks
- Local environments without sqlite-vec may fail new strict startup checks; error messaging must be explicit.
- Version matching must allow normalized prefixes (`v` vs non-`v`) without weakening pin guarantees.

## Validation strategy
- cargo test -p storage
- cargo test -p embeddings
- cargo test -p frigg
- bash scripts/smoke-ops.sh
