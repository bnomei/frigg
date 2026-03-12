# frigg: deterministic local code evidence over MCP

Frigg is a local-first code-evidence engine delivered primarily through MCP.
It helps agents and developer tools answer code questions with reproducible, source-backed results across one or more local repositories.

Frigg is:
- engine-first: deterministic local evidence from manifests, search, graph overlays, and optional semantics.
- MCP-delivered: a small, explicit read-only tool surface with versioned JSON schemas.
- CLI-operated: one deployable binary (`frigg`) built from a Rust workspace.

## Product Rings

### Stable core

The default public promise:
- discover or attach repositories (`list_repositories`, `workspace_attach`, `workspace_current`)
- read files safely inside allowed roots (`read_file`)
- search text and symbols (`search_text`, `search_symbol`, `search_hybrid`)
- navigate references and definitions (`find_references`, IDE-style navigation tools)
- persist provenance/events for replay and auditing

### Optional accelerators

Useful, but not required for Frigg to be valuable:
- semantic retrieval and embedding-backed ranking
- external SCIP ingestion for more precise navigation
- built-in watch mode and changed-only refreshes

### Advanced consumers

Important, but not the product core:
- bounded follow-up plus deep-search runtime tools behind [`crates/cli/src/mcp/advanced.rs`](./crates/cli/src/mcp/advanced.rs)
- the self-improvement loop under [`skills/`](./skills/)
- external replay, holdout, and repo-scanning harnesses under [`var/self-improvement/`](./var/self-improvement/)

## Support Matrix

Frigg is intentionally conservative about first-class support claims.

| Surface | Search + outline | Navigation | Semantic retrieval | Status |
| --- | --- | --- | --- | --- |
| Rust | first-class | first-class | first-class when enabled | first-class |
| PHP | first-class | first-class | first-class when enabled | first-class |
| Blade | first-class template surface | bounded source/template navigation and structural search | bounded template retrieval; no Laravel runtime overlays | first-class template surface |
| TypeScript / TSX | baseline runtime symbol, outline, and structural support | bounded heuristic/source navigation; precise SCIP parity not claimed | experimental when enabled; not semantic-parity | baseline runtime surface |
| Python | baseline runtime symbol, outline, and structural support | bounded heuristic/source navigation; precise SCIP parity not claimed | experimental when enabled; not semantic-parity | baseline runtime surface |
| Go | baseline runtime symbol, outline, and structural support | bounded heuristic/source navigation; precise SCIP parity not claimed | experimental when enabled; not semantic-parity | baseline runtime surface |
| Kotlin / KTS | baseline runtime symbol, outline, and structural support | bounded heuristic/source navigation; precise SCIP parity not claimed | experimental when enabled; not semantic-parity | baseline runtime surface |
| Lua | baseline runtime symbol, outline, and structural support | bounded heuristic/source navigation; precise SCIP parity not claimed | experimental when enabled; not semantic-parity | baseline runtime surface |
| Roc | baseline runtime symbol, outline, and structural support | bounded heuristic/source navigation; precise SCIP parity not claimed | experimental when enabled; not semantic-parity | baseline runtime surface |
| Nim | baseline runtime symbol, outline, and structural support | bounded heuristic/source navigation; precise SCIP parity not claimed | experimental when enabled; not semantic-parity | baseline runtime surface |

If a language or framework is not marked first-class here, Frigg should not market it as if it already has parity.

## Language Onboarding Policy

New languages move through staged capability upgrades instead of being announced as first-class early.

- `witness_only`: path/surface evidence or external artifacts may be useful, but Frigg does not claim first-class runtime support yet.
- `runtime_l1_l2`: runtime symbol, outline, and heuristic navigation behavior is stable enough for public use.
- `precise_l3`: precise SCIP-backed navigation is validated and part of the supported story.
- `semantic_parity`: semantic chunking, indexing, ranking, watch/reindex behavior, and provenance all align with the first-class languages.

Current next-language priority: TypeScript / TSX.
Python, Go, Kotlin / KTS, Lua, Roc, and Nim now share the same baseline runtime surface, but TypeScript / TSX stays ahead in the follow-on queue for precise and semantic-parity work.

