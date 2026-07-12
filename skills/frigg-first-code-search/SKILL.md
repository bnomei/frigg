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

**Harness boundary (Frigg is an evidence layer, not an agent OS):**

- Host tool order, Grep-first defaults, and intermittent MCP **registration/inheritance** flukes are **outside** Frigg product code (**FUT-003** harness boundary — not a Frigg CI gate or ranking bug).
- When the bridge is down or tools are unregistered, that is **harness reliability**, not ranking quality.
- Soft PreToolUse / adopt hooks are **opt-in** (not guaranteed Frigg-first). Soft only: Claude hook injects next-step context (`search_text` / `search_batch` / …) and **never hard-denies** Grep/shell (`permissionDecision` is not product).
- Hard block of Grep/`rg` on indexed roots is a **host experiment**, not Frigg core — do not invent `FRIGG_HOOK_STRICT` or product tool-order hacks.
- Do **not** invent product hacks (fake Grep, hide host tools, force MCP tool order, or a “subagent bridge health” Frigg tool — Frigg cannot see the host registry when it is not registered).
- Route indexed source through Frigg when the live tool list exposes Frigg tools; progressive disclosure on this top screen is intentional (30-second rule).

**Probe-on-spawn (Task / subagent / new session):**

```text
1. Before claiming Frigg-first, verify Frigg is on the *live* host tool surface for THIS spawn:
   live tools/list (or a successful Frigg CallMcpTool). Do NOT trust on-disk mcps/…/tools
   descriptors or a parent session's tool list as proof this spawn has Frigg.
2. If Frigg is absent from live tools/list for this spawn (empty Frigg server, or
   Tool not found when calling a known Frigg tool like search_text while the parent still has Frigg):
   - Shell/Grep/Read fallback is correct for this spawn
   - Note the gap once (harness MCP inheritance / registration) — do NOT file as Frigg search/ranking failure
   - Do NOT invent workspace_reindex / bridge_health / fake tools to "fix" registration
   - (Invented names / extended-only tools missing while other Frigg tools work → surface honesty,
     not inheritance — see tool-surface rules)
3. Parent session success does NOT imply child Task success — re-probe every delegation.
4. Prefer loopback HTTP + frigg serve when multiple clients/subagents should share one Frigg
   *after* each spawn actually has Frigg registered; HTTP shares state when registration works —
   it does not create child registration by itself.
```

**Transport contract (dual mode — hosts choose; Frigg does not force one):**

| Mode | When | Freshness contract |
| --- | --- | --- |
| **Loopback HTTP** (`frigg serve`, default adopt MCP URL) | Shared / long-running / multi-client / subagents | **Full freshness** assumes HTTP + watch leases: adoption, hot-path refresh, shared caches |
| **Stdio** (client spawns `frigg`) | One local client that owns the process | **Valid, different contract** — default `WatchMode::Off` (not Auto); often `mode_off`; not “broken Frigg” |

- Managed adopt configs emit **HTTP** (`type: http`, `http://127.0.0.1:37444/mcp`). Keep `frigg serve` running for those clients.
- Do **not** claim full post-edit watch freshness on stdio when `runtime.watch_status.reason=mode_off` / `no_lease`. Use path-scoped live-disk or wait only if watch is actually on.
- Do **not** blame ranking, hybrid, or search quality for staleness that is really stdio-without-watch / multi-process stdio races.
- Stdio is not second-class or deprecated; it is the right shape for a single ephemeral client. Prefer HTTP as soon as a second client or subagent shares the repo.

Deep parameter encyclopedias live under [references/](references/) — use them after you pick a scenario. **This top screen is enough to route.**

