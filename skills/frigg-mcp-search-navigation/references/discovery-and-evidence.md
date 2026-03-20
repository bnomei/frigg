# Discovery And Evidence

## `search_hybrid`

Use `search_hybrid` first for broad natural-language questions. It is the discovery surface, not the final proof step.

Important inputs:
- `query`
- `repository_id`
- `language`
- `limit`
- `weights`
- `semantic`

Important output shape:
- `matches[]`
- optional top-level compatibility mirrors such as `semantic_status` and `warning`
- `metadata`

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
- `lexical_only_mode = true`: rely more on exact follow-up tools
- `utility.best_pivot_*`: good hint for the first file to open next

Typical next move:
- `search_symbol` when you now know the symbol
- `search_text` when you need exact strings or path scoping
- navigation tools when you need defs/refs/calls
- `read_file` when you need repository-backed proof

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

## `search_text`

Use `search_text` when shell search is not enough and you specifically need:
- canonical repository-relative paths
- repository scoping
- regex search over indexed files
- `path_regex` narrowing

Important inputs:
- `query`
- `pattern_type`
  - `literal`
  - `regex`
- `repository_id`
- `path_regex`
- `limit`

## `read_file`

Use `read_file` for a bounded repository-backed read, not as the default replacement for a quick shell slice.

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