## Core Concepts

- `workspace roots`: local directories Frigg is allowed to index/read.
- `repository IDs`: runtime IDs (`repo-001`, `repo-002`, ...) derived from startup root order plus any later `workspace_attach` calls.
- `snapshots + file manifests`: persisted index state used for deterministic reindex behavior.
- `provenance events`: stored tool-call evidence for replay/debugging.
- `deterministic contracts`: versioned tool schemas and error taxonomy in `contracts/`.

## Evidence Channels

Frigg’s retrieval model is better understood as evidence channels than as “one smart search”:

- `lexical + manifest`: deterministic file universe, literal/regex matches, and symbol/search anchors.
- `graph + precise`: symbol relations, references, call edges, and SCIP-backed overlays when available.
- `semantic`: optional embedding-backed recall and reranking.
- `path + surface witnesses`: framework- and runtime-aware path evidence such as routes, providers, workflows, tests, Blade views, or Livewire components.

These channels are blended into hybrid results, but they are not interchangeable. Lexical and graph evidence stay the grounding layer; semantic is an optional accelerator. See [`docs/architecture.md`](./docs/architecture.md) for the durable vocabulary and layer boundaries.
Release-readiness benchmark coverage now includes cached graph-backed hybrid grounding, semantic disabled/degraded hybrid control paths, and direct sqlite-vec top-k storage retrieval, so latency reports track both mixed-channel search behavior and the local vector hot path instead of semantic-only variants.
At runtime, Frigg now keeps these channels as first-class `EvidenceHit` and `ChannelResult` data with shared anchors and channel health, instead of collapsing witness evidence into lexical state before MCP metadata or audit paths can see it.

## Install And Build

### From source
```bash
git clone <your-frigg-repo-url>
cd frigg
cargo build --release -p frigg
```

### Local install from this repo
```bash
cargo install --path crates/cli
```

Deploy artifact:

```text
target/release/frigg
```

## Quickstart

### 1) Build
```bash
just build
# or: cargo build -p frigg
```

### 2) Initialize and index a workspace
```bash
just init .
just reindex .
just verify .
```

Changed-only reindex:
```bash
just reindex-changed .
# or: cargo run -p frigg -- reindex --changed --workspace-root .
# or: frigg reindex --changed --workspace-root .
```

Notes:
- `--changed` rebuilds the current manifest from file metadata, diffs it against the latest persisted snapshot, and only rehashes suspect `added + modified` files before treating them as changed.
- Deleted files are tracked separately.
- If nothing changed and a prior manifest exists, Frigg reuses the previous `snapshot_id` instead of writing a new one.
- Built-in watch mode now exists for local MCP runs. HTTP still defaults to `--watch-mode auto`; stdio now defaults to `--watch-mode off` so one-shot agent spawns do not each start their own watcher.
- `--watch-mode auto` enables the background changed-only watcher for stdio and loopback HTTP, but keeps it disabled for non-loopback HTTP.
- If the latest manifest is missing or stale at startup, built-in watch mode queues one immediate `manifest_fast` changed-only refresh before waiting for new filesystem events.
- Watch scheduling is class-aware and fair across roots: Frigg keeps one conflicting refresh per root, lets `manifest_fast` work run alongside an unrelated root's `semantic_followup`, and only queues semantic work after the manifest is current.
- External watchers are still useful for multi-repo fan-out, editor-owned lifecycle, or when you want reindex scheduling outside the Frigg process.
- When Frigg serves MCP over stdio and `RUST_LOG` is unset, it defaults tracing to `error` so raw clients do not need special stderr-drain handling. Set `RUST_LOG=info` if you want startup/watch logs.

Built-in watch options:
```bash
# HTTP/daemon-style defaults shown explicitly:
cargo run -p frigg -- --mcp-http-port 37444 --watch-mode auto --watch-debounce-ms 750 --watch-retry-ms 5000

# stdio already defaults to watch off; this is the explicit equivalent:
cargo run -p frigg -- --watch-mode off
```