**Dual OS note:** Agents follow this **skill loop** (symbol → text → hybrid → batch → proof). Searcher ranking/policy depth is an **internal ranking OS** for product engineers and evals — not a second agent DSL. Prefer compact pivots and exact tools over “trust hybrid rank-1.”

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
Need impact -> copy a row's target_ref into impact_bundle(target) or navigation; impact_bundle(symbol) remains a legacy/ad-hoc route
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
BAD: find_declarations then go_to_definition (or both) as the default known-symbol loop
BAD: read_match with a match_id from another result_handle
BAD: read_match with a pre-edit result_handle after post-edit / reindex without re-search
BAD: repo-wide shell rg to "confirm" an explainable Frigg zero
BAD: throwaway shell rg on indexed src when Frigg is attached
BAD: calling tools only in a stale schema cache, not in live tools/list
BAD: invent workspace_index / workspace_reindex / deep_search from source or host descriptors
BAD: treat search_batch as one cheap multi-pattern scan / expect early-exit across probes
BAD: blame ranking/hybrid for staleness when watch is mode_off (often default stdio WatchMode::Off) without checking watch_status/profile
BAD: spawn multiple stdio Frigg processes against one repo and expect shared HTTP-style freshness
BAD: invent compose_evidence_packet / sealed evidence MCP tools — packets are skill-assembled
BAD: invent review_bundle / bug_trace_bundle / citation-service tools — impact_bundle is the composition template
BAD: invent product tool-order hacks or blame Frigg ranking when host Grep wins / MCP bridge is flaky
BAD: answer from hybrid rank-1 alone or treat ranking policy as a second agent skill
BAD: treat hybrid graph neighbors / graph_score as proven call edges (use incoming_calls / find_references)
BAD: claim Frigg-first in a Task/subagent without re-probing live tools/list for that spawn
BAD: treat parent-OK/child-missing Frigg (or empty Frigg server on this spawn) as ranking/hybrid/index failure — that is harness registration (FUT-003)
BAD: treat every Tool not found as inheritance — invented phantoms and extended-only names are surface honesty
BAD: invent bridge_health / subagent_mcp_status Frigg tools (cannot observe missing registration from inside Frigg)
```

---

## Done criteria

```text
After search: read_match (or read_file) before answering or editing.
After hybrid: exact search_text or search_symbol before proof.
After impact query: navigation evidence before textual whole-repo grep.
After surprising zero: trust zero_hit_reason / recovery fields before shell.
After edit: workspace freshness before reusing Frigg for touched paths; re-search before old handles.
```

Multi-hypothesis answers must cite at least one Frigg proof window with no parallel shell grep on indexed source while Frigg is healthy.

## Bounded-result truth

Before concluding that a short or empty response proves absence, read its `completeness` envelope:

- `unit` declares the row unit; `returned` is page-local; `total` is exact for the normalized
  request only when present. Absent `total` means coverage is unproven, never “at least this
  many.”
- `complete=true` means exact and exhausted, with no continuation or omission reasons. A final
  page can be complete even if its page-local `returned` is less than request-wide `total`.
- `truncated=true` is an intentional bound; inspect `truncation_reasons`. `complete=false` with
  `incomplete_reasons` signals diagnostics, ranked discovery, or navigation coverage that cannot
  establish exhaustiveness.
- Continue only when `completeness.continuation` is present, replaying the same normalized
  request with that opaque v2 token in `continuation`. It is bound to tool, request, session,
  repository scope, and snapshot. Do not combine it with legacy `resume_from`; old cursors are a
  compatibility input, while v2 is canonical output.

Search-specific reading rules:

- `search_text.total_matches` is raw occurrence cardinality; use `completeness.unit` and
  `completeness.total` for the shaped row collection. `count_only=true` intentionally has no
  `matches[]`. File-collapse modes count files, and `max_count_per_file` affects capped rows
  without redefining the raw occurrence total.
- `search_hybrid` is ranked discovery: total is absent, coverage is incomplete for
  `ranked_discovery`, and it has no exhaustive continuation. Pivot to exact tools before proof.
- For navigation, page completeness is independent of `mode`. `heuristic_no_precise` and
  `unavailable_no_precise` remain weaker or unavailable semantic coverage even if a returned page
  is otherwise complete. `find_references.total_matches` is the known pre-page active-mode total.
- `search_batch` exposes child truth in each `probe_summary.completeness`; `impact_bundle` does
  the same per included section. Read both child and aggregate envelopes before asserting a
  complete investigation.

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
| **Product support** | 2–8 probes, each a full search; `probe_summary` per probe; path:line dedupe; handles / authoritative `next_actions` (with deprecated lossy `suggested_next` compatibility projection). **Not** a single shared multi-query walk or early-exit fusion. |

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
| **Frigg path** | `search_hybrid(question)` → authoritative `next_actions[]` or noun pivot → `search_symbol` / `search_text` → proof read |
| **Fallback** | Skip hybrid if a likely symbol/string is already known. Never shell-grep as the precision layer after hybrid |
| **Proof** | Exact search, then `read_match` / `read_file` |
| **Done** | Never answer from hybrid rank-1 alone; exact Frigg evidence present |
| **Product support** | Compact pivots, `ranking_note`, `best_pivot_path`, and typed `next_actions`; semantic is **opt-in** (product default off) — optional accelerator only |

```text
1. search_hybrid("<user question>")
2. Read ranking_note (always-on in compact):
   - "discovery_only; confirm with exact search" → multi-channel hybrid; still never proof from rank-1
   - "discovery_only; lexical_only (semantic not contributing); …" → product default / semantic off /
     empty semantic channel — hybrid is still valid (lexical+graph); pivot sooner to exact tools
