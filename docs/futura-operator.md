# Futura operator runbook

Operator-facing product-contract home for running Frigg under the Futura
evidence-layer rules. Diagnostic and policy guidance only — does not change
runtime behavior.

| Document | Role |
| --- | --- |
| [`docs/futura.md`](futura.md) | **Product contract** — tool shapes, `FUT-*` requirements, scenario flows, SLOs |
| [`docs/futura-roadmap.md`](futura-roadmap.md) | **Operational board** — phases, checkboxes, commit series |
| [`docs/futura-phase0-inventory.md`](futura-phase0-inventory.md) | Phase 0 freeze — live tools vs gaps |
| This file | Operator runbook: path class, fallback, ignore truth, adopt, re-probe |
| [`docs/operator-runbook.md`](operator-runbook.md) | Runtime lifecycle diagnostics (index, semantic, precise, watch) |
| `skills/frigg-first-code-search/SKILL.md` | Agent policy home (scenario-first routing) |

**Date:** 2026-07-08 · **Branch context:** `feat/futura`

---

## Product boundary (one screen)

Frigg owns source-backed **read-discovery** for attached repositories: adoption /
runtime status, indexed file discovery, search (text / hybrid / symbol / batch
when shipped), bounded reads, proof handles, navigation, outlines, freshness
signals, recovery grammar, and agent-facing instructions.

**Frigg does not own** mutation, tests, commits, PRs, or the user’s build
system. Git, cargo/npm/etc., CI, and host editor tools stay outside Frigg.

Harness-specific MCP registration or tool-order flukes (for example one-off
schema-cache issues) are **outside the product contract** (`FUT-003`).

---

## Path-class and positive fallback matrix

Mirrors the production skill top-screen **Positive fallback boundary**. Use this
when deciding “Frigg, direct read, or shell?”

### Path classes (indexed surfaces)

Frigg classifies repository-relative paths roughly as:

| `path_class` | Typical paths | Prefer |
| --- | --- | --- |
| `runtime` | `src/**`, `crates/**`, `app/**`, `lib/**`, root source files, framework runtime roots | Search + read with Frigg; default for implementation questions |
| `support` | `tests/**`, fixtures, `vendor/**`, `node_modules/**`, `generated/**`, views in some layouts | Frigg when indexed; shell only if unindexed or build artifact |
| `project` | manifests (`Cargo.toml`, …), configs, docs/skills when **not** gitignored | Frigg search when indexed; `read_file` when path known |
| *unindexed / generated* | `target/**`, `.frigg/**`, `.git/**`, gitignored trees | Shell / direct disk — not Frigg search |

