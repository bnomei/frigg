---
name: frigg-mcp-search-navigation
description: Use Frigg MCP for repository-aware code discovery and navigation when questions need canonical paths, cross-file movement, symbol or structural search, multi-repository scope, or MCP-backed follow-up. Trigger when users ask cross-file questions, architecture or onboarding questions, refactor impact, symbol or call-flow questions, structural queries, or multi-repository questions that need `workspace_attach`, `search_hybrid`, `search_symbol`, navigation tools, `document_symbols`, `inspect_syntax_tree`, or bounded repository-backed reads.
---

# Frigg MCP Search Navigation

## Choose The Right Surface

Frigg is not the default replacement for every terminal read, but it also no longer needs to be treated as “too heavy” for exact scans by default.

- Prefer local shell tools such as `rg`, `rg --files`, `fd`, `git grep`, `sed`, or `cat` for quick one-off local inspection in the checked-out workspace.
- Reach for Frigg when repository-aware semantics matter: canonical repository-relative paths, cross-file navigation, symbol lookup, structural search, hybrid doc/runtime discovery, bounded repository-backed reads, or multi-repository search.
- Do not avoid `search_text` just because the query is exact. On macOS and Linux, Frigg may use `rg` internally as a lexical accelerator while still preserving repository scope, canonical paths, and downstream navigation flow.
- Treat `workspace_attach` as the explicit setup boundary. Sessions can start detached even when the client is launched inside a repo.

## Default Loop

1. If the task is a simple local read or quick one-off path scan, shell tools are fine.
2. Call `list_repositories`.
3. If no repo is attached, or you want omitted `repository_id` calls to stay local to one repo, call `workspace_attach` explicitly. Use `workspace_current` when you need health, precise, or runtime task status.
4. Start with `search_hybrid` for broad discovery when you do not yet have a stable symbol, string, or path anchor.
5. Pivot to `search_symbol` when you know an API, type, or function name, or to `search_text` when exact strings, canonical paths, `path_regex` scoping, or MCP-backed follow-up matter.
6. Frigg read-only tools default to compact responses. Ask for `response_mode=full` only when you need diagnostics, freshness detail, or selection notes.
7. Use navigation tools for impact and code flow: `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`.
8. Prefer `read_match` when a prior Frigg result already returned `result_handle` plus `match_id`; use `read_file` when you already know the canonical path. Use `explore` when the extended tool profile is enabled and you need probe/zoom/refine follow-up inside one artifact.
9. Use `document_symbols(top_level_only=true)` or `inspect_syntax_tree` before `search_structural` when syntax shape matters more than ranking.

Treat `search_hybrid` as discovery-first. If top-level `warning` is present, top-level `semantic_status != ok`, or `response_mode=full` shows `metadata.lexical_only_mode = true`, treat the ranking as weaker evidence and pivot to more concrete tools before making claims. In lexical-only mode, broad natural-language ranking is noticeably less trustworthy than explicit `search_symbol` or `search_text` queries.

Compact responses still keep the main contract fields, but they intentionally omit bulky `metadata` and `note` payloads. When a tool returns `result_handle` and per-row `match_id` values, prefer `read_match` over manually repeating `path`, `line`, and `column`.

Structural follow-up suggestions are opt-in. Use `include_follow_up_structural=true` when you want replayable `search_structural` follow-ups derived from the resolved AST focus rather than from the user's original query. Phase 1 covers `inspect_syntax_tree` and `search_structural`; phase 2 extends the same typed `follow_up_structural` payloads to `document_symbols`, `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls`. Do not expect this on `search_hybrid` or `search_symbol`.

For technical reviews or blog-style investigations, use this trust order:
- `search_text` for framing and exact narrative anchors
- `read_file` plus defs/refs (`go_to_definition`, `find_declarations`, `find_references`) for proof
- `search_structural` for complex AST-shaped evidence
- `incoming_calls` as a useful call-flow hint
- `outgoing_calls` only as provisional until confirmed elsewhere

## Decision Table

- Simple local file read, file listing, or one-off literal scan with no need for repository-aware semantics: shell tools
- Broad architecture, onboarding, or "where does this live?" questions: `search_hybrid`, but pivot quickly if lexical-only mode is active
- Known API, type, trait, class, or function name: `search_symbol`
- Exact string or regex probe that needs canonical paths, repository scoping, or direct MCP follow-up: `search_text`
- Repository-backed file slice or source proof tied to Frigg results: `read_file`
- Probe, zoom, or refine within one file after discovery: `explore` when the extended profile is enabled
- References, definitions, implementations, callers, or callees: navigation tools
- File outline, AST inspection, or syntax-shape fallback: `document_symbols`, `inspect_syntax_tree`, `search_structural`
- Replayable AST-shaped follow-up probes after an anchored result: re-run the returned `follow_up_structural` suggestion via `search_structural`
- Explicit setup, health, freshness, or precise-generator state: workspace lifecycle tools

## References

- Read [references/workspace-and-runtime.md](references/workspace-and-runtime.md) for `list_repositories`, `workspace_attach`, `workspace_current`, write tools, precise generation, semantic refresh, and runtime status.
- Read [references/discovery-and-evidence.md](references/discovery-and-evidence.md) for `search_hybrid`, `search_symbol`, `search_text`, `read_file`, and `explore`.
- Read [references/navigation-and-structure.md](references/navigation-and-structure.md) for defs/refs/call hierarchy, `document_symbols`, `inspect_syntax_tree`, and `search_structural`.
- Read [references/workflows.md](references/workflows.md) for repeatable investigation loops.
- Read [references/extended-tools.md](references/extended-tools.md) when the extended tool profile is enabled or when a task explicitly calls for deep-search traces or citation composition.