3. Do NOT answer from rank #1 alone
4. If `next_actions[]` is present, execute each named existing tool with exact `arguments`, following role/order/dependencies; otherwise extract nouns → search_symbol → scoped search_text
5. read_match / read_file for proof
```

```text
BAD:  search_hybrid → read_file(first hit) → answer
BAD:  ranking_note has lexical_only → abandon Frigg for shell rg
GOOD: search_hybrid → search_symbol/search_text → read_match
GOOD: lexical_only ranking_note → still Frigg exact tools; semantic is opt-in upgrade
```

**Semantic default (product):** runtime semantic is **off** unless the operator enables it. Hybrid without semantic is intentional (lexical + graph), not a broken server. Compact does **not** dump `semantic_status` / long readiness warnings — the mode cliff is only the short `ranking_note` token. Full `response_mode` keeps diagnostics for operators.

**Local MiniLM (`provider=local`, preset `offline-small`):** **offline smoke** / zero-key general embedder — not a code-specialized model. `semantic_status: ok` means vectors ran; still pivot hybrid → exact `search_text` / `search_symbol` before proof. Prefer cloud / `openai_compat` when you want a stronger semantic channel; do not chase “better hybrid rank-1” under MiniLM alone.

**Google Gemini (`provider=google`, preset `cloud-google`):** **credential peer** — use when `GEMINI_API_KEY` is already present. Not an unmeasured “best cloud for code” default over OpenAI. OpenAI-only sessions need not configure Google.

Prefer `next_actions[].tool` and exact `next_actions[].arguments` after hybrid; do not paste the original natural-language question into `search_symbol`. `suggested_next` is deprecated and lossy for at least two minor releases. Stale or mixed proof handles require rerunning the typed origin producer and choosing a fresh `match_id`; never reuse the old match. Frigg has no generic executor or automatic chaining endpoint.

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
| **Frigg path** | `search_symbol(name, path_class=runtime)` → **`go_to_definition(symbol=name)`** → proof |
| **Fallback** | No hybrid or unscoped grep when name is known. Do **not** call `find_declarations` first (or serially by default) |
| **Proof** | `read_match` or `read_file` (json/citation if citing) |
| **Done** | Starts at `search_symbol`; definition-first navigation with `symbol`, not path+line alone |
| **Product support** | Runtime-first defaults, `column`/`excerpt`, empty-`{}` rejection; both def/decl tools kept for LSP parity; heuristic `mode` is valid |

```text
DEFAULT LOOP (stop after proof unless decl≠def is the actual question):
1. search_symbol(name, path_class=runtime)  # prefer symbol + column from hit when present
2. go_to_definition(symbol=name)            # body / implementation anchor
3. If path+line only was used: check ambiguous_location / location_warning before editing
4. read_match OR read_file
5. If navigation mode is heuristic/unavailable: still usable — do NOT install scip-* mid-task

OPTIONAL BRANCH (not a numbered step after 2 by default):
  find_declarations(symbol=name)
  — only when declaration vs definition matters (headers, interfaces, re-exports, ambient decls)
  — never run both serially as the default known-symbol loop