Exact classification is implemented in `crates/cli/src/path_class.rs`. When in
doubt for implementation work, scope with `path_class=runtime` and/or
`path_regex` under runtime roots (see [Default `path_regex`](#default-path_regex-for-implementation-questions)).

### Positive fallback matrix

| Class | Always shell / host | Frigg or direct-read | Never shell as trust patch |
| --- | --- | --- | --- |
| **Git** | `git status`, `git diff`, commit, branch, merge | — | — |
| **Build / test / package manager** | command **output** and logs | — | — |
| **Generated / ignored dirs** | `target/**`, build out, ignored trees | — | — |
| **Live-disk after write** | When freshness is not proven: **touched paths only** | Frigg after watch / reindex ready | Repo-wide `rg` “to verify” |
| **Authored manifests / config** | — | Prefer Frigg `search_text` / `read_file` when indexed; direct read if path known or unindexed | Whole-repo shell grep for a known file path |
| **Project docs / fixtures** | — | Frigg when indexed; direct read when path known or gitignored | Shell-confirming an explainable Frigg zero on indexed source |
| **Newly created files** | — | Direct read until watch confirms ingestion; then Frigg | Assuming index is fresh without `workspace` when search looks wrong |
| **Indexed runtime source** | Only if Frigg unregistered / unavailable | `search_*`, nav, `read_match` / `read_file` | Parallel shell grep in the same turn as Frigg; “confirm” scoped zero without stale/unindexed reason |
| **References / call graph** | — | `find_references`, `incoming_calls`, … when anchor exists | Shell `rg` as the primary reference pass when Frigg is healthy |

**Always shell or host tool (skill list):**

- `git status`, `git diff`, commit and branch operations
- build output, test output, package-manager output
- generated or ignored directories (e.g. `target/**`)
- explicit live-disk verification after a write when freshness is not proven
- Frigg unavailable / unregistered

**Frigg or direct read (depending on path and goal):**

- small authored manifests and config
- project docs and fixtures when indexed, or direct read when path is known
- newly created files before watch confirms ingestion
- ignored paths that exist on disk but are not in the index

**Never shell as a trust patch:**

- source search under indexed runtime paths when Frigg is registered
- “confirming” a scoped Frigg zero-hit without a stale or unindexed reason
- references or call analysis when a symbol anchor exists

```text
BAD:  Frigg search_text zero under path_regex=^crates/ → shell rg to “double-check”
GOOD: trust scope/index diagnostics (or workspace gate) → broaden scope / adopt / wait / live-read touched paths only

BAD:  hybrid rank-1 → answer
GOOD: hybrid → search_symbol / search_text → read_match

BAD:  parallel shell grep + Frigg on indexed src in one turn
GOOD: search_batch (when available) or parallel Frigg probes only
```

---

## Proof vs citation vs evidence packet

Split read modes by **audience** (mirrors skill proof / citation cards and
`futura.md` §9).

### Proof read (internal understanding)

```text
search_* / nav hit
  → read_match(result_handle, match_id)   # same response only
  → read_file(path, start_line, end_line) when no handle
```

- Text mode returns raw source (no line prefixes) — fine for internal proof.
- `match_id` is valid **only** with its own `result_handle` from the **same** call.
- Do not answer or edit from hybrid rank alone; exact pivot + proof window first.

### Citation read (user-facing `start:end:path`)

```text
read_file(path, start_line, end_line, presentation_mode=json)
  → use start_line / end_line in the citation fence

# when available:
presentation_mode=citation   # LINE|content for citation-trained agents
```

Host/editor Read is acceptable when line-prefixed citation is required and Frigg
citation mode is absent — without abandoning Frigg for every internal proof.

### Evidence packet (review / security / multi-claim)

For multi-finding reports (`FUT-022` — `EvidencePacket` types + skill shape;
**no live MCP packet tool** yet), each claim carries a path/line witness:

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
    }
  ]
}
```

Prefer Frigg handles over shell transcripts as witnesses.

| Need | Branch |
| --- | --- |
| Understand behavior before edit | Proof (`read_match` → `read_file`) |
| Cite lines to the user | Citation (`presentation_mode=json` or `citation`) |
| Multi-claim review / security report | Evidence packet of path/line witnesses |

---

## Workspace gate, not preamble

`workspace` (and session attach / current-status surfaces) is a **trust gate**,
not a mandatory first tool on every turn (`FUT-007`).

**Call `workspace(path=...)` when:**

- session may be detached or default repo is wrong
- multi-repo ambiguity
- index errors or unexplained zero-hits
- post-edit freshness for touched paths is uncertain

**Skip `workspace` when:**

- first search already hits expected repository paths
- index is known ready for the task

```text
Call workspace IF trust changes (wrong repo, zero, stale, multi-repo, post-edit).
Skip workspace IF the first Frigg hit is already in the expected tree.
After gate: return to the real scenario tool (search / nav / proof) — do not
treat workspace as the answer.
```

`workspace.freshness` is authoritative; the older `recommended_action`, `gate_hint`, and
`fresh_enough_for` fields are derived compatibility projections retained for two minor releases.
Read all three axes before choosing a recovery:

| Authoritative state | Action |
| --- | --- |
| `snapshot=ready`, `dirty_scope=clean` | Use the snapshot on HTTP **or** stdio. Do not wait. |
| Ready, known changed paths, leased `debouncing` / `refreshing` | `post_edit=wait_for_refresh`: wait briefly, then recheck `workspace`. |
| Ready, dirty, `mode_off`, `no_lease`, `retry_backoff`, `blocked`, or `notify_degraded` | `can_converge_by_waiting=false`: direct-read touched paths; snapshot remains useful for untouched paths. |
| `snapshot=missing`, `uninitialized`, or `error` | Run CLI/operator `frigg index`; this is not a public MCP request. |
| `snapshot=detached` | Adopt the repository. |
| `snapshot=unavailable` | Restore Frigg/service state before retrying. |

HTTP is the shared-runtime transport and can carry leased watch refresh. Stdio is a valid
single-client/ephemeral transport; its default watch is commonly `mode_off`, not broken. Do not
blame ranking for a non-waitable stdio edit: use a touched-path live read or move shared work to
HTTP. When recovery includes canonical `next_actions`, call the named existing MCP tool with its
exact arguments; legacy suggestions are compatibility-only. After a touched edit, rerun the
search/navigation producer before using `read_match` as proof.

---

## `.gitignore` behavior

Frigg **follows `.gitignore`** (and related ignore rules; hard-excludes include
`.frigg`, `.git`, and `target` — see README security notes).

### What operators must know

1. **Ignored paths are absent from indexed search.** A file can exist on disk
   and still never appear in `search_text` / `search_symbol` / `list_files` hits.
2. **Local `/docs/` is a common case.** This repository gitignores `/docs/`.
   Operator and product-contract files under `docs/` (including this runbook and
   `futura.md`) are **not** discoverable via Frigg index search. That is correct
   ignore behavior, not a ranking bug (`FUT-005` dogfood case in phase-0
   inventory).
3. **When to direct-read:** known path on disk that is ignored or not yet
   ingested; small config/manifest by path; post-create before watch.
4. **When not to shell-grep the whole repo:** indexed runtime source while Frigg
   is healthy — use scoped Frigg tools instead.
5. **How to include research paths intentionally:**
   - stop ignoring the path (adjust `.gitignore` / `.ignore`), **or**
   - move content under an indexed tree, **or**
   - keep it ignored and **direct-read** / shell only for those paths.
6. **Skills and checked-in docs** that are *not* ignored remain searchable; do
   not assume every `docs/` tree is ignored — only paths matching ignore rules.

```text
Default to path_regex under runtime roots for code questions.
Search docs or research notes only when the task is about docs.
If the path is ignored by .gitignore, use direct read or adjust ignore rules.
```

---

## Default `path_regex` for implementation questions

Prefer **repo runtime roots**, not unscoped whole-tree probes:

| Layout | Prefer |
| --- | --- |
| Typical app (`src/`) | `path_regex='^src/'` (or tighter, e.g. `^src/catalog/`) |
| This Frigg monorepo | `path_regex='^crates/'` (often `^crates/cli/src/`) |
| Multi-root / framework | Real runtime dirs (`app/`, `lib/`, `packages/*/src/`, …) |

Reasons:

- Scoped source questions are more precise.
- Unscoped search may rank project docs, skills, or specs before runtime code.
- This is **not** a ranking workaround for ignore — gitignored `/docs/` already
  never enters the index.

Also default `path_class=runtime` (or `include_tests=false` when available) for
implementation questions unless tests are in scope.

---

## `frigg adopt`: default vs `--policy expanded`

Install managed agent instructions and MCP client config into a project:

```bash
# Preview
frigg adopt --target agents-md --target mcp-project --dry-run

# Default: lightweight AGENTS pointer → skill owns detail
frigg adopt --target agents-md --target mcp-project

# Opt-in: compact in-repo routing policy (picker + shell→Frigg one-liners)
frigg adopt --target agents-md --policy expanded
```

| Policy | Asset | Content |
| --- | --- | --- |
| **Default** (omit `--policy`) | `crates/cli/assets/frigg-directive.md` | Lightweight Frigg-first pointer to `frigg-first-code-search` skill; no full scenario tables |
| **`expanded`** | `crates/cli/assets/frigg-directive-expanded.md` | Compact picker + shell→Frigg one-liners + short fallback boundary; still points at skill for full cards |

Other useful flags (README): `--all`, `--check` (CI drift), `--uninstall`,
`--force`, targets such as `claude-md`, `cursor`, `mcp-project`, `hook`.

Managed MCP JSON entries use **loopback HTTP only** (`type: http`, default
`http://127.0.0.1:37444/mcp`) — keep `frigg serve` running for shared clients.
`frigg adopt` does **not** write stdio `command` Frigg entries for managed MCP
targets (stdio remains available for manual single-client configs).

**Dual transport contract (not a quality hierarchy):**

| Transport | Product intent | Freshness |
| --- | --- | --- |
| Loopback HTTP + watch | Shared runtime, multi-client, subagents | Full post-edit / lease freshness |
| Stdio (client-owned process) | One local client | Valid different contract; Auto watch often off |

Stdio is not “broken Frigg.” Agents must not treat ranking or hybrid as the
cause of staleness when `watch_status.reason` is `mode_off` / `no_lease` on a
stdio session. Prefer path-scoped live-disk or move to HTTP when shared
freshness is required. Adoption, watch freshness, and cache reuse are
**HTTP-first** shared-runtime behaviors.

### Local routing stats (`FUT-024`)

Opt-in, process-local only (no cloud telemetry):

```bash
FRIGG_ROUTING_STATS=1 frigg serve
# MCP resource: frigg://stats/routing
# or from another terminal (same process only if stats were recorded there):
frigg stats
frigg stats --json
```

Counters: tool call counts by name, zero-hit count, recovery issued, handle
failures, workspace gate uses. Healthy mix: Frigg search/nav dominant on
indexed source; recovery fields used after zeros; near-zero shell trust patches.

### SLO snapshot (`FUT-023`)

Tracked snapshot + probe:

- Snapshot: `crates/cli/assets/futura-slo-snapshot.md`
- Probe (binding FUT-023): `scripts/futura_slo_probe.sh [OUTPUT_MD]`
  - Runs **release** `futura_bench` head-to-head warm `search_text` vs subprocess `rg`
  - Gate: `frigg.p95_ms <= rg.p95_ms * 1.5` (debug soft 2s only; 1.25× flaked under load)
  - Alias: `cargo futura-bench` (release)
  - Requires local `rg` installed

Small-N synth fixture posture vs local `rg`. Large-repo monorepo p95 is
**deferred** (not a silent green).

**Do not** paste the full skill into default `AGENTS.md`. Larger in-repo policy
is **opt-in only** via `--policy expanded`.

---

## Re-probe checklist (after product or policy changes)

Extracted from [`docs/futura.md`](futura.md) (Re-probe checklist). Run after any
tool contract, skill, or recovery change:

1. Re-run `frigg-futura-bench` fully:
   - Debug contracts: `cargo test -p frigg --test futura_bench -- --nocapture`
   - Binding SLO: `cargo futura-bench` or `scripts/futura_slo_probe.sh`
2. Re-audit skill top screen against the scenario picker and BAD list
   (`cargo test -p frigg --test futura_routing_scorecard`).
3. Re-check zero-hit and recovery samples for search and navigation.
4. Re-check handle stale/mixed failures (`handles_futura` tests).
5. Re-check hybrid never-as-proof behavior.
6. Re-check multi-language representative probes (lang board in bench).
7. Update operator SLO snapshot if latency surfaces changed
   (`FUTURA_SLO_OUT=crates/cli/assets/futura-slo-snapshot.md cargo futura-bench`).
8. Update roadmap checkboxes and `futura.md` if contracts shifted.

Also when the product contract changes:

1. Edit `docs/futura.md` first.
2. Update requirement IDs and scenario cards as needed.
3. Adjust [`docs/futura-roadmap.md`](futura-roadmap.md) checkboxes in the same change.
4. Extend `crates/cli/tests/futura_bench/**` when contracts are behavioral.
5. Run this re-probe list.

---

## `frigg-futura-bench` (shipped)

Named evaluation harness (in-process **shipped** `FriggMcpServer` handlers, not
a reimplementation):

```bash
cargo test -p frigg --test futura_bench -- --nocapture   # debug: contracts + soft SLO
cargo futura-bench                                       # release: contracts + FUT-023 gate
```

Surfaces: dogfood (shaped fixture or `FUTURA_BENCH_DOGFOOD_ROOT`), synth, multi-lang.
CI job `futura-bench` installs `rg` and runs **release**.

Note: this is **not** an HTTP `tools/call` wire protocol suite; it exercises the
same handler methods the MCP server exposes.

---

## Operator quick links

| Topic | Where |
| --- | --- |
| Product contract | [`docs/futura.md`](futura.md) |
| Phase board | [`docs/futura-roadmap.md`](futura-roadmap.md) |
| Phase 0 inventory | [`docs/futura-phase0-inventory.md`](futura-phase0-inventory.md) |
| Runtime lifecycle (index / semantic / precise / watch) | [`docs/operator-runbook.md`](operator-runbook.md) |
| Agent routing policy | `skills/frigg-first-code-search/SKILL.md` |
| Deep tool parameters | skill `references/` (not skill top screen) |
| Adopt / install | README “Add client configuration” |

---

## Related idea notes (optional depth)

| Note | Topic |
| --- | --- |
| `docs/ideas/non-code-fallback-boundary.md` | Path-class / shell boundary research |
| `docs/ideas/workspace-adoption-gate.md` | Gate vs preamble probe notes |
| `docs/ideas/flows/proof-and-citation-read.md` | Proof vs citation flows |
| `docs/ideas/zero-hit-and-index-freshness.md` | Zero-hit / freshness |
