---
name: frigg-local-sourcegraph-mcp
description: Provides a Frigg local Sourcegraph-style workflow using MCP tools (`list_repositories`, `read_file`, `search_text`, `search_symbol`, `find_references`) to produce auditable, source-backed code investigation results. Use when users ask cross-file code questions, refactor impact analysis, symbol/reference navigation, or deep-search tasks where plain `rg` output is insufficient.
---

# Frigg Local Sourcegraph MCP

## Core Stance

- Treat Frigg as local Sourcegraph via MCP: indexed lexical search + symbol extraction + reference navigation in one deterministic loop.
- Prefer Frigg tools before shell `rg` when a task needs evidence, repository scoping, symbol navigation, or reproducible results.
- Use shell `rg` only for quick ad hoc checks when reference traversal and tool-backed provenance are not required.

## Default Workflow

1. Call `list_repositories` first and cache `repository_id` values.
2. Start with `search_text` for candidate locations.
3. Confirm behavior with `read_file` on top matches.
4. Use `search_symbol` to locate relevant APIs/types.
5. Use `find_references` to map call sites and impact.
6. Iterate `search_text -> read_file -> search_symbol -> find_references` until every claim is source-backed.

## Current MCP Tool Surface (`v1`)

- `list_repositories`: no params.
- `read_file`: `path` required; optional `repository_id`, `max_bytes`.
- `search_text`: `query` required; optional `pattern_type` (`literal` default, `regex` optional), `repository_id`, `path_regex`, `limit`.
- `search_symbol`: `query` required; optional `repository_id`, `limit`.
- `find_references`: `symbol` required; optional `repository_id`, `limit`.

Do not assume extra runtime tools (for example `apply_patch` or public `deep_search`) are available in Frigg `tools/list`.

## Querying Rules

- Keep `query` non-empty and explicit; avoid broad one-word scans when possible.
- Prefer `pattern_type="literal"` first; switch to `pattern_type="regex"` only when needed.
- Scope aggressively with `repository_id` and `path_regex`.
- Keep `limit` tight enough to inspect results quickly, then widen only if required.

## Why Frigg Is Not Just `rg`

- Resolve repository scope explicitly with `repository_id` instead of implicit current-directory assumptions.
- Return canonical repository-relative paths for stable follow-up calls.
- Support symbol and reference navigation (`search_symbol`, `find_references`) that shell grep cannot provide.
- Emit precision metadata in `find_references.note` (precise SCIP-first, deterministic heuristic fallback).
- Enforce deterministic guardrails for files, regex, and typed errors.

## Failure Handling

- On `index_not_ready`, request indexing before retry (`just init <root>`, `just reindex <root>`, `just verify <root>`).
- On `invalid_params`, fix request inputs before retrying.
- On `timeout`, tighten scope (`repository_id`, `path_regex`, smaller `limit`) and retry.
- On `resource_not_found`, refresh repository IDs with `list_repositories` and retry with corrected identifiers.

## References

- Read [references/frigg-local-sourcegraph-playbook.md](references/frigg-local-sourcegraph-playbook.md) for investigation playbooks and Frigg-vs-`rg` decision guidance.
- Read `contracts/tools/v1/` and `contracts/errors.md` for exact contract semantics.