Runtime profiles surfaced via `workspace_current.runtime.profile`:
- `stdio_ephemeral`: default one-shot stdio behavior with no warm-state promise.
- `stdio_attached`: stdio session that intentionally opts into warm local state, typically via built-in watch.
- `http_loopback_service`: preferred persistent local service mode on `127.0.0.1` or `localhost`.
- `http_remote_service`: non-loopback HTTP mode; explicit remote-bind opt-in plus auth token still required.

External watcher example:
```bash
watchexec -w crates -w docs -w README.md -e rs,toml,md -- just reindex-changed .
```

If you pair an external watcher with a running Frigg server, disable the built-in watcher with `--watch-mode off` to avoid double scheduling.

`watchexec` is usually not the bottleneck here; `reindex --changed` still metadata-scans the workspace to rebuild the manifest, but unchanged files now reuse prior digests and only suspect paths are rehashed. Semantic indexing can still dominate runtime when enabled.

### 3) Optional: add external SCIP for precise navigation

Frigg consumes SCIP artifacts, but it does not generate them.
If you want precise-first navigation instead of heuristic-only fallback, generate `.scip` files with an external indexer and place them under `.frigg/scip/` at the repository root.

Benefits:
- more accurate `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls`
- better handling for relationships that identifier-token heuristics miss, such as trait/interface implementations, import or re-export targets, call edges, and non-trivial declaration anchors
- explicit precise-versus-heuristic metadata when you need to audit why a navigation answer looks weak

Frigg currently discovers both binary `.scip` and JSON `.json` artifacts under `.frigg/scip/`.
Run generators from the repository root so document paths line up with Frigg's repository-relative path contract.

Create the artifact directory:
```bash
mkdir -p .frigg/scip
```

Rust:
```bash
rust-analyzer scip . > .frigg/scip/rust.scip
```

PHP:
```bash
composer require --dev davidrjenni/scip-php
vendor/bin/scip-php
mv index.scip .frigg/scip/php.scip
```

TypeScript / TSX:
```bash
npm install -g @sourcegraph/scip-typescript
npm install
scip-typescript index
mv index.scip .frigg/scip/typescript.scip
```

Python:
```bash
npm install -g @sourcegraph/scip-python
# activate your virtualenv first when applicable
scip-python index . --project-name="$(basename "$PWD")"
mv index.scip .frigg/scip/python.scip
```

Notes:
- regenerate these artifacts when the source changes materially
- if navigation metadata reports `precise_absence_reason=no_scip_artifacts_discovered`, check `.frigg/scip/` first
- today Frigg's strongest validated runtime/query surface is Rust, PHP, and Blade for source-backed symbol/search workflows; TypeScript / TSX, Python, Go, Kotlin / KTS, Lua, Roc, and Nim now ship baseline runtime symbol/outline/structural support, while precise SCIP-backed navigation remains validated for Rust and PHP
- TypeScript / TSX precise parity remains the next follow-on priority, and the other baseline runtime languages are still not claimed as precise-parity or semantic-parity surfaces
- generating a `.scip` artifact for TypeScript / TSX, Python, Go, Kotlin, Lua, Roc, or Nim does not by itself make those languages first-class in Frigg's public support matrix

### 4) Run as MCP server

Stdio transport (default):
```bash
just run
# or: cargo run -p frigg --
```

