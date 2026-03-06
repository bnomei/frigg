# MCP Tool Schemas Contract (`v1`)

This directory is the public contract for Frigg MCP tool schemas version `v1`.

## 1) Versioning policy

- `v1` is the active major contract. All `v1` schema files in this directory are compatible with each other by the rules below.
- A new major (`v2`) is required for any breaking change to an existing `v1` tool schema.
- `v2` must be introduced as a sibling directory (`contracts/tools/v2/`) and must not mutate published `v1` schemas in place.
- Breaking changes must be recorded in the `Breaking change log` section of the affected major version README.

## 2) Per-tool schema file naming convention

- One JSON schema file per MCP tool.
- File name format: `<tool_name>.v1.schema.json`
- `<tool_name>` is the MCP tool name mapped to a filesystem-safe token:
- Mapping rule: lowercase, keep `[a-z0-9._-]`, replace all other characters with `_`.
- Examples:
- `read_file` -> `read_file.v1.schema.json`
- `search.text` -> `search.text.v1.schema.json`

## 3) Breaking vs non-breaking changes

Breaking (requires next major, e.g. `v2`):
- Removing a field.
- Renaming a field.
- Changing field type or format incompatibly.
- Making an optional field required.
- Narrowing allowed enum values, ranges, or string patterns.
- Changing response shape in a way that breaks existing clients.

Non-breaking (allowed within `v1`):
- Adding a new optional field.
- Expanding enum values/ranges in a backward-compatible way.
- Clarifying descriptions, examples, or documentation-only metadata.
- Adding response fields that clients may ignore.

## 4) Deprecation window policy

- Any `v1` field/tool marked deprecated must remain available for at least one minor release cycle after deprecation notice.
- The notice must include:
- `deprecated_since` (release tag/date),
- `removal_in` (target major),
- migration guidance.
- Removals only occur in the next major (`v2+`) unless a critical security issue requires accelerated removal.

## 5) Mapping schemas to MCP tool names

- MCP `tools/list` entry `name` is the canonical identity.
- Canonical schema ID format: `frigg.tools.<tool_name>.v1`.
- Each tool implementation must reference the matching schema file in this directory and expose the same contract via `inputSchema`.
- If aliases are needed, aliases are documentation-only and must resolve to one canonical `<tool_name>` schema.

## v1 public read-only tool schemas (runtime `tools/list`)

<!-- tool-surface-profile:core:start -->
- `list_repositories` -> `list_repositories.v1.schema.json` (`ListRepositoriesParams` / `ListRepositoriesResponse`)
- `workspace_attach` -> `workspace_attach.v1.schema.json` (`WorkspaceAttachParams` / `WorkspaceAttachResponse`)
- `workspace_current` -> `workspace_current.v1.schema.json` (`WorkspaceCurrentParams` / `WorkspaceCurrentResponse`)
- `read_file` -> `read_file.v1.schema.json` (`ReadFileParams` / `ReadFileResponse`)
- `search_text` -> `search_text.v1.schema.json` (`SearchTextParams` / `SearchTextResponse`)
- `search_hybrid` -> `search_hybrid.v1.schema.json` (`SearchHybridParams` / `SearchHybridResponse`)
- `search_symbol` -> `search_symbol.v1.schema.json` (`SearchSymbolParams` / `SearchSymbolResponse`)
- `find_references` -> `find_references.v1.schema.json` (`FindReferencesParams` / `FindReferencesResponse`)
- `go_to_definition` -> `go_to_definition.v1.schema.json` (`GoToDefinitionParams` / `GoToDefinitionResponse`)
- `find_declarations` -> `find_declarations.v1.schema.json` (`FindDeclarationsParams` / `FindDeclarationsResponse`)
- `find_implementations` -> `find_implementations.v1.schema.json` (`FindImplementationsParams` / `FindImplementationsResponse`)
- `incoming_calls` -> `incoming_calls.v1.schema.json` (`IncomingCallsParams` / `IncomingCallsResponse`)
- `outgoing_calls` -> `outgoing_calls.v1.schema.json` (`OutgoingCallsParams` / `OutgoingCallsResponse`)
- `document_symbols` -> `document_symbols.v1.schema.json` (`DocumentSymbolsParams` / `DocumentSymbolsResponse`)
- `search_structural` -> `search_structural.v1.schema.json` (`SearchStructuralParams` / `SearchStructuralResponse`)
<!-- tool-surface-profile:core:end -->

