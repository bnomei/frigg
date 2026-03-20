# Frigg Workflows

Use the lightest tool that preserves the right semantics. Shell tools are still good for quick local inspection, but Frigg is now also a reasonable exact-search surface when you want repository scoping, canonical paths, or direct MCP follow-up.

## Bug Trace

1. `list_repositories`
2. If the session is detached or the default repo is wrong, call `workspace_attach` explicitly
3. `search_hybrid` for the failure symptom
4. `search_symbol` for the central API or type
5. `find_references` or call hierarchy for impact
6. `read_file` on the strongest witnesses only when you need repository-backed evidence; otherwise a shell slice is still fine
7. If call hierarchy or nav underfills, check `mode`, `availability`, and `workspace_current.precise` before assuming the code path is absent

## Refactor Impact

1. `search_symbol` for the API to change
2. `find_references` for call sites
3. `find_implementations` when the change hits an interface or trait boundary
4. Use a shell slice or `read_file` for the high-risk sites depending on whether repository-aware evidence matters
5. Use `search_text` with `path_regex` when canonical paths, scoped MCP results, or direct follow-up matter; shell `rg` or `git grep` is still fine for nearby throwaway pattern checks

## Onboarding And Architecture

1. `search_hybrid` with the feature or subsystem question
2. Treat mixed docs, tests, and runtime hits as expected
3. Pivot to `search_symbol` once the likely runtime anchor is visible
4. Use `go_to_definition` or `document_symbols` to pin the actual implementation entrypoints

## Multi-Repository Investigation

1. `list_repositories`
2. `workspace_attach` the main repo you want as the session default
3. Use `search_hybrid` or `search_symbol` without `repository_id` when the question may cross repo boundaries
4. Re-anchor with explicit `repository_id` once the target repo is clear
5. Use `read_file` or navigation tools on the resolved repo-specific paths

## Structural Query Recovery

1. `document_symbols` or `read_file` on a representative file
2. `inspect_syntax_tree` on the actual cursor location
3. Write the `search_structural` query from real node kinds, not guessed shapes
4. Add `path_regex` when the scan should stay inside one slice

## Security Or Pattern Sweep

1. Start with either a narrow shell literal or `search_text`, depending on whether you want repository-backed results and follow-up
2. Upgrade to regex only when the literal underfills
3. Use `search_text` with `path_regex` when you need repository scoping or canonical-path results
4. Use a shell slice or `read_file` to validate true positives
5. `find_references` or call hierarchy to measure blast radius
