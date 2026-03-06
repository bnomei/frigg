# Tasks — 00-contracts-and-governance

Meta:
- Spec: 00-contracts-and-governance — Public Contracts and Governance
- Depends on: -
- Global scope:
  - contracts/
  - docs/overview.md
  - docs/phases.md
  - specs/index.md
  - scripts/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Author tool schema/versioning contract baseline (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: contracts/) (depends: -)
  - Started_at: 2026-03-04T16:25:22Z
  - Completed_at: 2026-03-04T16:28:23Z
  - Completion note: Established the v1 MCP schema contract baseline with explicit versioning, naming, compatibility, and deprecation rules mapped to MCP tool names.
  - Validation result: `test -f contracts/tools/v1/README.md` and `rg -n "version|breaking" contracts/tools/v1/README.md` passed.
- [x] T002: Define typed public error taxonomy (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: contracts/) (depends: T001)
  - Started_at: 2026-03-04T16:28:23Z
  - Completed_at: 2026-03-04T16:30:19Z
  - Completion note: Added canonical v1 error taxonomy with deterministic semantics, retryability guidance, MCP mapping hints, and a per-tool mapping template.
  - Validation result: `test -f contracts/errors.md` and `rg -n "invalid_params|resource_not_found|internal" contracts/errors.md` passed.
- [x] T003: Define configuration contract and defaults policy (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: contracts/, crates/config/) (depends: T001)
  - Started_at: 2026-03-04T16:30:19Z
  - Completed_at: 2026-03-04T16:32:17Z
  - Completion note: Documented the v1 configuration contract and aligned runtime defaults/validation in `crates/config`.
  - Validation result: `rg -n "workspace_roots|max_search_results|max_file_bytes" contracts/config.md crates/config/src/lib.rs` passed.
- [x] T004: Add docs-sync guard script (owner: worker:019cb9aa-f032-7153-a3de-ace79d676435) (scope: scripts/, specs/index.md) (depends: T001, T002, T003)
  - Started_at: 2026-03-04T16:32:17Z
  - Completed_at: 2026-03-04T16:35:48Z
  - Completion note: Added deterministic docs-sync guard script and synchronized `specs/index.md` marker/date contract.
  - Validation result: `bash scripts/check-doc-sync.sh` passed.