## v1 optional deep-search read-only tool schemas (feature-gated runtime `tools/list`)

<!-- tool-surface-profile:extended_only:start -->
- `deep_search_run` -> `deep_search_run.v1.schema.json` (`DeepSearchRunParams` / `DeepSearchRunResponse`)
- `deep_search_replay` -> `deep_search_replay.v1.schema.json` (`DeepSearchReplayParams` / `DeepSearchReplayResponse`)
- `deep_search_compose_citations` -> `deep_search_compose_citations.v1.schema.json` (`DeepSearchComposeCitationsParams` / `DeepSearchComposeCitationsResponse`)
<!-- tool-surface-profile:extended_only:end -->
- These schemas are part of the `v1` public contract and are excluded from default `core` runtime `tools/list`; they are exposed only when the `extended` deep-search runtime profile is explicitly enabled (`FRIGG_MCP_TOOL_SURFACE_PROFILE=extended`).
- The three deep-search schema docs also publish `contract_notes`, `nested_contracts`, `step_tool_schema_refs`, `input_example`, and `output_example` because their top-level wrapper fields (`playbook`, `trace_artifact`, `citation_payload`) contain the real first-call ergonomics burden.
- First-time clients should call `list_repositories`; if it returns an empty list or a session-local default repository is needed, call `workspace_attach` before read/search/navigation tools.
- `workspace_current` returns the session default repository selected by `workspace_attach`; omitted `repository_id` values on read/search/navigation tools prefer that session default before falling back to all attached repositories.
- For raw stdio MCP clients, Frigg now defaults to a quiet `error` tracing filter when `RUST_LOG` is unset and defaults built-in watch mode to `off`; opt back into built-in watch behavior with `--watch-mode auto` or `--watch-mode on`.
- This README is the canonical public contract for core versus extended MCP tool-surface gating, `tools/list` visibility, and semantic-response metadata across the read-only surface.

## v1 canonical path contract

- `read_file`, `search_text`, `search_hybrid`, `search_symbol`, `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`, `document_symbols`, and `search_structural` responses expose repository-relative canonical `path` values.
- Canonical `path` values are root-stripped (no workspace-root prefix), use `/` separators, and avoid `./` prefixes.
- `search_text.path_regex` is matched against those canonical repository-relative paths before files are searched, so clients can narrow broad queries to code, docs, or runtime slices without changing search semantics.
- `read_file.path` input remains backward-compatible: repository-relative paths are canonical and absolute paths are accepted when they resolve inside attached workspace roots.
- `read_file` response `path` is still canonical repository-relative regardless of request form.
- `read_file` supports optional one-based inclusive line slicing (`line_start`, `line_end`). For sliced reads, `max_bytes` is enforced against returned slice content (not full-file size), and invalid ranges fail as typed `invalid_params`.
- Optional response `note` metadata is serialized as a JSON-encoded string inside the wrapper payload; dotted references such as `note.precise.*` refer to the parsed JSON payload, not a nested wrapper object.

## v1 deterministic behavior and limits (IDE navigation/read-only tools)

