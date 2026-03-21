# Discovery And Evidence

## `search_hybrid`

Use `search_hybrid` first for broad discovery when you do not yet have a stable symbol, string, or path anchor. It is the discovery surface, not the final proof step.

Important inputs:
- `query`
- `repository_id`
- `language`
- `limit`
- `weights`
- `semantic`

Important output shape:
- `matches[]`
- `result_handle`
- optional top-level compatibility mirrors such as `semantic_status` and `warning`
- `metadata` only when `response_mode=full`

Compact-first rule:
- read-only search tools default to compact responses
- ask for `response_mode=full` only when you need diagnostics, freshness detail, or channel-level reasoning
- in compact mode, use `result_handle` plus per-row `match_id` values to continue with `read_match`

What to inspect on each match:
- `path`
- `line` / `column`
- `excerpt`
- `blended_score`
- `lexical_score`
- `graph_score`
- `semantic_score`
- `path_class`
- `source_class`
- `surface_families`
- `navigation_hint`

What to inspect in `metadata`:
- `channels`
- `semantic_status`
- `semantic_reason`
- `lexical_only_mode`
- `warning`
- `semantic_capability`
- `utility`
- `freshness_basis`

Interpretation rules:
- `warning` present: ranking is weaker than normal, pivot sooner
- `semantic_status != ok`: semantic is missing, disabled, or degraded
- `lexical_only_mode = true`: broad natural-language ranking is weaker; use matches as candidate pivots and move to `search_symbol`, `search_text`, `read_file`, or navigation sooner
- `utility.best_pivot_*`: good hint for the first file to open next

Typical next move:
- `search_symbol` when you now know the symbol
- `search_text` when you need exact strings or path scoping
- navigation tools when you need defs/refs/calls
- `read_match` when you already have a concrete hit row
- `read_file` when you need repository-backed proof and already know the canonical path

## `search_symbol`

Use `search_symbol` when you know the API, type, function, method, trait, class, or module name.

Important inputs:
- `query`
- `repository_id`
- `path_class`
  - `runtime`
  - `support`
  - `project`
- `path_regex`
- `limit`

Use `path_class` or `path_regex` when overloaded names are noisy.

Compact-first rule:
- default responses omit `metadata` and `note`
- use `response_mode=full` when you need ranking or freshness detail
- compact responses still return `result_handle` and row `match_id` values for `read_match`

Practical caution:
- inline test modules can still overmatch inside runtime files, even under `path_regex:"^src/"` or `path_class:"runtime"`
- treat `search_symbol` as a candidate locator, then confirm the specific runtime anchor with `go_to_definition`, `document_symbols`, or `read_file`

## `search_text`

Use `search_text` when you need exact or regex search plus Frigg semantics:
- canonical repository-relative paths
- repository scoping
- regex search over indexed files
- `path_regex` narrowing
- easy pivoting into `read_file`, navigation, or other MCP-backed follow-up

Notes:
- on macOS and Linux, Frigg may use `rg` internally as a lexical accelerator when it is available
- that does not change the public flow: Frigg still owns candidate scope, ordering, metadata, and fallback behavior
- for review-style work, `search_text` is often the best first proof surface when the repo has stable narrative terms, API names, or deterministic contract phrases

Important inputs:
- `query`
- `pattern_type`
  - `literal`
  - `regex`
- `repository_id`
- `path_regex`
- `limit`
- `context_lines`
- `max_matches_per_file`
- `collapse_by_file`
- `response_mode`

Shaping guidance:
- `context_lines` is the cheap first-pass alternative to a separate read for small review windows
- `max_matches_per_file` keeps one noisy file from dominating the result set
- `collapse_by_file=true` is the quickest way to reduce repeated-path spam
- compact responses still return `result_handle` and row `match_id` values so you can reopen one hit with `read_match`

## `read_match`

Use `read_match` when a prior search or navigation response already returned a `result_handle` plus `match_id` and you want a bounded source window without manually repeating path and line data.

Important inputs:
- `result_handle`
- `match_id`
- `before`
- `after`

Important outputs:
- `repository_id`
- `path`
- `line`
- `column`
- `line_start`
- `line_end`
- `bytes`
- `content`

Default behavior:
- 10 lines of context before the hit
- 10 lines of context after the hit
- typed `resource_not_found` if the handle or match has expired

## `read_file`

Use `read_file` for a bounded repository-backed read once the path matters to the Frigg investigation flow, not as the default replacement for every quick shell slice.

Important inputs:
- `path`
- `repository_id`
- `max_bytes`
- `line_start`
- `line_end`

Important outputs:
- `repository_id`
- `path`
- `bytes`
- `content`

Notes:
- paths are canonical repository-relative paths
- line numbers are 1-based
- reads reflect live disk state

## `explore` (extended profile)

Use `explore` after you already know the file and want bounded follow-up inside it.

Operations:
- `probe`: search inside the file
- `zoom`: return a bounded window around an anchor
- `refine`: search only inside a smaller anchor-derived window

Important inputs:
- `path`
- `repository_id`
- `operation`
- `query`
- `pattern_type`
- `anchor`
- `context_lines`
- `max_matches`
- `resume_from`

Important outputs:
- `scan_scope`
- `window`
- `matches`
- `truncated`
- `resume_from`
- `metadata`

Prefer `explore` over repeated `read_file` calls when you are iterating inside one large file.