BAD: go_to_definition + find_declarations every time and treat both rows as two anchors
BAD: pause the agent loop to install rust-analyzer / scip generators because precise is missing
GOOD: go_to_definition(symbol=…) for body anchors; declarations only when decl≠def matters
GOOD: treat NavigationMode heuristic as valid Frigg; precise is an upgrade when ready
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
| **Product support** | `summary` counts/top_paths, mode flags; **public thin composers** are `impact_bundle` (nav) and `search_batch` (multi-probe) — no further scenario bundles |

```text
PREFERRED: impact_bundle(symbol, path_class=runtime)
  → read summary first (counts / modes / top_paths) for plan
  → then proof via handles/lists; authoritative `next_actions` for tests pass + `read_match` (legacy `suggested_next` is compatibility-only)
  // full response_mode only forwards diagnostics to child nav/search — not a second next-step list
  // default bundle omits outgoing_calls (provisional) and does not embed tests (use canonical next_actions)

OR sequential:
search_symbol(anchor, path_class=runtime)
find_references(symbol, include_definition=false)
incoming_calls(symbol)              # who calls — reliable
outgoing_calls(symbol)              # trust=provisional (+ trust_note); confirm with read_file
find_implementations(symbol)        # traits/interfaces only
```

**Composition bar / surface growth (EXP-when-to-grow-surface A):** a new public “bundle/service”
tool is justified only when orchestration **cannot** be a skill loop without shared handles and
recovery budgets (same class as `impact_bundle` / `search_batch`). Prefer, in order:

1. **Skill loop** over existing tools (symbol → text → hybrid → batch → proof)
2. **Internal module / policy / ranking fix** (not agent-visible) when the miss is order/quality
3. **Thin composer** (`impact_bundle`, `search_batch` class) when multi-primitive orchestration needs
   shared handles/recovery the skill loop cannot share — not a substitute for fixing ranking (2)
4. **New public tool** last; never tool-per-scenario (`review_bundle`, `bug_trace_bundle`, citation
   services, packet MCP tools). Multi-claim review packets stay **skill-assembled**
   (see Technical review + `frigg://policy/evidence-packet.json`).

Do **not** invent tools to “fix” harness registration, ranking, or host Grep order.

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
| **Proof** | Bounded source window from the source revision observed when the pair was issued |
| **Done** | Internal proof stays on Frigg handles/paths |
| **Product support** | Revision-bound scoped handles; `STALE_HANDLE`, `MIXED_HANDLE`, and `STALE_PROOF_ANCHOR` failures |

```text
match_id is valid ONLY with its own result_handle from the SAME call.
match_id is scoped: search:m1, symbols:m1, hybrid:m1, nav:m1 (never reuse across tools).
handle_scope + handle_expires="session" mark the pairing lifetime.

Handle lifetime (session-scoped, not a durable citation id):
- Explicit reindex / detach / whole-repo cache wipe → handles for that repo drop (StaleHandle).
- Watch refresh with a known dirty set → only anchors on **dirty paths** drop; untouched-path
  bookmarks may still work. Known-empty success (noop refresh) does not wipe handles.
- Unknown dirty set (notify drop, failed refresh) → whole-repo handle wipe (conservative).
- After post-edit / use_live_disk / wait_watch→ready for paths you care about:
    **re-run search** before trusting an old result_handle for proof or citations.
  Do not chain pre-edit handles across a freshness transition for those paths.

`read_match` verifies the live source against the revision bound to the pair before returning
content. If the source changed, was deleted, or cannot be verified, it returns
`STALE_PROOF_ANCHOR` and no source bytes. Re-run the originating search/navigation tool and use
its new `result_handle` + `match_id`; do not retry the old pair. `read_file` is the explicit path
for current live content, not a way to refresh historical proof.

`STALE_HANDLE` → the pair expired or was invalidated; re-run search.
`MIXED_HANDLE` / foreign `match_id` → use the matching handle from the same call; if the handle
is known but the match vanished after a path-scoped watch drop, re-run search (do not treat it as
a cross-call mix-up alone).
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
| **Product support** | `recommended_action`, `gate_hint`, `runtime.watch_status` (reason + dual-class queue depth / dirty count), `runtime.tools_exposed`, optional `lexical_ready`/`semantic_ready`, dirty/changed paths |

```text
Call workspace(path=...) IF:
  - wrong-repo paths, unexplained zeros, index errors, multi-repo ambiguity, post-edit freshness
