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

## v1 optional extended read-only tool schemas (feature-gated runtime `tools/list`)

<!-- tool-surface-profile:extended_only:start -->
- `explore` -> `explore.v1.schema.json` (`ExploreParams` / `ExploreResponse`)
- `deep_search_run` -> `deep_search_run.v1.schema.json` (`DeepSearchRunParams` / `DeepSearchRunResponse`)
- `deep_search_replay` -> `deep_search_replay.v1.schema.json` (`DeepSearchReplayParams` / `DeepSearchReplayResponse`)
- `deep_search_compose_citations` -> `deep_search_compose_citations.v1.schema.json` (`DeepSearchComposeCitationsParams` / `DeepSearchComposeCitationsResponse`)
<!-- tool-surface-profile:extended_only:end -->
- These schemas are part of the `v1` public contract and are excluded from default `core` runtime `tools/list`; they are exposed only when the `extended` runtime profile is explicitly enabled (`FRIGG_MCP_TOOL_SURFACE_PROFILE=extended`).
- `explore` is the bounded single-artifact follow-up tool for probe/zoom/refine workflows after repo-wide discovery.
- The deep-search schema docs also publish `contract_notes`, `nested_contracts`, `step_tool_schema_refs`, `input_example`, and `output_example` because their top-level wrapper fields (`playbook`, `trace_artifact`, `citation_payload`) contain the real first-call ergonomics burden.
- First-time clients should call `list_repositories`; if it returns an empty list or a session-local default repository is needed, call `workspace_attach` before read/search/navigation tools.
- `workspace_current` returns the session default repository selected by `workspace_attach`, plus additive `repositories` and `runtime` blocks for attached-repo state, runtime profile, active/recent tasks, and recent provenance; omitted `repository_id` values on read/search/navigation tools prefer that session default before falling back to all attached repositories.
- `workspace_current.runtime.profile` distinguishes `stdio_ephemeral`, `stdio_attached`, `http_loopback_service`, and `http_remote_service`; `workspace_current` is the typed read-only status surface even when MCP resources are unavailable.
- Repository summaries returned by `list_repositories` and `workspace_current` include nested `storage` plus split index `health` (`lexical`, `semantic`, `scip`). `workspace_current.health.lexical` and `workspace_current.health.semantic` reuse the shared manifest/semantic freshness contract used by watch/search startup (`missing_manifest_snapshot`, `stale_manifest_snapshot`, `manifest_valid_no_semantic_eligible_entries`, `semantic_snapshot_missing_for_active_model`). `workspace_attach` keeps `storage` at the top level and returns split `health` inside `repository` so attach responses do not duplicate the storage block.
- For raw stdio MCP clients, Frigg now defaults to a quiet `error` tracing filter when `RUST_LOG` is unset and defaults built-in watch mode to `off`; opt back into built-in watch behavior with `--watch-mode auto` or `--watch-mode on`.
- Loopback HTTP remains the preferred warm local-service profile; non-loopback HTTP still requires explicit `--allow-remote-http` plus `--mcp-http-auth-token`.
- This README is the canonical public contract for core versus extended MCP tool-surface gating, `tools/list` visibility, and semantic-response metadata across the read-only surface.

## v1 canonical path contract

- `read_file`, `explore`, `search_text`, `search_hybrid`, `search_symbol`, `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`, `document_symbols`, and `search_structural` responses expose repository-relative canonical `path` values.
- Canonical `path` values are root-stripped (no workspace-root prefix), use `/` separators, and avoid `./` prefixes.
- `search_text.path_regex` is matched against those canonical repository-relative paths before files are searched, so clients can narrow broad queries to code, docs, or runtime slices without changing search semantics.
- `search_text` is the repository-aware exact-text tool; for simple direct scans in the current checkout, local shell tools such as `rg` may be faster.
- `read_file.path` input remains backward-compatible: repository-relative paths are canonical and absolute paths are accepted when they resolve inside attached workspace roots.
- `read_file` response `path` is still canonical repository-relative regardless of request form.
- `read_file` supports optional one-based inclusive line slicing (`line_start`, `line_end`). For sliced reads, `max_bytes` is enforced against returned slice content (not full-file size), and invalid ranges fail as typed `invalid_params`.
- `read_file` is the repository-aware file reader for canonical paths and bounded slices; for ordinary local inspection without repo-aware semantics, shell reads may be simpler and faster.
- `explore.path` input follows the same canonical rules as `read_file`, and `explore` returns repository-relative `path` plus explicit line/column anchors and stateless `resume_from` cursors for bounded follow-up.
- Additive response `metadata` fields expose the canonical structured payloads for navigation/search tools; clients should prefer `metadata` when present and treat `note` as legacy backward-compatibility transport when a given tool still emits it.
- Optional response `note` metadata is serialized as a JSON-encoded string inside the wrapper payload; dotted references such as `note.precise.*` refer to the parsed JSON payload, not a nested wrapper object.

