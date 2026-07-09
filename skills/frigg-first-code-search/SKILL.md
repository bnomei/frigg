---
name: frigg-first-code-search
description: "Default to Frigg MCP instead of shell rg/grep/find/fd/cat/sed for source-code discovery, search, navigation, and proof reads in attached repos. Scenario-first routing: pick the flow, then the tool. Shell remains correct for git, build/test output, generated/unindexed paths, explicit live-disk checks, and when Frigg is unavailable."
version: 2026-07-08
---

# Frigg First Code Search

Frigg is the default for code discovery, file listing, navigation, exact code search, and bounded source reads.

Before using shell `rg`, `grep`, `find`, `fd`, `cat`, or `sed` for code exploration, use the matching Frigg MCP tool in attached or attachable code repositories.

Shell tools are fallback only for git state and diffs, non-code files, build/test output, generated or unindexed files, explicit live-disk verification, or when Frigg is unavailable.

Do not run parallel shell grep in the same turn as Frigg search on indexed source.

Harness-specific MCP registration or tool-order flukes (for example one-off Grok schema/cache issues) are **outside Frigg's product contract**. Route indexed source through Frigg when the live tool list exposes Frigg tools; do not invent product workarounds for host quirks.

**HTTP-first runtime:** prefer loopback HTTP for shared and long-running work. Stdio is for one local client that owns the process. Adoption, watch freshness, and cache reuse are HTTP-first behaviors.

Deep parameter encyclopedias live under [references/](references/) — use them after you pick a scenario. **This top screen is enough to route.**

---

## Pick your scenario first

