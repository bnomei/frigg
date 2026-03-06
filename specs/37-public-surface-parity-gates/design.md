# Design — 37-public-surface-parity-gates

## Scope
- crates/mcp/
- crates/cli/
- contracts/tools/v1/
- contracts/changelog.md
- docs/overview.md
- scripts/
- Justfile

## Normative excerpt (in-repo)
- Tool identity is contract-bound to schema files and MCP `tools/list` names.
- Public tool surface is read-only and must remain deterministic.
- Current repo has schema/type/docs declarations that can drift from runtime registration unless explicitly gated.

## Architecture decisions
1. Treat runtime tool registration as source of truth for executable surface.
2. Derive expected schema/doc tool lists from a single manifest profile produced by code.
3. Make docs-sync/release-ready fail when runtime and docs diverge.

## Surface profiles
- `core`: default runtime profile (feature flags disabled).
- `extended`: profile with optional runtime tool gates enabled (for example deep-search runtime tools).
- Each profile has explicit expected tool-name set and deterministic order.

## Enforcement points
1. Unit/integration test: runtime `tools/list` set equals profile manifest set.
2. Schema parity test: profile manifest set equals `contracts/tools/v1/*.schema.json` (or documented profile subset rules).
3. Docs parity script: parses `contracts/tools/v1/README.md` and `docs/overview.md` runtime tool sections, compares against manifest.
4. Release gate integration: `just docs-sync` and `just release-ready` invoke parity checks.

## Deterministic diagnostics
- On failure, report:
  - `profile`
  - `missing_in_runtime`
  - `missing_in_schema`
  - `missing_in_docs`
  - `unexpected_in_runtime`
- Output format is stable JSON + concise one-line summary for CI logs.

## Rollout order
1. Land profile manifest + runtime/tools test.
2. Land docs parity checker + release gate wiring.
3. Resolve current drift and keep gate mandatory for future changes.