Notes:
- If no `--workspace-root` values are passed, stdio starts detached and does not create local Frigg state until `workspace_attach` is called explicitly.
- For Codex-style stdio MCP clients, prefer launching `frigg` with no startup `--workspace-root` args. That lets the MCP handshake complete before session-local workspace attach/status logic runs.
- If Frigg starts without startup roots, the session stays detached until `workspace_attach` is called explicitly.
- `workspace_current` is the read-only runtime status tool: it returns the session default repository, all attached repositories, runtime profile, watch/index health, active or recent runtime tasks, and recent provenance summaries.
- Repository index `health.lexical` and `health.semantic` in `workspace_current` now reuse the same shared snapshot freshness semantics as watch/search startup status and can also surface live semantic integrity drift: expect reasons such as `missing_manifest_snapshot`, `stale_manifest_snapshot`, `manifest_valid_no_semantic_eligible_entries`, `semantic_snapshot_missing_for_active_model`, and `semantic_vector_partition_out_of_sync` when the live semantic corpus and derived sqlite-vec rows drift apart.
- Built-in watch runtime tasks now distinguish `watch_manifest_fast` (`changed_reindex`) from `watch_semantic_followup` (`semantic_refresh`) in `workspace_current.runtime.active_tasks` and `workspace_current.runtime.recent_tasks`.
- When `RUST_LOG` is unset, stdio MCP launches default to an `error` tracing filter so raw clients do not need to drain routine startup/watch logs from stderr.
- Set `RUST_LOG=info` or `RUST_LOG=debug` if you want startup and watch diagnostics over stdio.
- Stdio defaults to `--watch-mode off`; pass `--watch-mode auto` or `--watch-mode on` if you want built-in changed-only reindex scheduling.

Codex config example:
```toml
[mcp_servers.frigg]
command = "/absolute/path/to/frigg/target/release/frigg"
args = []
```

Bootstrap note:
- Keep repo bootstrap explicit: run `frigg init --workspace-root <repo>` and `frigg verify --workspace-root <repo>` per repository as needed. Do not encode repo-specific `--workspace-root` args into stdio client config.

HTTP transport (loopback token optional; non-loopback requires auth token):
```bash
just run-http 37444
# equivalent:
cargo run -p frigg -- \
  --mcp-http-port 37444 \
  --mcp-http-host 127.0.0.1
```

Loopback with explicit token:
```bash
just run-http 37444 127.0.0.1 change-me
# equivalent:
cargo run -p frigg -- \
  --mcp-http-port 37444 \
  --mcp-http-host 127.0.0.1 \
  --mcp-http-auth-token change-me
```

HTTP MCP endpoint:
- `POST /mcp`

Remote bind requires explicit opt-in and auth token:
- add `--allow-remote-http`
- keep `--mcp-http-auth-token` set (or `FRIGG_MCP_HTTP_AUTH_TOKEN` env var).

HTTP attach-first flow:
- HTTP can start with zero `--workspace-root` flags.
- If Frigg started without startup roots, the session stays detached until `workspace_attach` is called explicitly.
- Call `list_repositories`; if it still returns an empty list or you need a different session-local default repository, call `workspace_attach`.
- Call `workspace_current` to confirm the session default repository and inspect runtime/task status when needed.

### 5) Optional: enable semantic retrieval

Semantic runtime is disabled by default.

Why:
- enabling it without a configured provider, model, and API key would make startup fail,
- semantic indexing/search can issue external provider calls and add latency/cost,
- Frigg should still start and work in lexical/graph-only mode with zero provider setup.

Semantic runtime remains optional and non-core.
The grounding layer is still lexical, graph, and path/surface witness evidence even when semantic retrieval is enabled.

OpenAI example:
```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=openai
export OPENAI_API_KEY=...
```

Google example:
```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=google
export GEMINI_API_KEY=...
```

Default provider models:
- `openai` defaults to `text-embedding-3-small`
- `google` defaults to `gemini-embedding-001`
- set `FRIGG_SEMANTIC_RUNTIME_MODEL` if you want to override either default explicitly

Codex MCP config example:
```toml
[mcp_servers.frigg]
command = "/Users/you/Sites/frigg/target/release/frigg"
args = []

[mcp_servers.frigg.env]
FRIGG_SEMANTIC_RUNTIME_ENABLED = "true"
FRIGG_SEMANTIC_RUNTIME_PROVIDER = "openai"
OPENAI_API_KEY = "..."
```

Notes for Codex:
- the `frigg` MCP subprocess inherits semantic setup from the `mcp_servers.frigg.env` table, not from your interactive shell unless `cx` was launched from that shell,
- keep `args = []` for stdio startup; do not add `--workspace-root` there,
- after changing Codex MCP config, restart `cx` so the Frigg subprocess picks up the new env.

