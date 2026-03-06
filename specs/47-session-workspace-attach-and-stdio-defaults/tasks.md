# Tasks — 47-session-workspace-attach-and-stdio-defaults

Meta:
- Spec: 47-session-workspace-attach-and-stdio-defaults — Session Workspace Attach and Stdio One-Shot Defaults
- Depends on: 07-mcp-server-and-tool-contracts, 10-mcp-surface-hardening, 22-tool-path-semantics-unification, 44-integrated-local-watch-mode
- Global scope:
  - crates/cli/src/main.rs
  - crates/cli/src/settings/
  - crates/cli/src/mcp/server.rs
  - crates/cli/src/mcp/types.rs
  - contracts/tools/v1/
  - contracts/errors.md
  - contracts/config.md
  - README.md
  - specs/47-session-workspace-attach-and-stdio-defaults/
  - specs/index.md

## In Progress

- (none)

## Blocked

- (none)

## Todo

- [x] T001: Split MCP serving startup from utility-command root validation and add transport-aware watch defaults (owner: codex) (scope: crates/cli/src/main.rs, crates/cli/src/settings/, contracts/config.md, README.md, specs/47-session-workspace-attach-and-stdio-defaults/) (depends: -)
  - Context: HTTP must be allowed to start with zero bootstrap roots, while `init`/`verify`/`reindex` continue requiring explicit roots. Stdio should default to watch `off` when watch mode is not explicitly configured; HTTP should keep `auto`.
  - Reuse_targets: current `resolve_command_config(...)`, `resolve_startup_config(...)`, watch-mode helpers in `main.rs` and `settings/mod.rs`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Bundle_with: T004
  - DoD: MCP serving mode can start with zero roots, utility commands still validate roots, and effective watch defaults are transport-specific and documented.
  - Validation: `cargo test -p frigg startup_ watch_ -- --nocapture`
  - Escalate if: serving-mode empty roots force a broader config-contract split than this spec anticipates.

- [x] T002: Add process-wide workspace registry and new MCP tool contracts for `workspace_attach` and `workspace_current` (owner: codex) (scope: crates/cli/src/mcp/server.rs, crates/cli/src/mcp/types.rs, contracts/tools/v1/, contracts/errors.md, specs/47-session-workspace-attach-and-stdio-defaults/) (depends: T001)
  - Context: Frigg needs an attach-on-demand workflow that reuses canonical roots, reports storage readiness without indexing, and provides session-default metadata.
  - Reuse_targets: existing `list_repositories`, storage path helpers, repository record wrappers, typed error helpers in `server.rs`
  - Autonomy: standard
  - Risk: medium
  - Complexity: high
  - Bundle_with: T003
  - DoD: `workspace_attach` and `workspace_current` are exposed with versioned schemas, typed errors, and storage-readiness responses, and duplicate attach calls reuse the same canonical workspace record.
  - Validation: `cargo test -p frigg workspace_attach_ -- --nocapture`
  - Escalate if: attach semantics require a new public error code rather than reuse of the current taxonomy.

- [x] T003: Apply session-default repository precedence to existing read/search/navigation tools and dynamic `list_repositories` output (owner: codex) (scope: crates/cli/src/mcp/server.rs, crates/cli/src/mcp/types.rs, contracts/tools/v1/README.md, specs/47-session-workspace-attach-and-stdio-defaults/) (depends: T002)
  - Context: Explicit `repository_id` must keep working, but omitted `repository_id` should prefer a session default before falling back to all attached repositories.
  - Reuse_targets: current root resolution helpers, `roots_for_repository(...)`, `list_repositories` response wrappers
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: existing tools honor explicit repository hints first, then session default, then process-wide fan-out; `list_repositories` reflects the current attached registry instead of only startup roots.
  - Validation: `cargo test -p frigg workspace_session_ -- --nocapture`
  - Escalate if: per-session state cannot be keyed deterministically from the active rmcp session/peer identity.

- [x] T004: Add stdio cwd/git-root auto-attach and one-shot no-index readiness flow (owner: codex) (scope: crates/cli/src/main.rs, crates/cli/src/mcp/server.rs, crates/cli/src/settings/, README.md, specs/47-session-workspace-attach-and-stdio-defaults/) (depends: T001, T002)
  - Context: A stdio spawn in a repo should be able to attach the repo automatically, read existing `.frigg/storage.sqlite3` state, answer one-shot requests, and exit without starting duplicate background watchers.
  - Reuse_targets: current-dir resolution in tests, storage readiness gates, watch transport gating
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - DoD: stdio startup with no explicit roots auto-attaches Git root or cwd as session default, reports index readiness without triggering reindex, and stays watch-off unless explicitly configured otherwise.
  - Validation: `cargo test -p frigg stdio_workspace_ -- --nocapture`
  - Escalate if: cwd/Git-root resolution needs platform-specific repository discovery logic beyond a bounded local helper.

- [x] T005: Add focused docs and regression coverage for workspace attach, attach reuse, and no-workspace remediation (owner: codex) (scope: crates/cli/src/main.rs, crates/cli/src/mcp/server.rs, crates/cli/tests/, contracts/config.md, contracts/errors.md, README.md, specs/index.md, specs/47-session-workspace-attach-and-stdio-defaults/) (depends: T002, T003, T004)
  - Context: This spec changes both startup expectations and first-call UX, so docs and tests must lock the behavior in.
  - Reuse_targets: current startup tests in `main.rs`, tool-handler test style, README transport sections, specs index conventions
  - Autonomy: standard
  - Risk: low
  - Complexity: medium
  - DoD: docs describe attach-first HTTP and one-shot stdio flows, tests cover empty-start HTTP, stdio auto-attach, duplicate attach reuse, and typed no-workspace errors, and the program index tracks the new spec.
  - Validation: `cargo test -p frigg workspace_ -- --nocapture`
  - Escalate if: doc sync requires a broader contract/index reorganization beyond this spec.

## Done

- [x] T001: Split MCP serving startup from utility-command root validation and add transport-aware watch defaults.
- [x] T002: Add process-wide workspace registry and new MCP tool contracts for `workspace_attach` and `workspace_current`.
- [x] T003: Apply session-default repository precedence to existing read/search/navigation tools and dynamic `list_repositories` output.
- [x] T004: Add stdio cwd/git-root auto-attach and one-shot no-index readiness flow.
- [x] T005: Add focused docs and regression coverage for workspace attach, attach reuse, and no-workspace remediation.
