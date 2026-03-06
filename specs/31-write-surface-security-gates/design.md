# Design — 31-write-surface-security-gates

## Scope
- contracts/errors.md
- contracts/tools/v1/README.md
- docs/security/threat-model.md
- docs/security/release-readiness.md
- contracts/changelog.md
- scripts/check-release-readiness.sh
- crates/mcp/tests/
- crates/mcp/src/mcp/types.rs

## Approach
- Add explicit contract language for future write tools:
  - required `confirm` semantics
  - canonical `confirmation_required` typed error
  - required path/regex safety invariants
- Add release gate checks to enforce policy presence.
- Add preemptive MCP security regression checks to detect unsafe write-surface additions.

## Data flow and behavior
- Release script validates policy markers/docs before passing.
- MCP test suite asserts public contract remains safe and rejects unsafe expansions.

## Risks
- Overly rigid policy checks may block benign contract refactors unless marker wording is kept stable.

## Validation strategy
- cargo test -p mcp --test security
- cargo test -p mcp
- bash scripts/check-release-readiness.sh
