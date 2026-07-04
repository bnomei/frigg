---
name: frigg-first-code-search
description: Default to Frigg MCP instead of shell `rg`, `grep`, `find`, `fd`, `cat`, or `sed` for source-code discovery, exact text search, symbol lookup, repository-relative file reads, and navigation in any attached or attachable code repository. Use for implementation, debugging, review, refactors, architecture questions, and code Q&A. Shell search is fallback only for non-code files, git state, build output, generated/unindexed files, explicit live-disk verification, or when Frigg is unavailable.
---

# Frigg First Code Search

Frigg is the default for code discovery, file listing, navigation, exact code search, and bounded source reads.

When working in a source-code repository, Frigg is the default search and read surface.

Before using shell `rg`, `grep`, `find`, `fd`, `cat`, or `sed` for code exploration, use the matching Frigg MCP tool in attached or attachable code repositories.

Shell tools are fallback only for git state and diffs, non-code files, build/test output, generated or unindexed files, explicit live-disk verification, or when Frigg is unavailable.

Fallback to shell only for:
- git state and diffs
- non-code files or workspace metadata
- build/test output
- generated or unindexed files
- explicit live-disk verification
- Frigg unavailable

## Shell Replacement Map

- `rg --files` -> `list_files`
- `rg -n "text"` -> `search_text`
- `rg -n "foo|bar"` -> `search_text` with regex mode
- `rg -n "text" path/` -> `search_text` with `path_regex`
- identifier/API/type/class/function lookup -> `search_symbol`
- `cat path` -> `read_file`
- `sed -n '10,80p' path` -> `read_file` with `start_line`, `end_line`, or `line_count`
- follow definitions/references/calls -> navigation tools

Use `search_hybrid` for broad discovery-style questions, not as the cleanest direct-string lookup. When you already have exact text or a regex, use `search_text`; when you already have an identifier, use `search_symbol`.

Frigg may use its native scanner or `rg` internally while still preserving repository scope, canonical paths, and downstream navigation flow.

## Default Loop

1. Omit `repository_id` in normal single-repo work. Frigg uses the session default, adopted repositories, or auto-adopts a sensible default when possible.
2. If Frigg says the session is detached or the default repo is wrong, call `workspace` with `path=<repo root or any file inside it>` or `repository_id=<id>`.
3. Use `list_files` when you would otherwise run `rg --files`, `find`, or `fd`.
4. Use `search_hybrid` for broad discovery when you do not yet have a stable symbol, string, or path anchor.
5. Use `search_symbol` when you know an API, type, or function name, or `search_text` when exact strings, safe regexes, grouped alternation, canonical paths, `path_regex` scoping, or MCP-backed follow-up matter.
6. Frigg read-only tools default to compact responses. Ask for `response_mode=full` only when you need diagnostics or selection notes.
7. Use navigation tools for impact and code flow: `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`.
8. Prefer `read_match` when a prior Frigg result already returned `result_handle` plus `match_id`; use `read_file` when you already know the canonical path. Both default to text-first source output, so ask for `presentation_mode=json` only when you truly need the structured compatibility payload. Use `explore` when the extended tool profile is enabled and you need probe/zoom/refine follow-up inside one artifact.
9. Use `document_symbols(top_level_only=true)` or `inspect_syntax_tree` before `search_structural` when syntax shape matters more than ranking.

Treat `search_hybrid` as discovery-first. It is allowed to be less precise than exact tools because it is ranking candidate pivots for broad questions, not replacing direct string or symbol lookup. In normal compact responses, use matches as candidate pivots and move to `search_symbol`, `search_text`, `read_file`, or navigation for proof. Ask for `response_mode=full` only when you are diagnosing ranking behavior; if full metadata shows ranking notes or lexical-only ranking, pivot to exact tools sooner.

Compact responses still keep the main contract fields, but they intentionally omit bulky `metadata` and `note` payloads. `read_file`, `read_match`, and `explore(operation=zoom)` return only selected source bytes by default; request `presentation_mode=json` when path, line, byte, context-efficiency, or machine-readable `content` fields are required. When a tool returns `result_handle` and per-row `match_id` values, prefer `read_match` over manually repeating `path`, `line`, and `column`.

Structural follow-up suggestions are opt-in. Use `include_follow_up_structural=true` when you want replayable `search_structural` follow-ups derived from the resolved AST focus rather than from the user's original query. Phase 1 covers `inspect_syntax_tree` and `search_structural`; phase 2 extends the same typed `follow_up_structural` payloads to `document_symbols`, `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls`. Do not expect this on `search_hybrid` or `search_symbol`.

For technical reviews or blog-style investigations, use this trust order:
- `search_text` for framing and exact narrative anchors
- `read_file` plus defs/refs (`go_to_definition`, `find_declarations`, `find_references`) for proof
- `search_structural` for complex AST-shaped evidence
- `incoming_calls` as a useful call-flow hint
- `outgoing_calls` only as provisional until confirmed elsewhere

## Decision Table

- Git state and diffs, non-code files, build/test output, generated/unindexed files, explicit live-disk verification, or unavailable Frigg results: shell tools
- `rg --files`/`find`/`fd`-shaped repository file listing: `list_files`
- Broad architecture, onboarding, or "where does this live?" questions without a stable anchor: `search_hybrid`, but pivot quickly if lexical-only mode is active
- Known API, type, trait, class, or function name: `search_symbol`
- Exact string, safe-regex, grouped-alternation, or `rg`-shaped probe that needs canonical paths, repository scoping, or direct MCP follow-up: `search_text`
- Repository-backed file slice or source proof tied to Frigg results: `read_file`
- Probe, zoom, or refine within one file after discovery: `explore` when the extended profile is enabled
- References, definitions, implementations, callers, or callees: navigation tools
- File outline, AST inspection, or syntax-shape fallback: `document_symbols`, `inspect_syntax_tree`, `search_structural`
- Replayable AST-shaped follow-up probes after an anchored result: re-run the returned `follow_up_structural` suggestion via `search_structural`
- Explicit workspace status or target adoption: `workspace`

## References

- Read [references/workspace-and-runtime.md](references/workspace-and-runtime.md) for `workspace`, auto-adoption, precise generation, semantic refresh, and runtime status.
- Read [references/discovery-and-evidence.md](references/discovery-and-evidence.md) for `search_hybrid`, `search_symbol`, `search_text`, `read_file`, and `explore`.
- Read [references/navigation-and-structure.md](references/navigation-and-structure.md) for defs/refs/call hierarchy, `document_symbols`, `inspect_syntax_tree`, and `search_structural`.
- Read [references/workflows.md](references/workflows.md) for repeatable investigation loops.
- Read [references/extended-tools.md](references/extended-tools.md) when the extended tool profile is enabled or when a task explicitly calls for playbook traces or citation composition.