Enable and populate embeddings for an existing workspace:
```bash
just reindex .
just run
```

Equivalent direct commands:
```bash
frigg reindex --workspace-root .
frigg
```

Important:
- run one `reindex` pass after turning semantic runtime on for an already indexed workspace,
- `reindex --changed` is allowed for the first semantic backfill: if the active `(repository, provider, model)` tuple has no live semantic head yet, Frigg escalates that pass to a full semantic rebuild even when the manifest snapshot is reused,
- after the first semantic population, `reindex --changed` continues to advance the same live corpus incrementally.
- if you enabled semantics in Codex config, run the full `reindex` before expecting `search_hybrid` to contribute semantic scores in MCP sessions.
- semantic storage is one live corpus per `(repository, provider, model)` keyed by `semantic_head`; changed-only refreshes advance that corpus in place instead of keeping steady-state semantic snapshot partitions.
- Frigg no longer falls back to older semantic snapshots. If the active manifest snapshot is not covered for the active provider/model, runtime health and search surface that missing live corpus directly.
- Manifest snapshot retention is bounded to the latest `8` per repository by default while protecting any active `semantic_head` snapshot, and provenance retention is bounded to the latest `10_000` events.
- `embedding_vectors` is a derived sqlite-vec live projection, not a snapshot-partitioned source of truth. If `workspace_current` or repository health reports `semantic_vector_partition_out_of_sync`, use the storage repair surface to rebuild sqlite-vec from the live semantic corpus.

Semantic reindex troubleshooting:
- semantic embedding failures now report `batch_index`, `total_batches`, `batch_size`, first/last chunk anchors, and sanitized request metrics such as `inputs`, `input_chars_total`, `body_bytes`, and `body_blake3`,
- those diagnostics are intentionally content-safe: they do not include raw chunk text or API keys,
- if a full semantic reindex still fails, use that batch context first before blaming `search_hybrid` ranking, because Frigg may still have zero persisted semantic embeddings.
- if `workspace_current` or repository health reports `semantic_vector_partition_out_of_sync`, the live semantic corpus and derived sqlite-vec projection diverged; run the storage repair surface to rebuild sqlite-vec from the live semantic corpus before blaming ranking.
- if `search_hybrid` reports `semantic_status=unavailable`, Frigg could not find a live semantic corpus for the active repository/provider/model combination and ranked the result set from lexical and graph signals only.

If semantic runtime is enabled correctly and embeddings exist, `search_hybrid` can return non-zero semantic scores.
If startup succeeds but `search_hybrid` still reports `metadata.semantic_status=disabled`, the running Frigg process is not seeing the semantic runtime env/config you expect.
`metadata.channels.semantic.status` is the canonical semantic channel-health field, while `metadata.semantic_status` remains the flat compatibility mirror.
`metadata.semantic_enabled` only means semantic evidence actually contributed to at least one returned match.
`metadata.channels.semantic.candidate_count`, `metadata.channels.semantic.hit_count`, and `metadata.channels.semantic.match_count` are the canonical counters; the flat `metadata.semantic_*count` fields remain compatibility mirrors during this migration wave.
`metadata.channels` also exposes comparable health and counters for `lexical_manifest`, `graph_precise`, and `path_surface_witness`, so witness recall and graph filtering are auditable without inferring them from per-match scores alone.
If `search_hybrid` returns a non-null `metadata.warning`, treat the ranking as lexical/graph-only or partially semantic and pivot to `search_symbol`, `find_references`, or scoped `search_text` for concrete anchors. Warnings can also appear when `metadata.semantic_status=ok` but semantic retrieval returned no hits or no returned top result kept semantic contribution.

## CLI Public Surface

```text
frigg [OPTIONS] [COMMAND]
```

Commands:
- `init`: initialize storage schema for each workspace root.
- `verify`: verify schema/read-write/vector readiness for each workspace root.
- `reindex [--changed]`: reindex files and persist snapshot/manifest updates.

