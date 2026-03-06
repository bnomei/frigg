# Tasks — 44-integrated-local-watch-mode

Meta:
- Spec: 44-integrated-local-watch-mode — Integrated Local Watch Mode
- Depends on: 07-mcp-server-and-tool-contracts, 20-reindex-resilience-diagnostics, 42-manifest-freshness-validation
- Global scope:
  - crates/cli/src/main.rs
  - crates/cli/src/settings/
  - crates/cli/src/watch.rs
  - README.md
  - specs/44-integrated-local-watch-mode/
  - specs/index.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add watch config types, defaults, and CLI/env resolution (owner: mayor) (scope: crates/cli/src/main.rs, crates/cli/src/settings/, specs/44-integrated-local-watch-mode/) (depends: -)
  - Completed_at: 2026-03-06T13:37:36Z
  - Context: Watch mode must be configurable through `--watch-mode <auto|on|off>`, `--watch-debounce-ms`, `--watch-retry-ms`, and matching `FRIGG_*` env vars. `auto` enables the watcher only for stdio and loopback HTTP.
  - Reuse_targets: existing semantic runtime config resolution, HTTP runtime resolution, current startup config tests in `main.rs`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Bundle_with: T004
  - DoD: watch config is part of startup resolution, defaults are enforced, and activation policy is testable from pure config/runtime helpers.
  - Validation: `cargo test -p frigg watch_ -- --nocapture`
  - Escalate if: watch activation semantics require a new public contract beyond CLI/env and README guidance.

- [x] T002: Implement watcher supervisor and root-scoped debounce state machine (owner: mayor) (scope: crates/cli/src/watch.rs, specs/44-integrated-local-watch-mode/) (depends: T001)
  - Completed_at: 2026-03-06T13:37:36Z
  - Context: One watcher supervisor should observe all configured workspace roots, debounce each root independently, and run only one changed-only background reindex at a time across the process. Roots need `pending`, `last_event_at`, `in_flight`, and `rerun_requested` state.
  - Reuse_targets: current-thread Tokio runtime usage in `main.rs`, existing `reindex_repository(..., ReindexMode::ChangedOnly)`
  - Autonomy: standard
  - Risk: medium
  - Complexity: high
  - Bundle_with: T003
  - DoD: accepted filesystem events turn into root-local dirty signals, ignored paths do not trigger jobs, and rerun coalescing works without parallel background reindexes.
  - Validation: `cargo test -p frigg scheduler_ -- --nocapture`
  - Escalate if: `notify` behavior across platforms forces a broader compatibility contract than local stdio/loopback usage.

- [x] T003: Wire watch supervisor into stdio and loopback HTTP startup with logs-only observability (owner: mayor) (scope: crates/cli/src/main.rs, crates/cli/src/watch.rs, README.md, specs/44-integrated-local-watch-mode/) (depends: T001, T002)
  - Completed_at: 2026-03-06T13:37:36Z
  - Context: Built-in watch mode is a local UX feature. It should start before the serve loop when enabled, remain disabled by default for non-loopback HTTP, and never add a new MCP tool or status surface in v1.
  - Reuse_targets: existing startup logging, HTTP runtime bind safety checks, README reindex section
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: stdio and loopback HTTP can start with watch mode enabled, remote HTTP stays off in `auto`, and watch events/logs do not corrupt stdio protocol framing.
  - Validation: `cargo test -p frigg watch_runtime_ -- --nocapture`
  - Escalate if: startup ordering forces a transport-level lifecycle change outside the stated scope.

- [x] T004: Add focused config and state-machine regression coverage (owner: mayor) (scope: crates/cli/src/main.rs, crates/cli/src/watch.rs, specs/44-integrated-local-watch-mode/) (depends: T001, T002)
  - Completed_at: 2026-03-06T13:37:36Z
  - Context: The watcher should be protected by deterministic tests for config defaults, transport gating, debounce, serialized execution, rerun coalescing, and retry after failure.
  - Reuse_targets: existing startup tests in `main.rs`, test helper style in `crates/cli/src/searcher/mod.rs` and `crates/cli/src/indexer/mod.rs`
  - Autonomy: standard
  - Risk: low
  - Complexity: medium
  - DoD: the new watch-mode policy is executable from unit/integration tests without requiring a manual watcher session.
  - Validation: `cargo test -p frigg watch_ -- --nocapture`
  - Escalate if: deterministic coverage requires a small internal test seam around the filesystem event source.
