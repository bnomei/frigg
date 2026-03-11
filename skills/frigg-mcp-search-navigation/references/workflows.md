# Frigg Workflows

Use shell tools first for cheap local reads and literal scans. The Frigg loops below start once repository-aware search or navigation is justified.

## Bug Trace

1. `list_repositories`
2. If the session is detached or the default repo is wrong, call `workspace_attach` explicitly
3. `search_hybrid` for the failure symptom
4. `search_symbol` for the central API or type
5. `find_references` or call hierarchy for impact
6. `read_file` on the strongest witnesses only when you need canonical-path evidence; otherwise confirm with a shell slice

## Refactor Impact

1. `search_symbol` for the API to change
2. `find_references` for call sites
3. `find_implementations` when the change hits an interface or trait boundary
4. Use a shell slice or `read_file` for the high-risk sites depending on whether repository-aware evidence matters
5. Prefer shell `rg` or `git grep` for nearby patterns or config strings; use `search_text` with `path_regex` when you need canonical paths or scoped MCP results

## Onboarding And Architecture

1. `search_hybrid` with the feature or subsystem question
2. Treat mixed docs, README, contracts, tests, and runtime hits as expected
3. Pivot to `search_symbol` once the likely runtime anchor is visible
4. Use `go_to_definition` or `document_symbols` to pin the actual implementation entrypoints

## Security Or Pattern Sweep

1. Prefer shell `rg` or `git grep` with a narrow literal first
2. Upgrade to regex only when the literal underfills
3. Use `search_text` with `path_regex` when you need repository scoping or canonical-path results
4. Use a shell slice or `read_file` to validate true positives
5. `find_references` or call hierarchy to measure blast radius