Global options:
- `--workspace-root <PATH>` (repeatable)
- Serving mode may omit `--workspace-root`; stdio MCP clients should generally prefer omitting it so session attach/status remains available even before repo-local storage exists. Utility commands still require it explicitly.
- `--max-file-bytes <BYTES>` (default `2097152`; or env `FRIGG_MAX_FILE_BYTES`)
- `--mcp-http-port <PORT>`
- `--mcp-http-host <HOST>`
- `--allow-remote-http`
- `--mcp-http-auth-token <TOKEN>` (or env `FRIGG_MCP_HTTP_AUTH_TOKEN`)
- `--watch-mode <MODE>` (default `auto`; or env `FRIGG_WATCH_MODE`; `auto|on|off`)
- `--watch-debounce-ms <MILLISECONDS>` (default `750`; or env `FRIGG_WATCH_DEBOUNCE_MS`)
- `--watch-retry-ms <MILLISECONDS>` (default `5000`; or env `FRIGG_WATCH_RETRY_MS`)
- `--semantic-runtime-enabled <BOOL>` (or env `FRIGG_SEMANTIC_RUNTIME_ENABLED`)
- `--semantic-runtime-provider <PROVIDER>` (or env `FRIGG_SEMANTIC_RUNTIME_PROVIDER`; `openai|google`)
- `--semantic-runtime-model <MODEL>` (or env `FRIGG_SEMANTIC_RUNTIME_MODEL`; optional override of the provider default)
- `--semantic-runtime-strict-mode <BOOL>` (or env `FRIGG_SEMANTIC_RUNTIME_STRICT_MODE`)
- env `FRIGG_MCP_TOOL_SURFACE_PROFILE` (`core` is the stable default profile; `extended` adds `explore` plus advanced deep-search runtime tools)

## MCP Tool Surface (v1)

Stable default runtime tools (`core` profile):
<!-- tool-surface-profile:core:start -->
- `list_repositories`
- `workspace_attach`
- `workspace_current`
- `read_file`
- `search_text`
- `search_hybrid`
- `search_symbol`
- `find_references`
- `go_to_definition`
- `find_declarations`
- `find_implementations`
- `incoming_calls`
- `outgoing_calls`
- `document_symbols`
- `search_structural`
<!-- tool-surface-profile:core:end -->

