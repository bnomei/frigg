# Discovery And Evidence

## `list_files`

Use `list_files` as the repository-aware replacement for `rg --files`, `find`, and `fd` when listing files in a code repository.

Important inputs:
- `repository_id`
- `path_regex`
- `glob`
- `language`
- `path_class`
- `include_hidden`
- `limit`
- `resume_from`

Parameter mapping:
- `repository_id`: optional in normal single-repo work; set only for multi-repo searches
- `path_regex`: repository-relative path filter replacing `rg --files path/`, `find path/`, or `fd`
- `glob`: repository-relative glob filter replacing common `fd`/`rg -g` file-listing habits
- `language`: source language filter when you want only one language family
- `path_class`: runtime/support/project path family filter
- `include_hidden`: equivalent to including hidden path segments
- `limit`: bounded result count
- `resume_from`: continuation token from a truncated response

Typical next move:
- `search_text` when you now have a path family and a literal/regex
- `read_file` when you already know the canonical repository-relative path
- shell listing only for non-code, generated/unindexed, or unavailable Frigg cases

## Multi-hypothesis probes (`search_batch` / parallel)

When you would fire 3–6 shell greps in one turn:

- **Preferred:** `search_batch` with 2–8 **independent** typed probes run concurrently, then merged/deduped (plus `probe_summary`). Cost scales with probe count; not a single shared multi-query index walk. Shipped on the core surface.
- **Fallback only:** same-turn parallel `search_text` and/or `search_symbol` when `search_batch` is absent from live `tools/list`.
- Do **not** mix parallel shell grep with Frigg search on indexed source while Frigg is healthy.

After multi-probe discovery, proof still goes through `read_match` / `read_file` (or navigation), never hybrid rank-1 alone.

## Target-first navigation

When a search row has `target_ref`, copy that opaque value unchanged into a navigation tool's
`target` field. Do not rebuild a target from a name or coordinates. Result-match targets are
session/source scoped:

```json
{
  "kind": "result_match",
  "result_handle": "rh_01",
  "match_id": "m_01",
  "target_scope": "018f3f9c4a1b7e28a9c2d4e6f8012345"
}
```

`target_scope` is an opaque correlation value, not an authentication credential. On
`TARGET_SCOPE_MISMATCH`, `STALE_HANDLE`, or `STALE_PROOF_ANCHOR`, rerun the producer and copy a
fresh target; targets do not navigate historical source. Every handle-bound row, including a
symbol row, emits `result_match`. Frigg also accepts the repository/corpus-scoped variant when a
caller already has a stable symbol identity:

```json
{
  "kind": "stable_symbol",
  "repository_id": "repo_01",
  "stable_symbol_id": "sym_01",
  "snapshot_token": "snapshot_01"
}
```

It never crosses into a same-named symbol in another repository. `STALE_TARGET_SNAPSHOT` means
refresh the symbol search in that repository and use its new target; `REPOSITORY_NOT_FOUND` or
`TARGET_NOT_FOUND` requires choosing a current repository/result.

## `search_hybrid`

Use `search_hybrid` for broad discovery when you do not yet have a stable symbol, string, or path anchor. It is the discovery surface, not the final proof step or the cleanest direct-string lookup. Use `search_text` for known literal, safe-regex, or `rg`-shaped text scans, and use `search_symbol` for known identifiers.

Anti-patterns:

- `BAD: search_hybrid → answer from rank-1` (or hybrid → shell grep as "precision")
- `GOOD: search_hybrid → next_actions (exact tool + arguments) / search_symbol / search_text → read_match`

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
- `metadata` only when `response_mode=full`

Compact-first rule:
- read-only search tools default to compact responses
- ask for `response_mode=full` only when you are diagnosing ranking or runtime behavior
- compact responses omit metadata unless `include_context_efficiency=true`
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

Full-mode diagnostics, only when needed:
- `channels`
- `lexical_only_mode`
- `semantic_capability`
- `utility`
- ranking notes

Interpretation rules:
- direct exact string, regex, or known symbol query: start with `search_text` or `search_symbol` instead of `search_hybrid`
- compact `ranking_note` containing `lexical_only (semantic not contributing)` (product semantic default is off) → treat matches as candidate pivots and move to `search_symbol` / `search_text` / `read_file` sooner; do not abandon Frigg for shell
- if full-mode diagnostics show `lexical_only_mode = true` or weak ranking, same pivot rule
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
- use `response_mode=full` when you need ranking diagnostics
- compact responses still return `result_handle` and row `match_id` values for `read_match`