Skip workspace IF:
  - first search hits expected paths and index is ready

Agent health decision tree (branch here only — not full scorecards):
  1. recommended_action (primary)
  2. zero-hit recovery / ZeroHitIndex when a search returned empty
  3. optional lexical_ready / semantic_ready — never promote SCIP generator install mid-task
     (precise SCIP is an **optional accelerator**; heuristic nav is valid Frigg)
  4. runtime.watch_status.reason explains wait_watch (mode_off / no_lease / debouncing / refreshing / active);
     optional refresh_queue_depth / pending_dirty_path_count / oldest_pending_age_ms (dual-class only — no third queue)
  5. Transport: read `runtime.profile` + `watch_status.reason` — default stdio is often `mode_off`
     (watch Off); `no_lease` means watch is on but no session lease. Either way: not a ranking bug —
     path-scoped live-disk or switch to HTTP+serve for shared freshness; do not invent reindex tools

recommended_action=reindex means index substrate not Ready:
  - NOT a public MCP tool (tools/list has no reindex / workspace_reindex)
  - operator/CLI: `frigg index` (or attach-side ensure / operator lifecycle)
  - read gate_hint when present; do not invent a write tool or shell-grep the repo "to fix" it

Tool surface honesty:
  - Trust runtime.tools_exposed (or live tools/list) for this process
  - Do not call names only in host schema caches, skill memory, or source #[tool] attributes
  - tool_surface_profile is core|extended; **explore is core**; playbook tools only with `--features playbook` + extended
  - On Task/subagent spawn: re-probe Frigg registration before Frigg-first claims (see probe-on-spawn above)
  - Missing Frigg on child while parent works → harness inheritance (FUT-003), not Frigg product P0

Policy progressive disclosure:
  - Skill scenario cards are SSOT for agent routing
  - MCP frigg://policy/* resources are secondary/machine (hosts, support matrix, tool-surface)
  - Do not treat resources as a second full skill

Install surfaces (avoid three-doc drift):
  - **Skill** = sole scenario home (this file / host skill path)
  - **AGENTS.md** = lightweight pointer via `frigg adopt` (not a second skill)
  - **ideas/flows** research = not production routing
  - After monorepo skill changes: re-adopt managed blocks; reload host skill; `frigg adopt --check` in CI
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
| **Product support** | Zero-hit names repository; multi-repo disambiguation on same-rank symbols; recovery suggests `workspace` / `repository_id` |

```text
Multi-attach (common on HTTP):
  - Prefer repository_id on impact_bundle / nav when the symbol name is common across repos
  - DISAMBIGUATION_REQUIRED + target_selection.candidates → pick repo via repository_id or path+line
  - Do not invent federated cross-repo call graphs; one repo graph at a time
  - Wrong-repo zero → workspace(path=...) recovery, not shell grep
```

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
4. If wait_watch → read runtime.watch_status:
     - reason: mode_off / no_lease / debouncing / refreshing / active
     - refresh_queue_depth / pending_dirty_path_count / oldest_pending_age_ms when present
       (dual-class manifest_fast + semantic_followup only — there is no agent_hot third queue)
     - mode_off / no_lease: **not** full HTTP+watch freshness — use path-scoped live reads for
       touched paths; prefer loopback HTTP + `frigg serve` if multi-client post-edit freshness matters
     - high dirty count or rising age_ms → prefer path-scoped live reads on **touched** paths
       over busy-wait; low depth + debouncing/refreshing → brief wait then re-check workspace
     - do not "verify" with whole-repo rg or invent micro-retry loops
5. If reindex → CLI `frigg index` / operator lifecycle (not an MCP tool); optional path-scoped live reads while index rebuilds
6. **Handles after freshness:** do not `read_match` a pre-edit result_handle for touched paths.
     Re-run the originating search_text / search_symbol / navigation tool after ready (or after
     watch commits dirty paths) and use its new paired IDs. `STALE_PROOF_ANCHOR` is a
     fail-closed proof result: it returns no historical bytes, and `read_file` is only the
     explicit current-live-content path. Untouched-path handles may still resolve; treat that as
     opportunistic, not guaranteed across reindex or whole-repo invalidation.