Noise-control tip:
- `list_repositories` and `workspace_current` now surface nested repository `storage` plus split `health` (`lexical`, `semantic`, `scip`). `workspace_current` also returns additive `repositories` and `runtime` blocks for attached-repo state, runtime profile, active/recent tasks, and recent provenance, and its lexical/semantic health reasons reuse the same shared manifest/semantic freshness model the watcher uses. `workspace_attach` keeps `storage` at the top level and returns the same split `health` inside `repository`, so attach responses do not repeat the same storage block twice.
- `search_text` searches normal repository files broadly. When you only want docs/runtime evidence, add `path_regex`, for example `^(README\.md|crates/cli/src/.*)$`.
- `search_text` and `find_references` expose top-level `total_matches` so clients can distinguish the returned slice from the full match count.
- `search_hybrid` is the broad natural-language entrypoint for mixed doc/runtime questions. Expect contracts, README, runtime, and tests to coexist in top hits; when you need concrete runtime anchors, follow with `search_symbol` or scoped `search_text`. Live responses now publish canonical multi-channel diagnostics in `metadata.channels`, keyed by `lexical_manifest`, `graph_precise`, `semantic`, and `path_surface_witness`, while keeping the flat `metadata.semantic_*` keys and `warning` as compatibility mirrors. Legacy top-level semantic mirrors and JSON-string `note` remain optional compatibility fields in the schema but are omitted from normal live responses. Ranking stays anchor-first: Frigg blends anchor evidence, aggregates corroborating anchors by document while preserving the strongest returned anchor and excerpt, and then diversifies once across the aggregated documents.
- Search heuristics are intentionally codebase-generic. Frigg may use source classes, artifact families, anchor signals, workspace ignore state, and supported ecosystem cues, but it does not hardcode FRIGG-repo path boosts into production ranking.
- For implementation-oriented queries like `initialize`, `subscriptions`, `completion providers`, `handlers`, `transport`, or `resource updated`, `search_hybrid` now keeps bounded token recall active and prefers concrete runtime/support/test/example witnesses over repeated generic docs or `composer.json`. It still remains mixed-mode rather than runtime-only.
- For Rust daily-work queries that ask where an app starts, wires runtime state, or builds a pipeline/runner object, `search_hybrid` now gives extra weight to canonical entrypoints like `src/main.rs` and prefers build-anchor excerpts over fake/mock helper snippets.
- Mixed symbol-plus-intent queries such as `build_pipeline_runner entry point bootstrap` or `ProviderInterface completion providers` now expand identifier overlap terms so exact-anchor runtime families stay above unrelated semantic tail files more reliably.
- `search_symbol` now supports optional `path_class` (`runtime`, `project`, `support`) and `path_regex` filters to cut overloaded-name noise. Within the same lexical bucket, runtime code under `src/` outranks project/support paths.
- `search_symbol` also benefits from stronger PHP canonical-name and class-target evidence, so exact queries like `App\\Handlers\\OrderHandler` or `App\\Handlers\\OrderHandler::handle` can resolve deterministically without adding Laravel-specific parameters.
- `find_references` accepts either `symbol` or `path` + `line` (with optional `column`). If you supply both a symbol and a source location, Frigg resolves by location and records `metadata.resolution_source="location"` and the same payload in the legacy JSON-string `note`.
- Symbol-targeted navigation keeps deterministic exact-name resolution, but ambiguous exact-name queries now prefer runtime code under `src/` ahead of `benches/`, `examples/`, and `tests/`. Parsed `note.target_selection` metadata records the chosen path class.
- `find_implementations` keeps direct precise SCIP relationships first and then tries occurrence-backed precise recovery from enclosing `impl` definitions before falling back heuristically.
- `incoming_calls` now classifies occurrence-derived precise matches as `calls` when the recovered source line is call-like for a callable target; other precise occurrence matches remain `refers_to`. Both call-hierarchy tools expose optional `call_path`/`call_line`/`call_column`/`call_end_line`/`call_end_column` on match rows when a precise occurrence anchor is available.
- `outgoing_calls` is callable-only. Occurrence-derived precise recovery emits `relation="calls"` for surviving callable targets and does not widen the result set to locals, fields, constants, or type-only references.
- Navigation, call-hierarchy, document-symbol, and structural-search responses now expose typed `metadata` objects alongside the backward-compatible JSON-string `note`.
- `document_symbols` now returns hierarchical `children` instead of a flat list with empty containers only.
- `document_symbols` and `search_structural` now accept Rust, PHP, and Blade, plus baseline TypeScript / TSX, Python, Go, Kotlin / KTS, Lua, Roc, and Nim source files. Blade responses include additive `metadata.blade` summaries for normalized template relations, literal Livewire tags and `wire:*` directives, and Flux tag or hint discovery.
- Blade, Livewire, and Flux support is source-only and bounded. Frigg does not boot Laravel and does not claim route, provider, container, policy, validation, or Eloquent overlays in this slice.

Advanced runtime tools (only added when `FRIGG_MCP_TOOL_SURFACE_PROFILE=extended`):
<!-- tool-surface-profile:extended_only:start -->
- `explore`
- `deep_search_run`
- `deep_search_replay`
- `deep_search_compose_citations`
<!-- tool-surface-profile:extended_only:end -->

## Shell Vs Frigg

- Use shell tools for trivial local literal scans, one-off file reads, generic filesystem inspection, and normal git work.
- Use Frigg when repository-aware evidence, symbols, navigation, provenance, or attached multi-repo context matter.
- `explore` is an extended-profile follow-up tool for bounded single-artifact probe/zoom/refine work after discovery; it is not part of the stable default surface.

## Policy Resources And Prompts