Practical caution:
- inline test modules can still overmatch inside runtime files, even under `path_regex:"^src/"` or `path_class:"runtime"`
- treat `search_symbol` as a candidate locator, then confirm the specific runtime anchor with `go_to_definition`, `document_symbols`, or `read_file`

## `search_text`

Use `search_text` as the repository-aware replacement for `rg`/`grep` source-code search when you need direct exact or regex search plus Frigg semantics:
- canonical repository-relative paths
- repository scoping
- regex search over indexed files
- `path_regex` narrowing
- easy pivoting into `read_file`, navigation, or other MCP-backed follow-up

Use it for the same class of code scans agents often reach for `rg` to run:
- known literals
- safe regexes
- grouped alternation
- `path_regex` path narrowing
- context windows with `context_lines`
- per-file shaping with `max_count_per_file` or `files_with_matches`
- "which files contain this?" probes with `files_with_matches`

Notes:
- Frigg may use its native scanner, its ripgrep accelerator, or a mixed path depending on configuration and file content
- on macOS and Linux, Frigg may use `rg` internally as a lexical accelerator when it is available
- that does not change the public flow: Frigg still owns candidate scope, ordering, metadata, and fallback behavior
- shell `rg` remains appropriate for explicit live-disk verification, unindexed/generated paths, or when Frigg is unavailable — not as a throwaway "confirm" pass on indexed source after a Frigg zero
- for review-style work, `search_text` is often the best first proof surface when the repo has stable narrative terms, API names, or deterministic contract phrases
- prefer `path_regex` under runtime roots for implementation questions; unscoped hits may rank docs/skills first

Important inputs:
- `query`: pattern argument equivalent to `rg -n PATTERN`; use literal text by default and do not include shell quotes
- `pattern_type`
  - `literal`
  - `regex`
- `repository_id`: optional in normal single-repo work; set only for multi-repo searches
- `path_regex`: repository-relative path filter replacing `rg PATTERN path/` or `rg -g`
- `glob`: repository-relative glob replacing `rg -g`
- `exclude_glob`: repository-relative exclusion glob replacing `rg -g '!pattern'`
- `include_hidden`: equivalent to `rg --hidden`
- `limit`
- `context_lines`: like `rg -C`; use 2-5 when you need surrounding code
- `case_sensitive` / `ignore_case`
- `word`
- `files_with_matches`
- `count_only`
- `max_count_per_file`
- `collapse_by_file`
- `response_mode`

Shaping guidance:
- `context_lines` is the cheap first-pass alternative to a separate read for small review windows
- `max_count_per_file` keeps one noisy file from dominating the result set
- `files_with_matches=true` is the quickest way to reduce repeated-path spam
- `count_only=true` returns the count contract without bulky match rows
- compact responses still return `result_handle` and row `match_id` values so you can reopen one hit with `read_match`

## `read_match`

Use `read_match` when a prior search or navigation response already returned a `result_handle` plus `match_id` and you want a bounded source window without manually repeating path and line data.

Important inputs:
- `result_handle`
- `match_id`
- `before`
- `after`
- `presentation_mode`

Important outputs:
- `repository_id`
- `path`
- `line`
- `column`
- `start_line`
- `end_line`
- `bytes`
- `content`

Default behavior:
- 10 lines of context before the hit
- 10 lines of context after the hit
- text-first output by default: selected source bytes only, with no `structuredContent`
- use `presentation_mode=json` for repository, path, line window, byte, metadata, or machine-readable `content` fields
- every `result_handle` + `match_id` pair is session-local and bound to the source revision
  observed when it was issued
- typed `resource_not_found` with `STALE_HANDLE` if the pair has expired or been invalidated
- typed `resource_not_found` with `MIXED_HANDLE` if a `match_id` is paired with a different
  result handle
- typed `resource_not_found` with `STALE_PROOF_ANCHOR` if the bound source changed, was deleted,
  or cannot be verified; this returns no source bytes rather than current content

