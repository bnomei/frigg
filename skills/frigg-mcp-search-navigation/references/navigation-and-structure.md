# Navigation And Structure

## General Rules

- Navigation tools can resolve by `symbol` or by source location.
- Source locations use canonical repository-relative `path` plus 1-based `line` and `column`.
- Read the top-level `mode` first:
  - `precise`
  - `precise_partial`
  - `heuristic_no_precise`
  - `unavailable_no_precise`

Treat `heuristic_no_precise` as useful but weaker. Treat `unavailable_no_precise` as an honest “not enough precise data” signal, not as proof that nothing exists.

`include_follow_up_structural=true` is an opt-in across the structure-aware surfaces below. When enabled, Frigg attaches typed `follow_up_structural` suggestions that replay into `search_structural`. These are best-effort AST follow-ups, not echoes of the original query. Phase 1 is `inspect_syntax_tree` plus `search_structural`; phase 2 is `document_symbols`, `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls`. `search_hybrid` and `search_symbol` do not expose this payload.

## `find_references`

Important inputs:
- `symbol`
- `repository_id`
- `path`
- `line`
- `column`
- `include_definition`
- `include_follow_up_structural`
- `limit`

Important outputs:
- `total_matches`
- `matches`
- `mode`
- `metadata`
- `note`

Each reference hit carries `match_kind`, so distinguish:
- `definition`
- `declaration`
- `reference`

When opted in, each match may also carry `follow_up_structural` suggestions for replayable structural narrowing.

Use `include_definition=false` when you want caller or usage sites without the defining row mixed in.

## `go_to_definition`, `find_declarations`, `find_implementations`

Use these when you have a concrete symbol or a concrete cursor location.

Common inputs:
- `symbol`
- `repository_id`
- `path`
- `line`
- `column`
- `include_follow_up_structural`
- `limit`

Common outputs:
- `matches`
- `mode`
- `metadata`
- `note`

Inspect per-match precision hints when present:
- `precision`
- `fallback_reason`
- `relation`
- `follow_up_structural`

If a location-based jump underfills, try:
- `search_symbol`
- `document_symbols`
- `find_references`
- a tighter source location on the actual token

For `find_implementations`, be more cautious on generic traits or blanket impl patterns. `heuristic_no_precise` can still be useful, but it is weaker evidence than a concrete precise implementation edge.

## `incoming_calls` and `outgoing_calls`

Call hierarchy is the most precise-data-sensitive part of Frigg.

Important outputs:
- `matches`
- `mode`
- `availability`
- `metadata`
- `note`

When opted in, each match may also include `follow_up_structural`.

Read `availability` before treating empty matches as meaningful. If `availability.status` says the result is unavailable without precise data, say that explicitly.

Trust guidance:
- `incoming_calls` is often good enough to map believable entry paths
- `outgoing_calls` is currently the more error-prone side of the stack, so confirm suspicious callees with `read_file`, `find_references`, or `search_structural` before asserting the edge

## `document_symbols`

Use `document_symbols` for a hierarchical file outline.

Best use cases:
- overloaded names
- large files
- finding the enclosing class / impl / function before a follow-up jump

When opted in, each returned symbol item may include `follow_up_structural` suggestions you can replay with `search_structural`.

## `inspect_syntax_tree`

Use `inspect_syntax_tree` before `search_structural` whenever the node shape is unclear.

Important inputs:
- `path`
- `repository_id`
- `line`
- `column`
- `max_ancestors`
- `max_children`
- `include_follow_up_structural`

Important outputs:
- `language`
- `focus`
- `ancestors`
- `children`
- `follow_up_structural`

This is the safest way to learn the real Tree-sitter node kinds before writing a structural query.

Practical caution:
- it is cursor-sensitive
- if the focus lands on punctuation or an unexpected wrapper node, move the cursor onto the identifier or call token and retry

## `search_structural`

Use `search_structural` when syntax shape matters more than ranking or symbol metadata.

Important inputs:
- `query`
- `language`
- `repository_id`
- `path_regex`
- `limit`
- `result_mode`
- `primary_capture`
- `include_follow_up_structural`

Important outputs:
- `matches`
- `result_mode`
- `metadata`
- `note`

When opted in, each structural match may also include `follow_up_structural`.

Practical rules:
- grouped match rows are the default; use `result_mode=captures` when you need raw capture rows
- use `primary_capture` when your query includes helper captures but you want one specific capture to anchor the visible row
- read `anchor_capture_name`, `anchor_selection`, and `captures` before assuming the visible row tells the whole structural story
- the query must be a valid Tree-sitter query for the target language grammar
- if Frigg reports an impossible pattern or invalid query, inspect the AST first
- use `path_regex` to keep structural scans bounded
- for complex proof queries, `search_structural` is often more reliable than call-graph inference

Example flow:
- Query: `(function_item name: (identifier) @name) @match`
- Default grouped result: one row per function, typically anchored to `@match`
- If you want the function name token as the visible row: set `primary_capture=name`
- If you want every capture as its own row to debug the query: set `result_mode=captures`