- Resource: `frigg://policy/support-matrix.json`
- Resource: `frigg://policy/tool-surface.json`
- Resource: `frigg://guidance/shell-vs-frigg.md`
- Prompt: `frigg-routing-guide`
- These MCP surfaces publish the staged language policy, core-vs-extended tool boundary, and routing guidance without adding more runtime tools.

Schema files:
- `contracts/tools/v1/list_repositories.v1.schema.json`
- `contracts/tools/v1/workspace_attach.v1.schema.json`
- `contracts/tools/v1/workspace_current.v1.schema.json`
- `contracts/tools/v1/read_file.v1.schema.json`
- `contracts/tools/v1/search_text.v1.schema.json`
- `contracts/tools/v1/search_hybrid.v1.schema.json`
- `contracts/tools/v1/search_symbol.v1.schema.json`
- `contracts/tools/v1/find_references.v1.schema.json`
- `contracts/tools/v1/go_to_definition.v1.schema.json`
- `contracts/tools/v1/find_declarations.v1.schema.json`
- `contracts/tools/v1/find_implementations.v1.schema.json`
- `contracts/tools/v1/incoming_calls.v1.schema.json`
- `contracts/tools/v1/outgoing_calls.v1.schema.json`
- `contracts/tools/v1/document_symbols.v1.schema.json`
- `contracts/tools/v1/search_structural.v1.schema.json`
- `contracts/tools/v1/explore.v1.schema.json`
- `contracts/tools/v1/deep_search_run.v1.schema.json`
- `contracts/tools/v1/deep_search_replay.v1.schema.json`
- `contracts/tools/v1/deep_search_compose_citations.v1.schema.json`

Contract notes:
- these are the canonical `v1` public tools,
- `core` is the stable default read-only profile; `extended` is an advanced-consumer profile that layers `explore` plus deep-search runtime tools on top of it,
- paths in responses are canonical repository-relative paths,
- breaking schema changes require a new major version directory.

## Public Contracts

- Tool schemas/versioning: `contracts/tools/v1/README.md`
- Config contract: `contracts/config.md`
- Error taxonomy: `contracts/errors.md`
- Storage contract: `contracts/storage.md`
- Semantic embeddings contract: `contracts/semantic.md`
- Contract changelog: `contracts/changelog.md`

## Tooling With Just

A root `Justfile` provides the common workflow commands:

- quality: `just fmt`, `just clippy`, `just test`, `just quality`
- app lifecycle: `just build`, `just run`, `just init <root>`, `just verify <root>`, `just reindex <root>`, `just reindex-changed <root>`
- ops gates: `just smoke-ops`, `just release-ready`, `just docs-sync`
- focused tests: `just test-security`, `just test-mcp-tool-handlers`, `just test-mcp-provenance`
- benchmarks/reporting: `just bench-core-latency`, `just bench-report`, `just bench-report-gate`

## Local Pre-commit

This repo ships a native `prek.toml` for fast local commit gates.

```bash
prek validate-config
prek run --all-files
prek install
```

The hooks intentionally stay lightweight: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`.

## Security And Release Gates

- Threat baseline: `docs/security/threat-model.md`
- Release checklist: `docs/security/release-readiness.md`
- Operational smoke: `scripts/smoke-ops.sh`
- Release gate: `scripts/check-release-readiness.sh`

Run the release gate:
```bash
just release-ready
# or: bash scripts/check-release-readiness.sh
```

## Performance Budgets

Benchmark docs and targets live in `benchmarks/`.

Generate report:
```bash
just bench-report
```

Generate strict gate report (fails on budget miss):
```bash
just bench-report-gate
```

## Workspace Structure

- Root `Cargo.toml` is a virtual workspace.
- Internal crates provide domain/search/index/storage/graph/MCP logic.
- The deployable binary package is `frigg` (`crates/cli`).
- The project intentionally stays as one binary/package today. A future split into `frigg-core` and `frigg-app` is only justified once engine reuse, release cadence, compile/test pressure, or ownership boundaries clearly require it; see [`docs/architecture.md`](./docs/architecture.md).

You ship one artifact (`frigg`), not one artifact per workspace crate.
