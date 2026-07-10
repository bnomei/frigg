# Frigg Workflows

Use the lightest tool that preserves the right semantics. Shell tools remain correct for git, build/test output, generated/unindexed paths, explicit live-disk checks, and Frigg-unavailable cases. Frigg is the default for indexed source discovery, search, navigation, and proof. Canonical scenario routing lives in the skill top screen (`SKILL.md`); this file expands narrative loops only.

Do not use shell `rg` as a "throwaway check" on indexed source while Frigg is attached — use scoped `search_text` instead.

## Bug Trace

1. `workspace` only if adoption or repo default is uncertain (gate, not preamble)
2. `search_text` on the exact error fragment with `path_regex` such as `^(src|tests)/`
3. `search_text` on the test name when the failure is test-driven
4. `search_symbol` for the stack-frame or central API (`path_class=runtime`)
5. `find_references` or `incoming_calls` for impact
6. Use compact responses first; `response_mode=full` only for diagnostics
7. `read_match` on the strongest witnesses when a prior Frigg result gave `result_handle` plus `match_id`; otherwise `read_file`
8. Use `search_hybrid` only when the symptom has no exact string
9. If call hierarchy or nav underfills, check `mode`, recovery fields, and `workspace` before assuming the code path is absent

## Refactor Impact

1. `search_symbol` for the API to change (`path_class=runtime`)
2. `find_references` for call sites (`include_definition=false` when listing usages)
3. `find_implementations` when the change hits an interface or trait boundary
4. `incoming_calls` for caller graph; treat `outgoing_calls` as provisional until body proof
5. `read_match` / `read_file` on each cluster before editing
6. Optional `search_text` with `path_regex='^tests/'` for an explicit test pass
7. Prefer `impact_bundle` when available; individual tools remain source of truth
8. Never shell `rg` for throwaway indexed-source checks — use scoped `search_text`
9. Do not expect other scenario “bundle” tools — composition beyond impact is skill-side

## Technical Review

1. `search_text` for the contract phrase, API name, or narrative anchor
2. `search_symbol` → `go_to_definition(symbol=…)` for the implementation anchor (`find_declarations` only when decl≠def matters)
3. `find_references` and `incoming_calls` for propagation and entry paths
4. `read_match` or `read_file` for final source proof; attach path/line witnesses (skill-assembled evidence packet when multi-claim — not an MCP tool)
5. `search_structural` only for cross-cutting AST proof that is too awkward in plain text (tier-3)
6. Treat `outgoing_calls` as provisional until another tool confirms the edge
7. Git diff and build/test output stay shell-owned; return to Frigg for source impact

## Onboarding And Architecture

1. `search_hybrid` with the feature or subsystem question
2. Treat mixed docs, tests, and runtime hits as expected discovery noise
3. Do **not** answer from hybrid rank-1 alone — pivot to `search_symbol` / `search_text`
4. Use `go_to_definition` or `document_symbols(top_level_only=true)` to pin entrypoints after exact anchors exist

## Multi-Repository Investigation

1. `workspace(path=...)` when session default may not match the task repo
2. Re-anchor with explicit `repository_id` once the target repo is clear
3. Search and navigate with the adopted default or explicit id; wrong-repo zeros → workspace recovery, not shell grep
4. Use `read_file` or navigation on the resolved repo-specific paths

## Structural Query Recovery

1. `document_symbols` or `read_file` on a representative file
2. `inspect_syntax_tree` on the actual cursor location (**line and column together**; `column: 1` if unknown)
3. Write the `search_structural` query from real node kinds, not guessed shapes
4. Add `path_regex` when the scan should stay inside one slice
5. Tier-3 only — not a substitute for `search_text` / `search_symbol`

## Security Or Pattern Sweep

1. Prefer `search_batch` with multiple independent text/symbol (and optional hybrid) probes when available — concurrent full searches, then merge (not one fused multi-query walk)
2. Prefer `search_batch`; only if missing from tools/list, same-turn parallel Frigg `search_text` / `search_symbol` probes (not parallel shell grep)
3. Scope runtime with `path_regex` / `path_class`; upgrade to safe regex only when literal underfills
4. `read_match` or `read_file` to validate true positives on indexed source
5. `find_references` or call hierarchy to measure blast radius of confirmed sinks
6. Package multi-finding reports as skill-assembled evidence packets with path/line witnesses (schema: `frigg://policy/evidence-packet.json`)
