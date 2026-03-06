# Tasks — 37-public-surface-parity-gates

Meta:
- Spec: 37-public-surface-parity-gates — Public Surface Parity Gates
- Depends on: 34-readonly-ide-navigation-tools, 35-semantic-runtime-mcp-surface, 36-deep-search-runtime-tools
- Global scope:
  - crates/mcp/
  - crates/cli/
  - contracts/tools/v1/
  - contracts/changelog.md
  - docs/overview.md
  - scripts/
  - Justfile

## In Progress

- (none)

## Blocked

- (none)

## Todo

## Done

- [x] T001: Add runtime tool-surface manifest profiles and parity test harness (owner: worker:019cbdf0-e25e-7782-8485-32e0557d9d5c) (scope: crates/mcp/, crates/mcp/tests/) (depends: -)
  - Started_at: 2026-03-05T12:20:14Z
  - Completed_at: 2026-03-05T12:28:00Z
  - Completion note: Added runtime tool-surface profile manifests (`core`, `extended`) with deterministic diff helpers and runtime introspection on `FriggMcpServer`; added `tool_surface_parity` integration tests to assert active-profile parity and ordering.
  - Validation result: `cargo test -p mcp schema_ -- --nocapture` and `cargo test -p mcp --test tool_surface_parity -- --nocapture` passed.
- [x] T002: Add docs/schema parity checker script with deterministic diff output (owner: worker:019cbdf0-e25e-7782-8485-32e0557d9d5c) (scope: scripts/, contracts/tools/v1/, docs/overview.md) (depends: T001)
  - Started_at: 2026-03-05T12:28:38Z
  - Completed_at: 2026-03-05T12:34:09Z
  - Completion note: Added profile-aware deterministic parity checker script (`scripts/check-tool-surface-parity.py`) comparing runtime manifests, schema files, and docs sections, plus stable docs markers and script docs.
  - Validation result: `python3 scripts/check-tool-surface-parity.py` passed and `python3 scripts/check-tool-surface-parity.py --intentional-fail docs_missing` failed deterministically with expected diagnostics.
- [x] T003: Wire parity checks into docs-sync and release-ready gates (owner: worker:019cbdf0-e25e-7782-8485-32e0557d9d5c) (scope: scripts/, Justfile, docs/security/release-readiness.md) (depends: T002)
  - Started_at: 2026-03-05T12:35:28Z
  - Completed_at: 2026-03-05T12:42:51Z
  - Completion note: Wired `scripts/check-tool-surface-parity.py` into docs-sync and release-ready gates with deterministic summary-line validation and updated operator-facing release-readiness documentation/checklist semantics.
  - Validation result: `just docs-sync` and `just release-ready` passed with tool-surface parity summary output.
- [x] T004: Reconcile current tool-surface drift and update changelog/docs semantics (owner: mayor) (scope: contracts/changelog.md, contracts/tools/v1/README.md, docs/overview.md, crates/mcp/) (depends: T003)
  - Started_at: 2026-03-05T13:26:44Z
  - Completed_at: 2026-03-05T13:42:03Z
  - Completion note: Closed runtime/schema/docs parity drift across core and extended tool-surface markers and semantics, including `search_hybrid` in the core runtime tool list documentation, and reconciled contract narrative with runtime manifest expectations.
  - Validation result: `python3 scripts/check-tool-surface-parity.py`, `cargo test -p mcp`, `just docs-sync`, and `just release-ready` passed (`tool-surface-parity status=pass`, `benchmark_summary pass=33 fail=0 missing=0`).
