---
name: frigg-mcp-search-navigation
description: Use Frigg MCP for repository-aware code discovery and navigation when shell tools are not enough. Trigger when users ask cross-file questions, architecture or onboarding questions, refactor impact, symbol or call-flow questions, structural queries, or multi-repository questions that need `workspace_attach`, `search_hybrid`, `search_symbol`, navigation tools, `document_symbols`, `inspect_syntax_tree`, or bounded repository-backed reads.
---

# Frigg MCP Search Navigation

## Shell First

Frigg is not the default replacement for local shell inspection.

- Prefer local shell tools such as `rg`, `rg --files`, `fd`, `git grep`, `sed`, or `cat` for simple file listing, literal search, and short direct reads in the checked-out workspace.
- Reach for Frigg when repository-aware semantics matter: canonical repository-relative paths, cross-file navigation, symbol lookup, structural search, hybrid doc/runtime discovery, bounded repository-backed reads, or multi-repository search.
- Treat `workspace_attach` as the explicit setup boundary. Sessions can start detached even when the client is launched inside a repo.

## Default Loop

1. If the task is a simple local read or literal scan, use shell tools first.
2. Call `list_repositories`.
3. If no repo is attached, or you want omitted `repository_id` calls to stay local to one repo, call `workspace_attach` explicitly. Use `workspace_current` when you need health, precise, or runtime task status.
4. Start with `search_hybrid` for broad natural-language doc/runtime questions.
5. Pivot to `search_symbol` when you know an API, type, or function name, or to `search_text` when exact strings or `path_regex` scoping matter.
6. Use navigation tools for impact and code flow: `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`.
7. Use `read_file` for bounded repository-backed confirmation. Use `explore` when the extended tool profile is enabled and you need probe/zoom/refine follow-up inside one artifact.
8. Use `document_symbols` or `inspect_syntax_tree` before `search_structural` when syntax shape matters more than ranking.

Treat `search_hybrid` as discovery-first. If `metadata.warning` is present, `semantic_status != ok`, or `metadata.lexical_only_mode = true`, treat the ranking as weaker evidence and pivot to more concrete tools before making claims.

Structural follow-up suggestions are opt-in. Use `include_follow_up_structural=true` when you want replayable `search_structural` follow-ups derived from the resolved AST focus rather than from the user's original query. Phase 1 covers `inspect_syntax_tree` and `search_structural`; phase 2 extends the same typed `follow_up_structural` payloads to `document_symbols`, `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls`. Do not expect this on `search_hybrid` or `search_symbol`.

## Decision Table

- Simple local file read, file listing, or literal scan with no need for repository-aware semantics: shell tools
- Broad architecture, onboarding, or "where does this live?" questions: `search_hybrid`
- Known API, type, trait, class, or function name: `search_symbol`
- Exact string or regex probe that needs canonical paths or repository scoping: `search_text`
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