| I need to… | Start here | Do not start with |
| --- | --- | --- |
| Try several guesses at once | [Parallel wide net](#parallel-wide-net) | Parallel shell grep |
| Answer “where is X?” (vague) | [Broad discovery](#broad-discovery) | Hybrid alone, or grep |
| Find exact text / regex | [Exact text search](#exact-text-search) | Hybrid |
| I know the function/type name | [Known symbol](#known-symbol) | Hybrid, unscoped grep |
| See who uses / calls something | [Impact](#impact) | `rg` on whole repo |
| List files or outline a big file | [Files & outline](#files--outline) | `find` when Frigg up |
| Read code to understand | [Proof read](#proof-read) | — |
| Cite `start:end:path` to user | [Citation read](#citation-read) | — |
| Wrong repo / zero hits / adoption | [Session gate](#session-gate) | Grep “to verify” |
| Several repos on one Frigg | [Multi-repo](#multi-repo) | Assume cwd = default |
| After I edited files | [Post-edit](#post-edit) | Repo-wide shell grep |
| Trace a failure | [Bug trace](#bug-trace) | Hybrid when error text known |
| Refactor blast radius | [Refactor impact](#refactor-impact) | Throwaway `rg` |
| Review change / subsystem risk | [Technical review](#technical-review) | Grep transcript as proof |
| Risky pattern / security sweep | [Security sweep](#security-sweep) | Parallel shell regex only |
| Open config / manifest path | Direct `read_file` | Repo-wide search first |

Compact intent map (same routing, shorter):

```text
Known string or regex -> search_text
Known function/type/API name -> search_symbol
Vague "where is X?" -> search_hybrid, then exact search
Several guesses -> search_batch or parallel search_text
Need proof -> read_match, then read_file
Need impact -> impact_bundle(symbol) or find_references / incoming_calls / implementations
Wrong repo, stale index, surprising zero -> workspace
Git/build/generated/unindexed/Frigg missing -> shell fallback
```

---

## Full shell → Frigg card

Use this table before reaching for shell on source code.

| Shell habit | Frigg call |
| --- | --- |
| `rg -n PATTERN` | `search_text`, `query=PATTERN` (literal default) |
| `rg -n 'a\|b'` | `search_text`, `pattern_type=regex`, `query='a\|b'` |
| `rg -n PATTERN src/` | `search_text`, `path_regex='^src/'` (or repo runtime roots, e.g. `^crates/`) |
| `rg -l PATTERN` | `search_text`, `files_with_matches=true` |
| `rg -c PATTERN` | `search_text`, `count_only=true` → read **`total_matches`**, not empty `matches[]` as failure |
| `rg -g '**/*.rs' PATTERN` | `search_text`, `glob='**/*.rs'` (use `**` for recursion) |
| `rg -C 3 PATTERN` | `search_text`, `context_lines=3` |
| `rg --files -g '*.rs' path/` | `list_files`, `glob='*.rs'`, `path_regex='^path/'` |
| `find path -name '*.rs'` / `fd` | `list_files`, `glob='*.rs'`, `path_regex='^path/'` |
| `cat path` | `read_file`, `path=path` |
| `sed -n '10,80p' path` | `read_file`, `start_line=10`, `end_line=80` |

**Default scope:** `path_regex='^src/'` or tighter (use the repo’s real runtime roots, e.g. `^crates/`) unless the question is about docs, specs, or intentionally indexed research paths.

**Regex trap:** `|`, `.*`, `^`, `$`, or character classes in a literal query with 0 hits → retry with `pattern_type=regex`.

**Ignore truth:** Frigg follows `.gitignore`. Ignored paths (for example gitignored `/docs/`) do not appear in indexed search even if they exist on disk — use direct read or adjust ignore rules when those paths are the task.

---

## Hard anti-patterns

```text
BAD: hybrid -> grep
BAD: hybrid rank-1 -> answer without exact pivot
BAD: Frigg search + parallel shell grep on indexed source
BAD: unscoped zero-hit conclusion
BAD: document_symbols for a known name
BAD: go_to_definition(path, line) without symbol on dense lines
BAD: ignore ambiguous_location=true / location_warning and edit from path+line defs
BAD: read_match with a match_id from another result_handle
BAD: repo-wide shell rg to "confirm" an explainable Frigg zero
BAD: throwaway shell rg on indexed src when Frigg is attached
BAD: calling tools only in a stale schema cache, not in live tools/list
BAD: invent workspace_index / workspace_reindex / deep_search from source or host descriptors
BAD: treat search_batch as one cheap multi-pattern scan / expect early-exit across probes
```

---

## Done criteria

```text
After search: read_match (or read_file) before answering or editing.
After hybrid: exact search_text or search_symbol before proof.
After impact query: navigation evidence before textual whole-repo grep.
After surprising zero: trust zero_hit_reason / recovery fields before shell.
After edit: workspace freshness before reusing Frigg for touched paths.
```

Multi-hypothesis answers must cite at least one Frigg proof window with no parallel shell grep on indexed source while Frigg is healthy.

---

## Positive fallback boundary

**Always shell or host tool:**

- `git status`, `git diff`, commit and branch operations
- build output, test output, package-manager output
- generated or ignored directories (e.g. `target/**`, `.ask/generated/`)
- explicit live-disk verification after a write when freshness is not proven
- Frigg unavailable / unregistered

**Frigg or direct read** (depending on path and goal):

- small authored manifests and config (`ask.toml`, `**/manifest.toml`)
- project docs and fixtures when indexed, or direct read when path is known
- newly created files before watch confirms ingestion
- ignored paths that exist on disk but are not in the index

**Never shell as a trust patch:**

- source search under indexed runtime paths when Frigg is registered
- “confirming” a scoped Frigg zero-hit without a stale or unindexed reason
- references or call analysis when a symbol anchor exists

---

## Scenario cards

Each card: **Trigger / Habit / Frigg path / Fallback / Proof / Done / Product support**.

### Parallel wide net

| Field | Content |
| --- | --- |
| **Trigger** | Several plausible strings or symbols; would fire 3–6 greps in one turn |
| **Habit** | Parallel `grep`/`rg`/`Grep` in one message |
| **Frigg path** | Prefer `search_batch([...])`: **independent** concurrent probes, then merge/dedupe. Same-turn parallel `search_text` / `search_symbol` only if batch is unavailable on the live surface. |
| **Fallback** | Shell only if Frigg unregistered or path unindexed |
| **Proof** | `read_match` on best merged hit |
| **Done** | Multi-hypothesis answer cites Frigg proof; no parallel shell on indexed source |
| **Product support** | 2–8 probes, each a full search; `probe_summary` per probe; path:line dedupe; handles / `suggested_next`. **Not** a single shared multi-query walk or early-exit fusion. |

```text
PREFERRED: search_batch([
  { id, kind: text|symbol|hybrid, query, path_regex?, glob?, path_class? },
  ...  // 2..=8 probes
])

Mental model: same as parallel Frigg searches in one call — not one fused rg with many patterns.
Prefer text+symbol probes for multi-guess; use hybrid probes sparingly (vague only); still proof via read_match.

ELSE — only if tools/list lacks search_batch:
  search_text(probe_1, path_regex=^src/)
  search_text(probe_2, glob=**/*.{rs,toml,yaml})
  search_symbol(guessed_name, path_class=runtime)
→ pick best hit → read_match → answer
```

Example probe set (registration-style):

```text
search_text: catalog_entries      path_regex=^src/
search_text: query_surfaces       glob=**/*.{rs,toml,yaml}
search_symbol: catalog_entries    path_class=runtime
```

### Broad discovery

| Field | Content |
| --- | --- |
| **Trigger** | “Where is X?” — no symbol, exact string, or path |
| **Habit** | Semantic search, guessed `rg` keywords, open top files |
| **Frigg path** | `search_hybrid(question)` → `suggested_next[]` or noun pivot → `search_symbol` / `search_text` → proof read |
| **Fallback** | Skip hybrid if a likely symbol/string is already known. Never shell-grep as the precision layer after hybrid |
| **Proof** | Exact search, then `read_match` / `read_file` |
| **Done** | Never answer from hybrid rank-1 alone; exact Frigg evidence present |
| **Product support** | Compact pivots, `ranking_note`, `best_pivot_path`, `suggested_next` |

```text
1. search_hybrid("<user question>")
2. Do NOT answer from rank #1 alone
3. If suggested_next[] → run those; else extract nouns → search_symbol → scoped search_text
4. read_match / read_file for proof
```

```text
BAD:  search_hybrid → read_file(first hit) → answer
GOOD: search_hybrid → search_symbol/search_text → read_match
```

If full mode shows `lexical_only_mode` or weak ranking → pivot to exact tools immediately.

Prefer `suggested_next` identifiers after hybrid (short symbol/text queries); do not paste the original natural-language question into `search_symbol`.

### Exact text search

| Field | Content |
| --- | --- |
| **Trigger** | Literal, regex, config key, error string, `rg`-shaped probe |
| **Habit** | `rg -n` / `-l` / `-c` / `-g` / scoped path |
| **Frigg path** | `search_text` with scope + shaping flags (see shell card) |
| **Fallback** | Shell only for unindexed / Frigg down / live-disk. Do not shell-confirm a complete indexed zero |
| **Proof** | `read_match` when handle present, else `read_file` |
| **Done** | `rg` habits map to `search_text`; regex and count-only traps handled without shell |
| **Product support** | Regex-looking literal recovery, count_only shape, scope echo, handles, `latency_class` |

```text
search_text(
  query=...,
  pattern_type=literal|regex,
  path_regex=^src/...,   # almost always
  glob=..., files_with_matches=..., count_only=..., context_lines=...
)
→ read_match(handle, match_id) OR read_file(path, start_line, end_line)
```

Unscoped search may rank docs/specs/skills before runtime — expected noise. Prefer runtime path scope for implementation questions.

### Known symbol

| Field | Content |
| --- | --- |
| **Trigger** | Known or strongly guessed function/type/API/class/trait/module name |
| **Habit** | `rg -n "fn name"` or IDE go-to-def |
| **Frigg path** | `search_symbol(name, path_class=runtime)` → `go_to_definition(symbol=name)` → proof |
| **Fallback** | No hybrid or unscoped grep when name is known |
| **Proof** | `read_match` or `read_file` (json/citation if citing) |
| **Done** | Starts at `search_symbol`; navigates with `symbol`, not path+line alone |
| **Product support** | Runtime-first defaults, `column`/`excerpt`, empty-`{}` rejection |

```text
1. search_symbol(name, path_class=runtime)  # prefer symbol + column from hit when present
2. go_to_definition(symbol=name)            # always pass symbol; never path+line alone
3. If path+line only was used: check ambiguous_location / location_warning before editing
4. read_match OR read_file
```

### Impact

| Field | Content |
| --- | --- |
| **Trigger** | Usages, callers, callees, trait impls for a known anchor |
| **Habit** | Whole-repo `rg -n symbol`, manual dedupe |
| **Frigg path** | Prefer `impact_bundle(symbol)`; or sequential symbol → refs → callers → impls |
| **Fallback** | Scoped `search_text` may supplement tests; shell `rg` is not the main reference pass |
| **Proof** | Read clusters; confirm `outgoing_calls` with body reads |
| **Done** | Navigation evidence before edit or impact claim |
| **Product support** | Ref filters, mode flags, `impact_bundle` composition |

```text
PREFERRED: impact_bundle(symbol, path_class=runtime)
  → suggested_next (single recovery channel; compact default) for tests pass + read_match proof
  // full response_mode only forwards diagnostics to child nav/search — not a second next-step list

OR sequential:
search_symbol(anchor, path_class=runtime)
find_references(symbol, include_definition=false)
incoming_calls(symbol)              # who calls — reliable
outgoing_calls(symbol)              # provisional; confirm with read_file
find_implementations(symbol)        # traits/interfaces only
```

### Files & outline

| Field | Content |
| --- | --- |
| **Trigger** | Which files exist under a path, or outline a large file |
| **Habit** | `find` / `rg --files` / scroll / `rg -n "^pub fn"` |
| **Frigg path** | `list_files(...)` or `document_symbols(path, top_level_only=true)` |
| **Fallback** | Shell `find` only if Frigg down or path unindexed. Known name → `search_symbol`, not outline |
| **Proof** | After pick, `read_file` / `read_match` |
| **Done** | No shell `find` for indexed listing when Frigg attached |
| **Product support** | List/outline pagination, `resume_from`, top-level default |

```text
list_files(glob='*.rs', path_regex='^src/catalog/')
document_symbols(path, top_level_only=true)
  → pick symbol line → read_match or read_file
```

Prefer anchored path filters (`^ontology/`) over bare substrings (`ontology`).

### Proof read

| Field | Content |
| --- | --- |
| **Trigger** | Understand behavior after a search/nav hit (internal) |
| **Habit** | Host Read with line prefixes |
| **Frigg path** | Prefer `read_match(result_handle, match_id)`; else `read_file(path, start_line, end_line)` |
| **Fallback** | Host Read acceptable only when needed outside Frigg scope |
| **Proof** | Bounded source window from Frigg |
| **Done** | Internal proof stays on Frigg handles/paths |
| **Product support** | Scoped handles, stale/mixed-handle failures |

```text
match_id is valid ONLY with its own result_handle from the SAME call.
match_id is scoped: search:m1, symbols:m1, hybrid:m1, nav:m1 (never reuse across tools).
handle_scope + handle_expires="session" mark the pairing lifetime.
Stale handle → re-run search; mixed/foreign match_id → use the matching handle from the same call.
Text mode returns raw source (no line prefixes) — fine for internal proof.
```

### Citation read

| Field | Content |
| --- | --- |
| **Trigger** | User-facing reply needs ` ```startLine:endLine:path``` ` |
| **Habit** | Cursor/host Read (`LINE\|content`) |
| **Frigg path** | `read_file` / `read_match` with `presentation_mode=json` for `start_line`/`end_line`; or `presentation_mode=citation` for `LINE\|content` text |
| **Fallback** | Host Read only when Frigg is unavailable |
| **Proof** | Stable `start_line`/`end_line` (json) or line-prefixed text (citation) |
| **Done** | Citation has line metadata without abandoning Frigg for every internal read |
| **Product support** | json + citation presentation modes, evidence packets |

```text
Proof (internal): presentation_mode=text (default) — raw source, no prefixes.
Citation (user-facing): presentation_mode=citation → "306|pub fn catalog_entries..."
Or: presentation_mode=json → use start_line/end_line + path in ```start:end:path``` fences.
```

### Session gate

| Field | Content |
| --- | --- |
| **Trigger** | Adoption, wrong repo, index health, or surprising zeros uncertain |
| **Habit** | Assume cwd; grep when search feels wrong |
| **Frigg path** | `workspace(path=<repo root>)` — **gate, not preamble** |
| **Fallback** | Shell not a patch for unexplained zeros; check repo/index first |
| **Proof** | After adoption/health, resume the real scenario tool |
| **Done** | Workspace used when trust changes, not before every search |
| **Product support** | `recommended_action`, `gate_hint`, `runtime.watch_status`, `runtime.tools_exposed`, optional `lexical_ready`/`semantic_ready`, dirty/changed paths |

```text
Call workspace(path=...) IF:
  - wrong-repo paths, unexplained zeros, index errors, multi-repo ambiguity, post-edit freshness
Skip workspace IF:
  - first search hits expected paths and index is ready

Agent health decision tree (branch here only — not full scorecards):
  1. recommended_action (primary)
  2. zero-hit recovery / ZeroHitIndex when a search returned empty
  3. optional lexical_ready / semantic_ready — never promote SCIP generator install mid-task
  4. runtime.watch_status.reason explains wait_watch (mode_off / no_lease / refreshing / active)

recommended_action=reindex means index substrate not Ready:
  - NOT a public MCP tool (tools/list has no reindex / workspace_reindex)
  - operator/CLI: `frigg index` (or attach-side ensure / operator lifecycle)
  - read gate_hint when present; do not invent a write tool or shell-grep the repo "to fix" it

Tool surface honesty:
  - Trust runtime.tools_exposed (or live tools/list) for this process
  - Do not call names only in host schema caches, skill memory, or source #[tool] attributes
  - tool_surface_profile is core|extended; explore/playbook may be absent on core

Policy progressive disclosure:
  - Skill scenario cards are SSOT for agent routing
  - MCP frigg://policy/* resources are secondary/machine (hosts, support matrix, tool-surface)
  - Do not treat resources as a second full skill
```

Verify live `tools/list` / `runtime.tools_exposed` before calling schema-only or extended-only tools.

### Multi-repo

| Field | Content |
| --- | --- |
| **Trigger** | One Frigg (often HTTP) serves several repos; session default may differ |
| **Habit** | Assume cwd; grep harder after zero |
| **Frigg path** | `workspace(path=<repo root>)` then search/nav with `repository_id` when needed |
| **Fallback** | Wrong-repo zero → adopt or explicit id, not shell grep |
| **Proof** | Continue with exact/symbol/nav after correct repo |
| **Done** | Wrong-default searches are recoverable via workspace diagnostics |
| **Product support** | Zero-hit names repository; recovery suggests `workspace(path=...)` |

### Post-edit

| Field | Content |
| --- | --- |
| **Trigger** | Edited or created files; need index freshness for re-search |
| **Habit** | Bypass index with Read or repo-wide grep |
| **Frigg path** | `workspace()` → if ready for path, Frigg search; if stale/unknown, **targeted** live read of **touched paths only** |
| **Fallback** | Live-disk for **touched paths only** (`changed_paths_since_snapshot` / known edit paths) — **never** a license for repo-wide shell grep |
| **Proof** | Frigg after watch; else direct read of edited paths |
| **Done** | Freshness visible; no default repo-wide shell verification |
| **Product support** | `working_tree_dirty`, `changed_paths_since_snapshot`, `runtime.watch_status`, `recommended_action` + `gate_hint` (path-scoped live-disk when dirty+Ready) |

```text
1. workspace() after edits
2. If recommended_action=ready (or fresh_enough_for includes search_*) → Frigg search
3. If use_live_disk_for_touched_files → read_file / host Read on **those paths only**
4. If wait_watch → read runtime.watch_status.reason (mode_off / no_lease / refreshing / active);
     wait or path-scoped live reads — do not "verify" with whole-repo rg or micro-manage retries
5. If reindex → CLI `frigg index` / operator lifecycle (not an MCP tool); optional path-scoped live reads while index rebuilds
BAD: workspace dirty → rg -n across the repo
BAD: recommended_action=reindex → call invented MCP workspace_reindex
BAD: wait_watch → invent a retry loop without reading watch_status
GOOD: workspace dirty → read_file(edited/path.rs) or wait for hot-path watch refresh
GOOD: recommended_action=reindex → frigg index (CLI) or wait for operator; read gate_hint
```

### Bug trace

| Field | Content |
| --- | --- |
| **Trigger** | Test/command/log/stack/runtime failure |
| **Habit** | `rg` error text + test name, then symbol grep |
| **Frigg path** | Exact error/test text → symbol → refs/callers → proof |
| **Fallback** | Hybrid only if no exact string. Shell owns build/test **output**; source returns to Frigg |
| **Proof** | Origin + caller/reference witnesses via `read_match` |
| **Done** | Failure text maps text → symbol → refs → proof |
| **Product support** | Zero-hit reasons, navigation `mode` diagnostics |

```text
1. workspace(path=...) only if repo/adoption uncertain
2. search_text(error fragment, path_regex='^(src|tests)/')
3. search_text(test_name) if test failure
4. search_symbol(fn_from_stack, path_class=runtime)
5. find_references / incoming_calls
6. read_match on strongest witness
```

If navigation `mode` is unavailable/heuristic, widen with `search_text` before assuming code is absent.

### Refactor impact

| Field | Content |
| --- | --- |
| **Trigger** | Will change API/type/behavior; need blast radius before edit |
| **Habit** | Whole-repo `rg` + manual sort |
| **Frigg path** | Prefer `impact_bundle(api)`; or runtime symbol → refs → impls → callers → optional tests pass |
| **Fallback** | No throwaway shell `rg` on indexed source; use scoped `search_text` for extra textual passes |
| **Proof** | `read_match` each cluster before editing |
| **Done** | Navigation-first blast radius with source witnesses |
| **Product support** | Ref filters, path-class filters, `impact_bundle` |

```text
PREFERRED: impact_bundle(api, path_class=runtime) → read_match clusters
OR:
1. search_symbol(api, path_class=runtime)
2. find_references(symbol, include_definition=false)
3. find_implementations if trait/interface
4. incoming_calls for caller graph
5. search_text(symbol, path_regex='^tests/') when tests matter
6. read_match on each cluster
```

### Technical review

| Field | Content |
| --- | --- |
| **Trigger** | Review change, PR, or subsystem for behavioral risk |
| **Habit** | Diff skim, grep symbols, manual callers/tests |
| **Frigg path** | Behavioral anchors + changed APIs → refs/callers → proof clusters → tests pass |
| **Fallback** | `git diff` and build/test stay shell; source impact returns to Frigg |
| **Proof** | Every finding has path/line witness; prefer evidence packets |
| **Done** | Findings grounded in Frigg evidence, not shell grep transcripts |
| **Product support** | Citation reads, evidence packets, ref filters, explainable zeros |

```text
search_text(behavioral anchors, path_regex='^src/')
search_symbol(changed APIs, path_class=runtime)
find_references / incoming_calls
read_match on affected clusters
search_text(changed API, path_regex='^tests/') when tests matter
```

Trust order for review:

1. `search_text` — narrative anchors  
2. `read_file` / `read_match` + defs/refs — proof  
3. `search_structural` — awkward AST evidence (tier-3)  
4. `incoming_calls` — call-flow hint  
5. `outgoing_calls` — provisional until body proof  

Evidence packet shape (multi-claim answers) — every review/security finding needs path/line witnesses:

```json
{
  "claims": [
    {
      "claim": "catalog_entries registers callable operations",
      "tool": "search_symbol",
      "path": "src/catalog/mod.rs",
      "start_line": 40,
      "end_line": 72,
      "match_id": "symbols:m1",
      "result_handle": "..."
    },
    {
      "claim": "caller reaches catalog_entries from the HTTP surface",
      "tool": "incoming_calls",
      "path": "src/http/routes.rs",
      "start_line": 88,
      "end_line": 110,
      "match_id": "nav:m2",
      "result_handle": "..."
    }
  ]
}
```

### Security sweep

| Field | Content |
| --- | --- |
| **Trigger** | Class of risky patterns (unsafe paths, exec, credentials, untrusted input) |
| **Habit** | Many parallel regex `rg` probes |
| **Frigg path** | Prefer `search_batch` of text/symbol/(optional hybrid) probes; interim parallel Frigg probes; then witnesses + refs |
| **Fallback** | Generated/vendor/build artifacts may need shell when explicitly in scope; indexed source stays on Frigg |
| **Proof** | Each candidate: source witness; sink → refs/callers when relevant; evidence packet when reporting |
| **Done** | Broad parallel probes without leaving Frigg for shell on indexed source |
| **Product support** | Safe-regex diagnostics, batch summaries, path_class filters, evidence packets |

```text
search_batch([
  text regex probes scoped to runtime,
  symbol probes for known sinks,
  optional hybrid for vague concepts
])   # interim: parallel search_text / search_symbol
→ read_match on witnesses
→ find_references / incoming_calls for validated sinks
```

---

## Structural search and explore (tier-3)

Do **not** put these on the main discovery path.

**Structural** — expert syntax-shape evidence only:

```text
document_symbols or read_file on a representative file
  → inspect_syntax_tree(line AND column; column: 1 if unknown)
  → search_structural from real node kinds
```

- Known name? Use `search_symbol`, not `document_symbols`.
- Do not write Tree-sitter queries for ordinary symbol or text search.
- Opt-in: `include_follow_up_structural=true` for replayable `search_structural` suggestions on structure-aware tools (not hybrid/symbol search).

**Explore** (extended profile) — **anchored in-file only**:

```text
Use explore after you already have a file anchor.
probe/refine scan inside one file.
zoom reads a bounded source window.
For cross-repo search, use search_text / search_symbol / search_hybrid.
```

`explore zoom` may nest content under structured `window` fields in JSON mode — prefer `read_file` / `read_match` for ordinary proof.

---

## Default loop (compact)

1. Omit `repository_id` in single-repo work unless multi-repo or wrong default.
2. Pick scenario from the table — do not default to grep.
3. Search tier: **symbol** (known name) → **text** (known string) → **hybrid** (vague only) → **batch/parallel** (several guesses).
4. Compact responses first; `response_mode=full` only for ranking diagnostics (impact_bundle: one `suggested_next` via recovery flatten — full is not more next-steps).
5. Navigate with the **`symbol`** parameter.
6. Proof: `read_match` → `read_file`.
7. Cite: json/citation `read_file` or host Read when required.
8. Structure/explore only as tier-3 after anchors exist.

---

## Compact response rules

- `result_handle` + `match_id` → `read_match` (**same call only**; scoped ids like `search:m1`).
- Text-first `read_file` / `read_match` / `explore(zoom)`: no line prefixes in text mode.
- `presentation_mode=json` when you need `start_line`, `end_line`, bytes, or machine-readable fields.
- Hybrid compact may omit rich scores — pivot to exact tools; do not grep.
- Prefer `latency_class` guidance when present (`hot` / `warm` / `cold`); do not use shell as a latency strategy on indexed source.

---

## Decision table

| Situation | Tool |
| --- | --- |
| Git / build / generated / unindexed / Frigg down | shell |
| Several guesses at once | `search_batch` (prefer); parallel Frigg probes only if batch missing |
| Vague “where is X?” | `search_hybrid` → exact pivot |
| Known identifier | `search_symbol` (`path_class=runtime`) |
| Known string / regex / rg-shaped | `search_text` |
| File listing | `list_files` |
| File outline | `document_symbols` |
| Proof after search | `read_match`, then `read_file` |
| User citation | `read_file` json/citation or host Read |
| References / callers | `find_references`, `incoming_calls` |
| Callees | `outgoing_calls` (confirm with read) |
| Trait / interface impls | `find_implementations` |
| Definition | `go_to_definition(symbol=...)` (not path+line alone; trust `ambiguous_location`) |
| AST shape | `inspect_syntax_tree` + `search_structural` (tier-3) |
| In-file zoom (extended) | `explore` — not a substitute for `search_text` |
| Repo health / adoption | `workspace` gate |

---

## Deep references

Parameter exhaustiveness and extended surfaces — open only after routing:

- [references/discovery-and-evidence.md](references/discovery-and-evidence.md) — `list_files`, `search_*`, `read_*`, `explore`
- [references/navigation-and-structure.md](references/navigation-and-structure.md) — defs, refs, calls, outline, structural
- [references/workflows.md](references/workflows.md) — extended narrative loops (aligned with scenario cards above)
- [references/workspace-and-runtime.md](references/workspace-and-runtime.md) — `workspace`, adoption, runtime
- [references/extended-tools.md](references/extended-tools.md) — only after verifying live `tools/list` / `runtime.tools_exposed`
- Live machine surface (not inventory freezes): MCP `frigg://policy/tool-surface.json`
