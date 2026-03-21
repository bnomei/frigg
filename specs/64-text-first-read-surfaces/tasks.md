# Tasks — 64-text-first-read-surfaces

Meta:
- Spec: 64-text-first-read-surfaces — Text-First Read Surfaces
- Depends on: -
- Global scope:
  - specs/64-text-first-read-surfaces/
  - crates/cli/src/mcp/server.rs
  - crates/cli/src/mcp/server/content.rs
  - crates/cli/src/mcp/server_state.rs
  - crates/cli/src/mcp/types/workspace.rs
  - crates/cli/src/mcp/types/search.rs
  - crates/cli/src/mcp/deep_search/runtime.rs
  - crates/cli/src/mcp/server/runtime_gate_tests/cache_runtime.rs
  - crates/cli/tests/tool_handlers/
  - crates/cli/src/mcp/guidance.rs
  - README.md
  - skills/frigg-mcp-search-navigation/

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add read-surface presentation-mode parameters and split internal payloads from public MCP presentation (owner: codex) (scope: crates/cli/src/mcp/types/workspace.rs, crates/cli/src/mcp/types/search.rs, crates/cli/src/mcp/server.rs, crates/cli/src/mcp/server_state.rs) (depends: -)
  - Covers: R001, R002, R003, R004, R005, R007, R008
  - Context: `read_file`, `read_match`, and `explore` currently hard-code `Json<T>` at the public tool boundary. The implementation needs a presentation selector and an internal typed payload seam before any text-first rendering can happen cleanly.
  - Reuse_targets: `ReadFileParams`, `ReadMatchParams`, `ExploreParams`, `ReadFileExecution`, `ExploreExecution`
  - Autonomy: high
  - Risk: medium
  - Complexity: medium
  - Bundle_with: T002
  - Verification_mode: required
  - Verification_status: complete
  - DoD: read-oriented param types expose `presentation_mode`; the public tool layer can emit more than one presentation shape without forcing internal execution to parse rendered text; operation-aware defaults are expressible.
  - Validation: `cargo check -p frigg --tests`
  - Escalate if: the MCP tool macro layer cannot support mixed text/json public results for one tool without a breaking tool-registration change.

- [x] T002: Implement text-first default presentation for `read_file` and `read_match` with explicit JSON compatibility mode (owner: codex) (scope: crates/cli/src/mcp/server/content.rs, crates/cli/src/mcp/server.rs, crates/cli/tests/tool_handlers/) (depends: T001)
  - Covers: R001, R002, R004, R006, R009, R011, R012, R013
  - Context: the current responses are already minimal structurally, but the full source body still sits inside a JSON object. This task changes the public default so the source slice is the primary text payload while preserving a deliberate JSON escape hatch.
  - Reuse_targets: `read_file_impl(...)`, `read_match_impl(...)`, shared file-content snapshot/window helpers, current `ReadFileResponse` / `ReadMatchResponse` field semantics
  - Autonomy: high
  - Risk: medium
  - Complexity: medium
  - Verification_mode: required
  - Verification_status: complete
  - DoD: `read_file` and `read_match` default to text-first MCP output; JSON mode stays available; the default path does not duplicate the full body in structured output; line-range and max-byte behavior stay unchanged.
  - Validation:
    `cargo test -p frigg --test tool_handlers -- core_read_file_ --nocapture`
    plus `cargo test -p frigg --test tool_handlers -- core_read_match_ --nocapture`
  - Escalate if: preserving stable public JSON compatibility requires a separate tool name instead of a mode flag.

- [x] T003: Make `explore` operation-aware: text-first for `zoom`, structured by default for `probe` and `refine` (owner: codex) (scope: crates/cli/src/mcp/server/content.rs, crates/cli/src/mcp/types/search.rs, crates/cli/tests/tool_handlers/, crates/cli/src/mcp/server/runtime_gate_tests/cache_runtime.rs) (depends: T001)
  - Covers: R003, R004, R005, R007, R009, R013
  - Context: `zoom` is functionally a bounded read surface, but `probe` and `refine` still earn their keep through structured match rows, anchors, windows, cursors, and truncation state. The implementation should treat those operations differently instead of flattening all of `explore`.
  - Reuse_targets: `ExploreOperation`, `ExploreResponse`, existing scan/match/window builders, shared file-content cache reuse tests
  - Autonomy: high
  - Risk: medium
  - Complexity: medium
  - Verification_mode: required
  - Verification_status: complete
  - DoD: `explore zoom` defaults to text-first output; `probe` and `refine` remain structured by default; unsupported text-mode requests for `probe` or `refine` fail with `invalid_params`; cache-sharing tests still pass.
  - Validation:
    `cargo test -p frigg --test tool_handlers -- extended_explore_ --nocapture`
    plus `cargo test -p frigg read_file_and_explore_share_the_file_content_window_cache -- --nocapture`
  - Escalate if: the mixed per-operation presentation model introduces an MCP schema ambiguity that cannot be documented clearly at the tool boundary.

- [x] T004: Update deep-search normalization, provenance-facing seams, and docs/skill guidance for the new default read surfaces (owner: codex) (scope: crates/cli/src/mcp/deep_search/runtime.rs, crates/cli/src/mcp/guidance.rs, README.md, skills/frigg-mcp-search-navigation/, specs/64-text-first-read-surfaces/) (depends: T002, T003)
  - Covers: R008, R010
  - Context: internal trace normalization and user guidance still assume JSON-shaped read results. They need to either consume internal typed payloads or request JSON mode explicitly, and the public guidance needs to explain the new defaults without implying that all of `explore` became plain text.
  - Reuse_targets: `normalize_read_file_response(...)`, MCP guidance text, skill references for `read_match` / `explore`
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - Verification_mode: mayor
  - Verification_status: complete
  - DoD: deep-search/read-trace flows no longer assume the public default is JSON; guidance and skill docs describe text-first reads, JSON compatibility mode, and `explore`'s mixed behavior accurately.
  - Validation:
    `cargo test -p frigg --test provenance provenance_core_tool_invocations_are_persisted -- --nocapture`
    plus `cargo test -p frigg --test security security_read_file_resolves_absolute_path_under_later_workspace_root -- --nocapture`
  - Escalate if: any downstream consumer outside the repo is known to require the current public JSON default and cannot be migrated to explicit JSON mode.