## v1 deterministic behavior and limits (IDE navigation/read-only tools)

- Tool set: `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`, `document_symbols`, `search_structural`.
- All eight tools are exposed as read-only/idempotent MCP tools (`read_only_hint=true`, `destructive_hint=false`, `idempotent_hint=true`).
- `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls` accept optional `symbol` and optional source-position targeting (`path` + `line` + optional `column`), prefer precise SCIP-backed results first, and include deterministic fallback metadata in `note` (`precision`, `heuristic`, `fallback_reason` when applicable).
- `search_text` and `find_references` expose top-level `total_matches` so callers can distinguish the returned slice from the complete match count.
- Source-position targeting resolves the nearest preceding symbol on the requested canonical path; when `column` is provided, same-line symbols are disambiguated deterministically by start column before stable-id tie-breaks. When location fields are present, deterministic location resolution takes precedence over `symbol` and `note.resolution_source` records the chosen path.
- Symbol-only navigation target-resolution metadata in `note.target_selection` is deterministic and includes the selected symbol anchor, selected path class, and ambiguity counters (`candidate_count`, `same_rank_candidate_count`, `ambiguous_query`) so generic symbol names remain auditable.
- When matching precise definition data exists, symbol-targeted precise navigation stays pinned to that selected target anchor instead of drifting to another same-name precise symbol by lexical order alone.
- Ambiguous exact-name symbol-only resolution ranks runtime code under `src/` ahead of `benches/`, `examples/`, and `tests/`, then preserves the existing repository/path/line/stable-id tie-break order within that path class.
- `find_references` heuristic fallback `note` includes `precise_absence_reason` to explain why precise SCIP relationships were unavailable (`no_scip_artifacts_discovered`, `scip_artifact_ingest_failed`, `precise_partial_non_authoritative_absence`, `target_not_present_in_precise_graph`, `no_usable_precise_data`).
- Precise diagnostics are deterministic in navigation notes: `note.precise.candidate_directories` lists SCIP discovery paths, `note.precise.discovered_artifacts` lists sampled discovered artifact paths, and `note.precise.failed_artifacts` lists sampled read/ingest failures (`artifact_label`, `stage`, `detail`).
- Runtime SCIP artifact discovery under `.frigg/scip` is deterministic and extension-scoped: `.json` fixtures and binary `.scip` protobuf payloads are both ingested on the precise path.
- SCIP ingest resource-budget overages on precise-only paths degrade deterministically to heuristic fallback (recorded in `note.precise.failed_artifacts`) instead of hard-failing the tool call.
- Navigation precise-mode eligibility is deterministic per repository snapshot: mixed-success SCIP ingest retains successful precise records and reports `note.precise.coverage=partial`, yielding `precision=precise_partial` when retained precise hits are returned. Empty precise lookups in partial mode remain non-authoritative and fall back heuristically with failure diagnostics in `note.precise.failed_artifacts`.
- `find_implementations`, `incoming_calls`, and `outgoing_calls` include relation metadata in result payloads and use deterministic ordering keys.
- `find_implementations` first uses direct precise implementation/type-definition relationships, then attempts occurrence-backed precise recovery from enclosing implementation definitions before dropping to heuristic fallback.
- `incoming_calls` may still derive precise occurrence-backed matches when explicit precise relationships are absent. Callable targets emit `relation="calls"` when the recovered source line is call-like; non-call occurrences remain `relation="refers_to"`.
- `incoming_calls` and `outgoing_calls` expose optional `call_path`, `call_line`, `call_column`, `call_end_line`, and `call_end_column` when a precise occurrence anchor exists for the call site.
- `outgoing_calls` is callable-only for both precise and heuristic results. Occurrence-derived precise survivors are labeled `calls`, and Frigg keeps an empty result rather than widening to locals, fields, constants, or type-only references.
- `search_symbol` now accepts optional `path_class` (`runtime`, `project`, `support`) and `path_regex` filters before ranking, and same-rank discovery results prefer runtime code under `src/` before project/support paths.
- `search_symbol` also uses PHP canonical class-like names and deterministic class-target evidence when local source evidence can resolve them, so exact queries such as `App\\Foo\\Bar` or `App\\Foo\\Bar::baz` participate in deterministic symbol selection without any Laravel-specific parameters.
- For `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`, and `search_structural`, effective result `limit` is deterministic: `min(requested_or_default, max_search_results.max(1))`.
- `document_symbols` is deterministic and read-only for Rust, PHP, Blade, TypeScript / TSX, Python, Go, Kotlin / KTS, Lua, Roc, and Nim files; unsupported extensions fail as typed `invalid_params`.
- `document_symbols` returns hierarchical `children` nodes rather than a flat-only outline, and child nodes carry `container` names for compatibility.
- `document_symbols` enforces the server `max_file_bytes` budget before reading file contents; over-budget requests fail as typed `invalid_params` with `path`, `bytes`, `max_bytes`, and `config_max_file_bytes`.
- Blade `document_symbols` responses may include additive `metadata.blade` summaries for normalized template relations, literal Livewire component or `wire:*` discovery, and Flux tag or hint discovery. These summaries are source-only and do not imply Laravel runtime overlays.
- `search_structural` is deterministic tree-sitter query search for Rust, PHP, Blade, TypeScript / TSX, Python, Go, Kotlin / KTS, Lua, Roc, and Nim; `query` must be non-empty and at most `4096` characters, `language` (if provided) must resolve through the shared supported-language alias table, and `path_regex` must satisfy safe-regex validation.
- `search_hybrid` is deterministic hybrid retrieval over lexical + graph + semantic channels, supports optional channel-weight overrides and semantic toggle, and publishes semantic diagnostics in canonical structured `metadata`. The legacy top-level semantic mirrors and JSON-string `note` remain optional compatibility fields in the schema but are omitted from normal live responses.
- `search_hybrid` is the broad natural-language entrypoint for mixed doc/runtime questions and may intentionally diversify top hits across contracts, README, runtime, and tests instead of collapsing to one file class. Ranking stays anchor-first: corroborating anchors strengthen the document score, but the returned `matches[].anchor`, `line`, `column`, and `excerpt` still come from the strongest retained anchor for that document.
- `search_symbol` and the navigation tools exist to beat raw text search when the task is about symbols, definitions, references, implementations, callers, or structural code flow inside attached repositories.
- For implementation-oriented runtime queries such as `initialize`, `subscriptions`, `completion providers`, `handlers`, `transport`, or `resource updated`, `search_hybrid` keeps bounded token recall active and prefers concrete runtime/support/test/example witnesses over repeated generic docs or `composer.json` while remaining mixed-mode overall.
- For Rust daily-work queries that ask where an app starts, wires runtime state, or builds a pipeline/runner object, `search_hybrid` gives extra weight to canonical entrypoints like `src/main.rs` and prefers build-anchor excerpts over fake/mock helper snippets.
- Mixed symbol-plus-intent queries such as `build_pipeline_runner entry point bootstrap` or `ProviderInterface completion providers` now expand identifier overlap terms so exact-anchor runtime families stay above unrelated semantic tail files more reliably.
- When a client needs concrete implementation anchors after `search_hybrid`, follow with `search_symbol` for a known API/type/function name or use `search_text.path_regex` to constrain the witness set to doc/runtime slices explicitly.
- `search_hybrid` strict semantic failures are part of the public contract too: `semantic_status=strict_failure` maps to canonical `unavailable` in [`contracts/errors.md`](../../errors.md).
- `search_hybrid.metadata.semantic_status` reports semantic channel health (`ok`, `disabled`, `unavailable`, `degraded`), while `metadata.semantic_enabled` only turns `true` when at least one returned match keeps a non-zero semantic score after ranking.
- `search_hybrid.metadata.semantic_candidate_count` counts the broader raw semantic chunk candidate pool before retained-hit pruning, `metadata.semantic_hit_count` counts retained semantic documents before document aggregation and final diversification, and `metadata.semantic_match_count` counts returned matches whose semantic score remained non-zero after ranking.
- `search_hybrid.metadata.stage_attribution` is additive diagnostic metadata when present. It reports candidate intake, freshness validation, scan, witness scoring, graph expansion, semantic retrieval, anchor blending, document aggregation, and final diversification timings without changing default result selection semantics.
- When `search_hybrid.metadata.warning` is present, callers should treat the result set as lexical/graph-only or partially semantic rather than as a full semantic ranking. Warnings can also appear for `metadata.semantic_status=ok` when semantic retrieval returned zero hits or when semantic hits existed but none contributed to the returned top results.
- When multi-token natural-language queries underfill exact lexical results, `search_hybrid` deterministically expands lexical recall via bounded exact-token and token-regex recall before ranking. This lexical evidence expansion remains active even when the semantic channel is enabled, including semantic-ok requests whose semantic channel contributes no hits.
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
