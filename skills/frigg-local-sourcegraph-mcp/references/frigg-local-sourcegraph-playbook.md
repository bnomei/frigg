# Frigg Local Sourcegraph Playbook

## What Makes Frigg Special

Frigg is a local, deterministic MCP server for code investigation:

- It combines text search, symbol search, and reference traversal.
- It works across multiple configured repositories with explicit `repository_id` scoping.
- It returns canonical repository-relative paths suitable for chained tool calls.
- It records deterministic provenance for tool calls, enabling source-backed answers.
- It prefers precise SCIP references in `find_references`, with a deterministic heuristic fallback when precise data is absent.

## Frigg vs `rg`: Decision Matrix

Use Frigg when you need:

- Cross-file and cross-repository investigation.
- Symbol-level discovery (`search_symbol`).
- Impact analysis (`find_references`).
- Reproducible, auditable evidence from MCP tool outputs.

Use shell `rg` when you need:

- A one-off local text check in your current directory.
- Very fast ad hoc triage where navigation/provenance are not required.

## Canonical Investigation Loops

### Loop A: Bug Trace

1. `list_repositories`
2. `search_text` for failing behavior keywords
3. `read_file` for top matches
4. `search_symbol` for central functions/types
5. `find_references` to follow callers and affected sites
6. Repeat with narrower queries until root cause is evidenced

### Loop B: Refactor Impact

1. `list_repositories`
2. `search_symbol` for the API to change
3. `find_references` for all call sites
4. `read_file` for high-risk call sites
5. `search_text` with `path_regex` to verify related patterns

### Loop C: Security Sweep

1. `search_text` with regex for dangerous sinks/sources
2. `read_file` to validate true positives
3. `search_symbol` to locate auth/sanitization helpers
4. `find_references` to check protection coverage

## Request Patterns

### `search_text` literal-first

```json
{
  "query": "issue_jwt",
  "pattern_type": "literal",
  "repository_id": "repo-001",
  "path_regex": "^src/",
  "limit": 30
}
```

### `search_text` regex when needed

```json
{
  "query": "auth(enticate|entication)|session|jwt",
  "pattern_type": "regex",
  "repository_id": "repo-001",
  "limit": 50
}
```

### `search_symbol` and `find_references`

```json
{
  "query": "AuthService",
  "repository_id": "repo-001",
  "limit": 20
}
```

```json
{
  "symbol": "AuthService::login",
  "repository_id": "repo-001",
  "limit": 200
}
```

## Anti-Patterns To Avoid

- Do not skip `list_repositories` and assume stale `repository_id` values.
- Do not use unconstrained regex scans first when a literal query can narrow scope.
- Do not stop at `search_text` snippets for impact questions; run `find_references`.
- Do not treat `rg` output as final evidence for cross-file behavior without Frigg tool confirmation.

## Operational Notes

- `search_text.pattern_type` currently supports `literal` and `regex`.
- `list_repositories` IDs (`repo-001`, `repo-002`, ...) are stable only for the current workspace-root ordering.
- Tighten `path_regex` and `limit` before retrying after `timeout`.
- On `index_not_ready`, run indexing (`just init`, `just reindex`, `just verify`) before retrying tool calls.