BAD: workspace dirty → rg -n across the repo
BAD: recommended_action=reindex → call invented MCP workspace_reindex
BAD: wait_watch → invent a retry loop without reading watch_status
BAD: post-edit → read_match(old_handle) for a file you just changed
GOOD: workspace dirty → read_file(edited/path.rs) or wait for hot-path watch refresh
GOOD: wait_watch + pending_dirty_path_count high → path-scoped live read of touched files
GOOD: recommended_action=reindex → frigg index (CLI) or wait for operator; read gate_hint
GOOD: after ready → new search → new result_handle → read_match
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
Heuristic nav is **valid product behavior** — precise SCIP is optional acceleration, not a gate on “Frigg works.”

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
PREFERRED: impact_bundle(api, path_class=runtime) → summary then read_match clusters
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
| **Proof** | Every finding has path/line witness; prefer skill-assembled evidence packets |
| **Done** | Findings grounded in Frigg evidence, not shell grep transcripts |
| **Product support** | Citation reads; skill-side evidence packet shape (+ `frigg://policy/evidence-packet.json`); types `EvidencePacket*` are **not** a tool |

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
5. `outgoing_calls` — always `trust=provisional` + `trust_note` on the wire; confirm edges with body reads (does not upgrade wire trust to verified)

**Evidence packets are skill composition, not an MCP tool.** Frigg supplies witnesses
(search/nav/read). Agents assemble multi-claim JSON for review/security reports.
There is no `compose_evidence_packet` (or sealed server packet) on the public surface.
Machine schema: MCP resource `frigg://policy/evidence-packet.json`. Rust types
`EvidencePacket` / `EvidencePacketClaim` mirror the shape for hosts only.

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

**Explore** (core product surface, tier-3 for routing) — **anchored in-file only**:

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
4. Compact responses first; `response_mode=full` only for ranking diagnostics. Compact and full responses carry identical executable `next_actions` data; `suggested_next` is only a deprecated lossy compatibility projection.
5. Navigate with a copied row **`target_ref`** in `target` when one is present; use direct `symbol` or location input only for ad-hoc/compatibility calls.
6. Proof: `read_match` → `read_file`.
7. Cite: json/citation `read_file` or host Read when required.
8. Structure/explore only as tier-3 after anchors exist.

---

## Compact response rules

- `result_handle` + `match_id` → `read_match` (**same call only**; scoped ids like `search:m1`).
- Text-first `read_file` / `read_match` / `explore(zoom)`: no line prefixes in text mode.
- `presentation_mode=json` when you need `start_line`, `end_line`, bytes, or machine-readable fields.
- Hybrid compact may omit rich scores and semantic readiness dumps — still read `ranking_note`:
  - `lexical_only (semantic not contributing)` means mode cliff (default-off or empty semantic), not “Frigg failed”
  - `hybrid graph is ranking signal (not nav call edges)` means graph channel ≠ `incoming_calls` / SCIP nav — confirm with exact tools; optional per-match `graph_mode` (`projection` / `heuristic_symbol_graph` / `heuristic_implementation` / `unknown`)
  - Pivot to exact tools; do not shell-grep indexed source
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
| Callees | `outgoing_calls` (`trust=provisional`; confirm with read) |
| Trait / interface impls | `find_implementations` |
| Definition / body anchor | `go_to_definition(symbol=...)` (default; not path+line alone; trust `ambiguous_location`) |
| Declaration-only need | `find_declarations` only when decl≠def matters — not a serial default with go_to_definition |
| AST shape | `inspect_syntax_tree` + `search_structural` (tier-3) |
| In-file zoom (core) | `explore` — not a substitute for `search_text` |
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
- Evidence packet schema (skill composition only): MCP `frigg://policy/evidence-packet.json`
- Embedding model defaults / pad / offline facts + soft presets (`offline-small` = MiniLM **offline_smoke**; `cloud-google` = Gemini **credential_peer**; `cloud-openai` / `openai-compat-selfhost`) — curated catalog (no public leaderboard); not CLI aliases: MCP `frigg://policy/semantic-models.json`
