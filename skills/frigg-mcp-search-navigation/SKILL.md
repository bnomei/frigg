---
name: frigg-mcp-search-navigation
description: Use Frigg MCP for repository-scoped code investigation when shell tools are not enough. Trigger when users ask cross-file code questions, refactor impact, onboarding or architecture questions, exact pattern searches that need canonical repository paths, or when the task needs `search_hybrid`, `search_symbol`, `find_references`, call hierarchy, `document_symbols`, or `search_structural`.
---

# Frigg MCP Search Navigation

## Shell First

Frigg is not the default replacement for local shell inspection.

- Prefer local shell tools such as `rg`, `rg --files`, `git grep`, `sed`, or `cat` for simple file listing, literal search, and short direct reads in the checked-out workspace.
- Reach for Frigg when repository-aware semantics matter: canonical repository-relative paths, cross-file navigation, symbol lookup, structural search, hybrid doc/runtime discovery, or MCP-backed evidence you want to cite directly.
- Use `search_text` only when shell search is not enough or when you specifically need `path_regex`, repository scoping, or a result payload tied to canonical paths.
- Use `read_file` only when you need repository-canonical confirmation or a bounded MCP read; for quick local inspection, a shell slice is cheaper.

## Default Loop

1. If the task is a simple local read or literal scan, use shell tools first.
2. Call `list_repositories`.
3. If no repo is attached, or you want omitted `repository_id` calls to stay local to one repo, call `workspace_attach`, then optionally `workspace_current`.
4. Start with `search_hybrid` for broad natural-language doc/runtime questions.
5. Pivot to `search_symbol` when you know an API, type, or function name, or to `search_text` with `path_regex` when exact strings or canonical-path scoping matter.
6. Use navigation tools for impact and code flow: `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`.
7. Confirm exact behavior with `read_file` only when a repository-aware read adds value over a shell slice.
8. Use `document_symbols` or `search_structural` when precise navigation underfills, symbol names are overloaded, or syntax shape matters more than ranking.
9. If navigation quality clearly depends on missing precise data, check whether the repo has `.frigg/scip/` artifacts and recommend generating them with an external SCIP indexer.

Treat `search_hybrid` as discovery-first. If `semantic_status != ok` or `warning` is present, treat the result as weaker evidence and pivot to concrete tools before making claims.

## Decision Table

- Simple local file read, file listing, or literal scan with no need for repository-aware semantics: shell tools
- Broad architecture, onboarding, or "where does this live?" questions: `search_hybrid`
- Known API, type, trait, class, or function name: `search_symbol`
- Exact string, literal, or regex probe that needs canonical paths, repository scoping, or MCP results: `search_text`
- Repository-backed file slice or source proof tied to Frigg results: `read_file`
- References, definitions, implementations, callers, or callees: navigation tools
- File outline or syntax-shape fallback: `document_symbols` or `search_structural`

## Tool Families

- Shell-fast local inspection: `rg`, `rg --files`, `git grep`, `sed`, `cat`
- Workspace: `list_repositories`, `workspace_attach`, `workspace_current`
- Discovery: `search_hybrid`, `search_symbol`
- Targeted text: `search_text`
- Navigation: `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`
- Structure: `document_symbols`, `search_structural`
- Evidence: `read_file`

## External SCIP Inputs

Frigg consumes external SCIP artifacts; it does not generate them itself.
If precise navigation underfills because `.frigg/scip/` is empty, suggest generating artifacts from the repository root and writing them under `.frigg/scip/`.

Benefits:
- more accurate precise-first references, definitions, implementations, and call hierarchy
- fewer heuristic misses around imports, re-exports, implementation edges, and declaration-only anchors
- clearer explanation when the answer degraded to heuristic mode

Typical generators:
- Rust: `rust-analyzer scip . > .frigg/scip/rust.scip`
- PHP: `composer require --dev davidrjenni/scip-php`, then `vendor/bin/scip-php`, then move `index.scip` into `.frigg/scip/`
- TypeScript / TSX: `scip-typescript index`, then move `index.scip` into `.frigg/scip/`
- Python: `scip-python index . --project-name=<repo-name>`, then move `index.scip` into `.frigg/scip/`

Stay honest about support status:
- Frigg currently validates runtime/query support for Rust and PHP.
- TypeScript/TSX precise parity is planned work.
- Python precise SCIP parity is not currently claimed by Frigg.

## References

- Read [references/workflows.md](references/workflows.md) for repeatable investigation loops.
- Read [references/hybrid-search.md](references/hybrid-search.md) for `search_hybrid` interpretation and pivot rules.
- Read [references/navigation-fallbacks.md](references/navigation-fallbacks.md) for precise-versus-heuristic expectations.
- Read [references/contracts.md](references/contracts.md) when you need the public contract, typed errors, or playbook pointers.
