# Public Error Taxonomy (`v1`)

This document defines canonical, deterministic error categories for Frigg public MCP tools.

## Classification rules

- Each failing tool call must emit exactly one canonical `error_code` from this document.
- `error_code` must be stable across tools for the same failure class.
- Error responses must include machine-readable details in `error.data` (or equivalent typed payload).

## Canonical error codes

| error_code | Deterministic meaning | Retryability guidance | Typical MCP mapping hint |
| --- | --- | --- | --- |
| `invalid_params` | Request payload is syntactically valid JSON but violates schema/validation rules. | Do not retry unchanged. Retry only after request correction. | JSON-RPC `code: -32602`; include `data.error_code: "invalid_params"` and field-level violations. |
| `resource_not_found` | Requested repo/path/symbol/id does not exist in the visible workspace/index. | Do not blind-retry. Retry only if caller changes identifier or resource is created. | Tool error with `isError=true`; optionally server code (e.g. `-32004`) plus `data.error_code`. |
| `access_denied` | Caller is not allowed to access the requested path or operation by policy/sandbox/security gate. | Do not retry until permissions/policy change. | Tool error with `isError=true`; typically custom server code (e.g. `-32003`) plus `data.error_code`. |
| `confirmation_required` | Write/destructive request omitted explicit confirmation (`confirm=true`) required by policy; no side effects were applied. | Retry only after caller explicitly sets `confirm=true` and revalidates request intent. | Tool error with `isError=true`; typically custom server code (e.g. `-32008`) plus `data.error_code` and confirmation metadata. |
| `timeout` | Operation exceeded enforced time budget (for example expensive regex/search). | Retryable with tighter scope/lower cost; may succeed with smaller input or higher timeout budget if allowed. | Tool error with `isError=true`; custom server code (e.g. `-32070`) plus elapsed/budget metadata. |
| `index_not_ready` | Tool depends on index/artifact state that is missing, stale, or still building. | Retryable after indexing/rebuild completes. | Tool error with `isError=true`; custom server code (e.g. `-32010`) plus readiness state/details. |
| `conflict` | Request conflicts with current resource state (version mismatch, stale precondition, concurrent mutation). | Retryable after refresh/re-read and request recomputation. | Tool error with `isError=true`; custom server code (e.g. `-32009`) plus expected/actual version metadata. |
| `rate_limited` | Request rejected due to configured quota/concurrency/rate control. | Retryable after backoff or window reset. | Tool error with `isError=true`; custom server code (e.g. `-32029`) plus retry-after metadata. |
| `unavailable` | Temporary upstream/dependency/service unavailability. | Retryable with backoff and jitter. | Tool error with `isError=true`; custom server code (e.g. `-32001`) plus dependency identifier. |
| `internal` | Unexpected server-side failure not attributable to caller input. | Retryable for transient faults; escalate if persistent. | JSON-RPC `code: -32603`; include `data.error_code: "internal"` and trace/correlation id. |

## MCP payload guidance

- Preferred shape for tool failures:
- `isError: true`
- `error.code`: JSON-RPC or server-specific numeric code
- `error.message`: short human-readable summary
- `error.data.error_code`: one canonical code from this taxonomy
- `error.data.retryable`: boolean matching this taxonomy and runtime context
- `error.data.details`: machine-readable map (validation issues, path, limits, trace id)

## Per-tool mapping template

Use this template to pin deterministic mappings for each public tool.

| Tool name | Failure condition | Canonical `error_code` | Retryable (`true/false`) | MCP numeric code hint | Required `error.data.details` keys |
| --- | --- | --- | --- | --- | --- |
| `<tool_name>` | `<condition>` | `<error_code>` | `<bool>` | `<code>` | `<key_1>, <key_2>` |
| `<tool_name>` | `<condition>` | `<error_code>` | `<bool>` | `<code>` | `<key_1>, <key_2>` |

## Read-only IDE navigation mappings (`v1`)

