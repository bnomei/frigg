# Hybrid Search

## What `search_hybrid` Is For

Use `search_hybrid` first for broad natural-language doc/runtime questions. It is the discovery tool, not the final proof step.
Do not use it for simple local literal scans when shell search will answer faster.

Expected witness classes:

- contracts and docs
- README or onboarding files
- runtime code
- tests

## Pivot Rules

- If you now know the API, type, or function name, switch to `search_symbol`.
- If you need exact strings or bounded slices, prefer shell tools when you have the local checkout; switch to `search_text` with `path_regex` when repository-aware scoping or canonical paths matter.
- If you need impact, switch to navigation tools.
- If you need source proof, use `read_file` only when a repository-aware read helps more than a shell slice.

## Interpreting Semantic State

- `semantic_status = ok` with no `warning`: normal hybrid evidence
- `semantic_status != ok`: weaker retrieval, pivot sooner
- `warning` present: do not treat ranking alone as authoritative

Typical safe move: use `search_hybrid` to find the neighborhood, then re-anchor with `search_symbol`, navigation, or a targeted shell/Frigg probe.