Handle lifetime:
- Session-scoped bookmarks (`handle_expires="session"`), not durable citation ids
- Dropped on explicit reindex / detach / whole-repo cache wipe for that repository
- Watch refresh with known dirty paths: anchors on those paths only are dropped; clean-path anchors may remain
- Known-empty success (noop refresh) does not wipe handles; unknown dirty set (notify drop, failed refresh) → whole-repo wipe
- After post-edit, `use_live_disk`, or `wait_watch`→ready for paths you care about: **re-run search** before trusting an old `result_handle`

Recovery:
- `STALE_HANDLE`: rerun the original search/navigation tool for a new pair.
- `MIXED_HANDLE`: keep the `match_id` with the `result_handle` from the same tool call.
- `STALE_PROOF_ANCHOR`: rerun the originating search/navigation tool and use its new pair. Do
  not retry the old handle or treat current bytes as proof of the old result. `read_file` remains
  the explicit current-live-content path when historical proof is not needed; it does not refresh
  a proof pair.

## `read_file`

Use `read_file` as the repository-backed replacement for `cat`, `sed -n`, and bounded source-code reads once the path matters to the Frigg investigation flow.

Important inputs:
- `path`
- `repository_id`: optional in normal single-repo work; set only for multi-repo searches
- `max_bytes`
- `start_line`: replacement for the start side of `sed -n 'start,endp'`
- `end_line`: replacement for the end side of `sed -n 'start,endp'`
- `line_count`: line-count alternative to `end_line`
- `presentation_mode`

Important outputs:
- `repository_id`
- `path`
- `bytes`
- `content`

Notes:
- paths are canonical repository-relative paths
- line numbers are 1-based
- reads reflect live disk state; unlike `read_match`, `read_file` does not claim historical proof
- default output is text-first: selected source bytes only, with no `structuredContent`
- use `presentation_mode=json` for repository, path, byte, metadata, or machine-readable `content` fields

## Completeness and v2 paging

Bounded discovery responses carry `completeness` as the authoritative collection contract. Read
`unit`, page-local `returned`, and exact-or-absent `total` before deciding a page proves absence.
`complete=true` has no omissions or continuation; `truncated=true` names deliberate omissions in
`truncation_reasons`; `incomplete_reasons` describe coverage Frigg cannot prove.

When `completeness.continuation` is present, replay the same request with that opaque value in
`continuation`. The v2 token binds the tool, normalized request, session, repository scope, and
snapshot; changed input or source state is rejected as scope mismatch or stale. Do not also send
legacy `resume_from`. Legacy cursors remain accepted only for the compatibility window, while
new output uses v2 continuation.

Surface-specific rules:

- `list_files` keeps `total_files`, `truncated`, and legacy `resume_from` synchronized with the
  canonical envelope; use `completeness.total` and `continuation` for new paging clients.
- `search_text.total_matches` is the raw occurrence total, separate from `completeness.total` for
  selected rows. Normal search rows use occurrence units; `files_with_matches` and
  `collapse_by_file` use file units; `max_count_per_file` caps rows before page retention.
  `count_only=true` intentionally returns zero `matches[]`, so read the count and completeness.
- `search_symbol`, `list_files`, `explore` probe/refine, `document_symbols`, and
  `search_structural` can issue v2 continuation only for deliberately truncated exhaustive rows.
  A final suffix page may be
  `complete=true` with a smaller page-local `returned` than the request-wide total.
- `search_hybrid` is ranked discovery, not exhaustive paging: its total is absent,
  `complete=false` includes `ranked_discovery`, and it provides no exhaustive continuation. Use
  an exact text or symbol pivot before treating discovery candidates as proof.
- `search_batch` has both merged `completeness` and per-probe
  `probe_summary[].completeness`; inspect both because a child cap or diagnostic propagates to
  the aggregate.

## `explore` (core product surface)

Use `explore` after you already know the file and want bounded follow-up inside it. It is on the **core** tool surface (not extended-only).

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
- `continuation`
- `presentation_mode`

Important outputs:
- `probe` and `refine`: structured `scan_scope`, `window`, `matches`, `truncated`, legacy `resume_from`, canonical `completeness`, and `metadata`
- `zoom` default: selected source bytes only
- `zoom` with `presentation_mode=json`: structured window and metadata fields
- `zoom` with `presentation_mode=json`: the structured compatibility payload

Default behavior:
- `probe` and `refine` remain structured by default
- `zoom` is text-first by default
- `presentation_mode=text` is rejected for `probe` and `refine`

Prefer `explore` over repeated `read_file` calls when you are iterating inside one large file.