| Tool name | Failure condition | Canonical `error_code` | Retryable (`true/false`) | MCP numeric code hint | Required `error.data.details` keys |
| --- | --- | --- | --- | --- | --- |
| `workspace_attach` | empty/invalid attach path payload | `invalid_params` | `false` | `-32602` | `path` |
| `workspace_attach` | attach path escapes canonical ancestor/root validation | `access_denied` | `false` | `-32003` | `path` |
| `read_file` | no repositories attached and no session default available | `resource_not_found` | `false` | `-32004` | `attached_repositories`, `action`, `hint` |
| `search_text` | no repositories attached and no session default available | `resource_not_found` | `false` | `-32004` | `attached_repositories`, `action`, `hint` |
| `search_hybrid` | no repositories attached and no session default available | `resource_not_found` | `false` | `-32004` | `attached_repositories`, `action`, `hint` |
| `search_symbol` | no repositories attached and no session default available | `resource_not_found` | `false` | `-32004` | `attached_repositories`, `action`, `hint` |
| `find_references` | invalid symbol/location payload (`symbol` empty OR missing both symbol and (`path`,`line`) OR empty `path` OR location missing `line`) | `invalid_params` | `false` | `-32602` | `symbol` OR (`path`,`line`) |
| `find_references` | resolved symbol/location missing in scope | `resource_not_found` | `false` | `-32004` | `repository_id`, `symbol` OR (`path`,`line`) |
| `go_to_definition` | `symbol` empty OR missing both symbol and (`path`,`line`) | `invalid_params` | `false` | `-32602` | `symbol` OR (`path`,`line`) |
| `go_to_definition` | resolved symbol/location missing in scope | `resource_not_found` | `false` | `-32004` | `repository_id`, `symbol` OR (`path`,`line`) |
| `find_declarations` | invalid symbol/location payload | `invalid_params` | `false` | `-32602` | `symbol` OR (`path`,`line`) |
| `find_declarations` | resolved symbol/location missing in scope | `resource_not_found` | `false` | `-32004` | `repository_id`, `symbol` OR (`path`,`line`) |
| `find_implementations` | invalid symbol/location payload | `invalid_params` | `false` | `-32602` | `symbol` OR (`path`,`line`) |
| `find_implementations` | resolved symbol/location missing in scope | `resource_not_found` | `false` | `-32004` | `repository_id`, `symbol` OR (`path`,`line`) |
| `incoming_calls` | invalid symbol/location payload | `invalid_params` | `false` | `-32602` | `symbol` OR (`path`,`line`) |
| `incoming_calls` | resolved symbol/location missing in scope | `resource_not_found` | `false` | `-32004` | `repository_id`, `symbol` OR (`path`,`line`) |
| `outgoing_calls` | invalid symbol/location payload | `invalid_params` | `false` | `-32602` | `symbol` OR (`path`,`line`) |
| `outgoing_calls` | resolved symbol/location missing in scope | `resource_not_found` | `false` | `-32004` | `repository_id`, `symbol` OR (`path`,`line`) |
| `document_symbols` | unsupported file extension (non-Rust/PHP/Blade) | `invalid_params` | `false` | `-32602` | `path`, `supported_extensions` |
| `document_symbols` | source file exceeds configured byte budget | `invalid_params` | `false` | `-32602` | `path`, `bytes`, `max_bytes`, `config_max_file_bytes` |
| `document_symbols` | file path/repository not found | `resource_not_found` | `false` | `-32004` | `repository_id`, `path` |
| `search_structural` | empty/oversized query, unsupported language, invalid `path_regex`, invalid tree-sitter query | `invalid_params` | `false` | `-32602` | `query` and/or `language` and/or `path_regex` |
| `search_structural` | repository scope not found | `resource_not_found` | `false` | `-32004` | `repository_id` |

## Hybrid retrieval mappings (`v1`)

These rows are the public contract for `search_hybrid` semantic note metadata (`semantic_status`, `semantic_reason`) and for the strict semantic failure mapping to canonical `unavailable`.

| Tool name | Failure condition | Canonical `error_code` | Retryable (`true/false`) | MCP numeric code hint | Required `error.data.details` keys |
| --- | --- | --- | --- | --- | --- |
| `search_hybrid` | empty query, invalid language filter, invalid channel weights, semantic startup validation failure | `invalid_params` | `false` | `-32602` | `query` and/or `language` and/or `semantic_runtime` |
| `search_hybrid` | strict semantic mode provider failure (`semantic_status=strict_failure`) | `unavailable` | `true` | `-32603` | `semantic_status`, `semantic_reason` |

## Deep-search runtime mappings (`v1`, feature-gated)

| Tool name | Failure condition | Canonical `error_code` | Retryable (`true/false`) | MCP numeric code hint | Required `error.data.details` keys |
| --- | --- | --- | --- | --- | --- |
| `deep_search_run` | unsupported playbook step tool OR invalid step params | `invalid_params` | `false` | `-32602` | `playbook_id`, `step_id`, `tool_name` |
| `deep_search_replay` | unsupported playbook step tool OR invalid step params | `invalid_params` | `false` | `-32602` | `playbook_id`, `step_id`, `tool_name` |
| `deep_search_compose_citations` | malformed trace artifact payload (invalid step `response_json` / missing citation evidence fields) | `invalid_params` | `false` | `-32602` | `playbook_id`, `step_id`, `tool_name` |
| `deep_search_run` | unexpected runtime/harness failure | `internal` | `false` | `-32603` | `playbook_id` |
| `deep_search_replay` | unexpected runtime/harness failure | `internal` | `false` | `-32603` | `playbook_id` |
| `deep_search_compose_citations` | unexpected runtime/harness failure | `internal` | `false` | `-32603` | `playbook_id` |

Deep-search runtime provenance payloads must include deterministic `source_refs.resource_budgets` and `source_refs.resource_usage` arrays. Entries are propagated from per-step tool metadata (for example `find_references` budget notes) and remain present even when deep-search calls fail with typed `invalid_params`.

## Future write-surface guardrails (policy markers)

These markers are release-gated and must remain stable even before write tools exist.

- `write_surface_policy: v1`
- `write_confirm_param: confirm`
- `write_confirm_required: true`
- `write_confirm_failure_error_code: confirmation_required`
- `write_no_side_effect_without_confirm: true`