- Tool set: `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`, `document_symbols`, `search_structural`.
- All seven tools are exposed as read-only/idempotent MCP tools (`read_only_hint=true`, `destructive_hint=false`, `idempotent_hint=true`).
- `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls` accept optional `symbol` and optional source-position targeting (`path` + `line` + `column`), prefer precise SCIP-backed results first, and include deterministic fallback metadata in `note` (`precision`, `heuristic`, `fallback_reason` when applicable).
- Source-position targeting resolves the nearest preceding symbol on the requested canonical path; when `column` is provided, same-line symbols are disambiguated deterministically by start column before stable-id tie-breaks.
- `find_references` symbol resolution metadata in `note.target_selection` is deterministic and includes the selected symbol anchor plus ambiguity counters (`candidate_count`, `same_rank_candidate_count`, `ambiguous_query`) so generic symbol names remain auditable.
- `find_references` heuristic fallback `note` includes `precise_absence_reason` to explain why precise SCIP relationships were unavailable (`no_scip_artifacts_discovered`, `scip_artifact_ingest_failed`, `precise_partial_non_authoritative_absence`, `target_not_present_in_precise_graph`, `no_usable_precise_data`).
- Precise diagnostics are deterministic in navigation notes: `note.precise.candidate_directories` lists SCIP discovery paths, `note.precise.discovered_artifacts` lists sampled discovered artifact paths, and `note.precise.failed_artifacts` lists sampled read/ingest failures (`artifact_label`, `stage`, `detail`).
- Runtime SCIP artifact discovery under `.frigg/scip` is deterministic and extension-scoped: `.json` fixtures and binary `.scip` protobuf payloads are both ingested on the precise path.
- SCIP ingest resource-budget overages on precise-only paths degrade deterministically to heuristic fallback (recorded in `note.precise.failed_artifacts`) instead of hard-failing the tool call.
- Navigation precise-mode eligibility is deterministic per repository snapshot: mixed-success SCIP ingest retains successful precise records and reports `note.precise.coverage=partial`, yielding `precision=precise_partial` when retained precise hits are returned. Empty precise lookups in partial mode remain non-authoritative and fall back heuristically with failure diagnostics in `note.precise.failed_artifacts`.
- `find_implementations`, `incoming_calls`, and `outgoing_calls` include relation metadata in result payloads and use deterministic ordering keys.
- For `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`, and `search_structural`, effective result `limit` is deterministic: `min(requested_or_default, max_search_results.max(1))`.
- `document_symbols` is deterministic and read-only for Rust/PHP files only; unsupported extensions fail as typed `invalid_params`.
- `document_symbols` enforces the server `max_file_bytes` budget before reading file contents; over-budget requests fail as typed `invalid_params` with `path`, `bytes`, `max_bytes`, and `config_max_file_bytes`.
- `search_structural` is deterministic tree-sitter query search for Rust/PHP only; `query` must be non-empty and at most `4096` characters, `language` (if provided) must be `rust` or `php`, and `path_regex` must satisfy safe-regex validation.
- `search_hybrid` is deterministic hybrid retrieval over lexical + graph + semantic channels, supports optional channel-weight overrides and semantic toggle, mirrors semantic probe fields at the top level (`semantic_requested`, `semantic_enabled`, `semantic_status`, `semantic_reason`), and retains the same metadata in `note` for backward compatibility.
- `search_hybrid` is the broad natural-language entrypoint for mixed doc/runtime questions and may intentionally diversify top hits across contracts, README, runtime, and tests instead of collapsing to one file class.
- When a client needs concrete implementation anchors after `search_hybrid`, follow with `search_symbol` for a known API/type/function name or use `search_text.path_regex` to constrain the witness set to doc/runtime slices explicitly.
- `search_hybrid` strict semantic failures are part of the public contract too: `semantic_status=strict_failure` maps to canonical `unavailable` in [`contracts/errors.md`](../../errors.md).
- When multi-token natural-language queries underfill exact lexical results, `search_hybrid` deterministically expands lexical recall via bounded exact-token and token-regex recall before ranking. This lexical evidence expansion remains active even when the semantic channel is enabled.
- Failures for these tools map to canonical error taxonomy codes in `contracts/errors.md` (`invalid_params`, `resource_not_found`, `timeout`, `index_not_ready`, `unavailable`, `internal`) with typed metadata.

## Future write-surface security policy (`v1`)

These policy markers are normative and release-gated before any write/destructive MCP tool is added.

- `write_surface_policy: v1`
- `current_public_tool_surface: read_only`
- `write_confirm_param: confirm`
- `write_confirm_required: true`
- `write_confirm_semantics: reject_missing_or_false_confirm_before_side_effects`
- `write_confirm_failure_error_code: confirmation_required`
- `write_safety_invariant_workspace_boundary: required`
- `write_safety_invariant_path_traversal_defense: required`
- `write_safety_invariant_regex_budget_limits: required`
- `write_safety_invariant_typed_deterministic_errors: required`

## Breaking change log

- `v1`: Initial baseline contract.
