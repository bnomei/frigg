# frigg

Frigg is a local-first, read-only MCP server built in Rust for code understanding. It scans local repositories, stores synchronized indexes in local SQLite, and gives AI agents fast, source-backed search and navigation across Rust, PHP, Blade, TypeScript / TSX, Python, Go, Kotlin / KTS, Lua, Roc, and Nim, even when the relevant answer lives in another adopted repository.
All supported languages participate in text search, symbol search, structural search, document outlines, and hybrid retrieval. Blade support is source-based and bounded.

It is built for the moment when an agent needs more than `rg/fd/ast-grep`: definitions, references, implementations, callers, structural queries, document outlines, and better answers to “which files matter here?”. **Under the hood Frigg combines deterministic file manifests, Tree-sitter AST parsing, optional SCIP overlays for more precise navigation, and optional semantic retrieval.** It is not a replacement for shell tools or your IDE. It is a context engine that brings more IDE-like code intelligence into MCP.

## What To Use Frigg For

Use Frigg when the question is repository-aware and you want source-backed navigation instead of another raw repo scan.

- jumping from a broad question to real code quickly with source-backed discovery, outlines, definitions, references, implementations, and call relationships
- asking natural-language questions without giving up concrete anchors, matched paths, and navigable files
- keeping one fast local index warm across one or more adopted repositories, so agents can move across shared code, related services, or neighboring projects without rebuilding context from scratch
- getting a more IDE-like flow for agents in the terminal: discover the area, open the file, inspect symbols, and continue navigating from there

## Installation

### Cargo

Published crate:

```bash
cargo install frigg
```

Local checkout:

```bash
cargo install --path crates/cli
```

### Homebrew

```bash
brew install bnomei/frigg/frigg
```

### GitHub Releases

Download a prebuilt archive or source package from GitHub Releases, extract it, and place `frigg` on your `PATH`.

### From source

```bash
git clone https://github.com/bnomei/frigg.git
cd frigg
cargo build --release -p frigg
```

## Quickstart

### 1) Prepare a repository

```bash
cd /absolute/path/to/repo
frigg init
frigg verify
```

Optional prewarm:

```bash
frigg reindex
```

When you run these commands inside the repository root, Frigg now uses the current directory as the default workspace root. If you run them from somewhere else, pass `--workspace-root` explicitly.

### 2) Start the recommended Frigg service

```bash
frigg serve
```

Keep that process running in its own terminal tab or background session. This is the Frigg service your MCP client connects to. `frigg serve` can start with zero startup roots, so you can keep one shared Frigg service running and let clients adopt repositories as needed. The usual flow is:

1. run `frigg init` / `frigg verify` inside each repository you care about
2. keep one `frigg serve` process running
3. point your MCP client at that running Frigg service

If you already know which repositories you want globally known at startup, you can still pass them explicitly:

```bash
frigg serve \
  --workspace-root /absolute/path/to/repo-a \
  --workspace-root /absolute/path/to/repo-b
```

`frigg serve` defaults to loopback HTTP on `127.0.0.1:37444`. Startup roots become globally known repositories immediately, but watch leases are session-driven and start only after a session adopts a repository. The MCP endpoint is:

`http://127.0.0.1:37444/mcp`

### 3) Add Frigg to your MCP client

Point your MCP client at the loopback HTTP endpoint of the running Frigg service:

`http://127.0.0.1:37444/mcp`

Example MCP client config for an HTTP / streamable MCP connection:

```json
{
  "mcpServers": {
    "frigg": {
      "transport": "streamable_http",
      "url": "http://127.0.0.1:37444/mcp"
    }
  }
}
```

The exact file name and field names vary by client, but the important part is that the client connects to the running Frigg service at that URL. In other words: this setup assumes `frigg serve` is already running in another terminal or background process. You are connecting to Frigg here, not asking the MCP client to spawn it.

## How Frigg Uses Your Workspace

For each indexed repository, Frigg creates and maintains:

- `.frigg/storage.sqlite3`: the local SQLite database for manifests, snapshot-scoped retrieval projections, search state, navigation data, semantic data, and provenance

Frigg can also read:

- your source files under the configured workspace roots
- optional `.frigg/scip/*.scip` or `.frigg/scip/*.json` artifacts for more precise definitions, references, implementations, and call navigation

Frigg does not modify your source tree during plain session adoption. `workspace_attach` by itself does not create `.frigg` state. Frigg writes `.frigg/storage.sqlite3` only when indexing/preparing/reindexing paths run.

## Showcases

The [showcases/](/Users/bnomei/Sites/frigg/showcases/) directory contains 52 public example catalogs for real repositories. Each JSON file records realistic questions and the kinds of paths a good Frigg answer should surface.

## Use Cases

### Standard code search and navigation

Once Frigg is running, the normal workflow is:

1. Let your agent adopt repositories session-locally with `workspace_attach`.
   `workspace_attach` reports whether the session attached a fresh workspace or reused an already-adopted one, and it returns a compact precise-index summary for the selected repo. That summary now exposes `state`, `failure_tool`, `failure_class`, `failure_summary`, `recommended_action`, and `generation_action` so clients do not need to parse nested generator detail first.
2. Use `search_hybrid` as the discovery surface for broad questions, then pivot into `read_file`, `document_symbols`, `go_to_definition`, or `search_symbol` when you need precise anchors and deeper navigation.
3. Use `workspace_prepare` or `workspace_reindex` only when you intentionally want to initialize or refresh repository state from inside the client.
4. Use `inspect_syntax_tree` before `search_structural` whenever the tree-sitter node shape is unclear.

`inspect_syntax_tree` and `search_structural` accept `include_follow_up_structural=true` as an opt-in. When enabled, Frigg returns typed `follow_up_structural` suggestions that are replayable `search_structural` invocations derived from the resolved AST focus, not from the user's original query. Omitting the flag keeps the normal response shape unchanged. Phase 1 covers `inspect_syntax_tree` and `search_structural`; phase 2 extends the same contract to `document_symbols`, `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls`. The phase 2 surfaces require stable `path`, `line`, and `column` anchors, and they omit suggestions when no usable AST focus can be resolved. `search_hybrid` and `search_symbol` remain deferred.

`search_structural` now defaults to one row per Tree-sitter match instead of one row per capture. Use `primary_capture` when your query has helper captures but you want one specific capture to anchor the visible row, or switch to `result_mode=captures` when you want raw capture rows for debugging.

Typical prompts:

- “Where is authentication bootstrapped?”
- “Show me implementations of `ProviderInterface`.”
- “Who calls `handleWebhook`?”
- “Which files are relevant to the checkout flow?”

### Optional semantic search

Semantic retrieval is off by default. When enabled, it improves recall for natural-language queries, but Frigg still grounds answers in local lexical and graph evidence.
Once enabled, **semantic refresh participates in reindex and watch-driven updates**, so Frigg may call the configured embedding provider automatically as the workspace changes. That means semantic mode can consume provider tokens over time, not only when you run a manual reindex.

OpenAI:

```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=openai
export OPENAI_API_KEY=...
```

Google:

```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=google
export GEMINI_API_KEY=...
```

Optional model override:

```bash
export FRIGG_SEMANTIC_RUNTIME_MODEL=text-embedding-3-small
```

After enabling semantic search for an existing repository, run one reindex pass:

```bash
frigg reindex
```

### Optional SCIP artifacts

Frigg can consume external SCIP artifacts, and if supported generator tools are installed it will **automatically detect and invoke them during workspace attach/reindex flows** for Rust, Go, TypeScript / JavaScript, Python, PHP, and Kotlin. Kotlin auto-generation is intentionally scoped to Gradle/KTS workspaces with Kotlin source files; other Kotlin/JVM layouts should continue to use manual `.frigg/scip/` artifact drops.

The commands below are only needed if you want to pre-populate artifacts yourself, or if your workspace falls outside Frigg's automatic generation path. Manual artifacts should be placed under:

```text
.frigg/scip/
```

Manual artifact directory:

```bash
mkdir -p .frigg/scip
```

If you want to generate SCIP artifacts yourself, these are good starting points:

- Overview of supported indexers: [Sourcegraph indexers](https://sourcegraph.com/docs/code-search/code-navigation/references/indexers)
- Rust: [rust-analyzer](https://github.com/rust-lang/rust-analyzer)
- PHP: [scip-php](https://github.com/davidrjenni/scip-php)
- Laravel: [scip-laravel](https://github.com/bnomei/scip-laravel)
- TypeScript / JavaScript: [scip-typescript](https://github.com/sourcegraph/scip-typescript)
- Python: [scip-python](https://github.com/sourcegraph/scip-python)
- Kotlin / Gradle: [scip-java](https://sourcegraph.github.io/scip-java/docs/getting-started.html)

Laravel PHP workspaces prefer repo-local `vendor/bin/scip-laravel` when `bootstrap/app.php` is present; otherwise Frigg keeps using the existing PHP `vendor/bin/scip-php` / `scip-php` lookup.

Frigg distills those artifacts into snapshot-scoped retrieval projections on the next `frigg reindex`. Server startup alone does not change retrieval state. If you do not provide SCIP data, Frigg still works with heuristic and source-backed navigation plus path and AST-derived retrieval summaries.

When generator tools are installed, `workspace_current.health.precise_generators` reports their detected status and any last generation result, and Frigg writes best-effort artifacts under `.frigg/scip/`. Python uses `scip-python index . --project-name <derived-name>` with a deterministic derived name, and Kotlin uses `scip-java index` only on Gradle/KTS workspaces that also contain Kotlin sources.

Optional repository-local precise config lives at `.frigg/precise.json`. Use it to disable a generator for one repo, add generator-specific extra args, or exclude paths from filtered generation workspaces and trigger calculations without compiling repo-specific path rules into FRIGG itself.

## Built-In Watch Mode

Frigg includes a built-in watch worker behind `frigg serve` that keeps indexed repositories fresh with changed-only refreshes.

- watchers activate only while active sessions hold watcher leases for adopted repositories
- refreshes update the same `.frigg/storage.sqlite3` state, not a separate sidecar index
- repository-scoped caches are invalidated only for the repo that changed, so follow-up reads and searches stay warm elsewhere
- if you already run an external watcher, start Frigg with `--watch-mode off` to avoid duplicate work

## Frigg Vs Shell Search

Use shell tools like [`rg`](https://github.com/BurntSushi/ripgrep) for fast literal and regex scans, [`fd`](https://github.com/sharkdp/fd) for quick file and path discovery, and [`ast-grep`](https://github.com/ast-grep/ast-grep) for standalone structural matching in normal repository work.

On macOS and Linux, if `rg` is installed, Frigg can also use it internally as an optional lexical accelerator for `search_text` and the lexical stage of `search_hybrid`. That stays inside Frigg's own candidate scope and falls back to the native scanner automatically when `rg` is missing, disabled, or fails.

Use Frigg when the question is repository-aware:

- definitions
- references
- implementations
- call relationships
- structural queries
- natural-language discovery across many files
- source-backed answers that need fewer manual file hops

Frigg works best when your agent is told to prefer Frigg for repo-aware search and navigation, and plain shell tools for quick text, path, or one-off structural tasks.

## MCP Tools

### Core tools

- `list_repositories`: list globally known repositories in the runtime catalog.
- `workspace_attach`: adopt a repository into the current session by `path` or `repository_id`.
- `workspace_detach`: remove a repository adoption from the current session and potentially release a watch lease.
- `workspace_prepare`: confirm-gated workspace/index preparation for an adopted repository.
- `workspace_reindex`: confirm-gated full or changed reindex for an adopted repository.
- `workspace_current`: inspect session-local repository adoption, defaults, compact precise status, health, and runtime status.
- `read_file`: read a file safely inside an adopted repository.
- `search_text`: run literal or regex text search across repository files.
- `search_hybrid`: broad natural-language search that blends lexical, graph, witness, and optional semantic evidence.
- `search_symbol`: search for symbols such as functions, classes, methods, traits, or modules.
- `find_references`: find references to a symbol or a source location.
- `go_to_definition`: jump to a symbol definition from a symbol or source location.
- `find_declarations`: find declaration sites for a symbol or source location.
- `find_implementations`: find implementing types or members for interfaces, traits, or base symbols.
- `incoming_calls`: find callers of a callable symbol.
- `outgoing_calls`: find callees from a callable symbol.
- `document_symbols`: return a hierarchical outline for a source file.
- `inspect_syntax_tree`: inspect the bounded AST stack around a source location before writing a structural query, with optional `include_follow_up_structural=true` for best-effort replayable follow-up queries.
- `search_structural`: run Tree-sitter structural queries over supported languages, grouped one row per match by default, with optional raw `result_mode=captures`, `primary_capture` anchoring, and per-match best-effort follow-up queries via `include_follow_up_structural=true`.
- Follow-up structural suggestions are opt-in across the phase 1 and phase 2 surfaces above. Phase 1 covers `inspect_syntax_tree` and `search_structural`; phase 2 covers `document_symbols`, `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls`. `search_hybrid` and `search_symbol` remain deferred.

### Extended profile tools

Set `FRIGG_MCP_TOOL_SURFACE_PROFILE=extended` to expose these additional tools:

- `explore`: bounded follow-up exploration for a single artifact after discovery.
- `deep_search_run`: run a deeper multi-step search workflow.
- `deep_search_replay`: replay a prior deep-search trace.
- `deep_search_compose_citations`: build citation payloads from deep-search output.

## Under the Hood

Frigg keeps a **local repository model** for each adopted workspace instead of rescanning from scratch on every question. That model starts with deterministic manifests and SQLite-backed snapshot state, then layers in Tree-sitter AST parsing, symbol extraction, retrieval projections, and optional overlays such as SCIP and semantic embeddings when you enable them.

For broad discovery, Frigg does not just sort raw text hits. `search_hybrid` turns the query into intent, collects evidence from lexical matches, path and surface witnesses, graph facts, and optional semantic recall, then runs a rule-driven reranker and post-selection pass that tries to keep the useful files visible: runtime code, entrypoints, config, tests, build surfaces, and nearby companions. That is the internal “DSL and reranker” story in plain language: query facts come in, scoring rules fire, and the final pass repairs or preserves good pivots so obvious source files are less likely to lose to generic noise.

The built-in watch runtime keeps that model fresh incrementally. Frigg tracks changed paths, refreshes the affected repository state, and invalidates only the repository-scoped caches that need to move.

## Configuration

Precedence is `CLI flag > env var > default`.

| Flag / Env | Default | Meaning |
| --- | --- | --- |
| `--workspace-root` | utility commands default to current directory; serving mode can start empty | Limits what Frigg can read and index. Repeatable. In serving mode these roots become the global known-repository catalog. |
| `--max-file-bytes` / `FRIGG_MAX_FILE_BYTES` | `2097152` | Maximum file size Frigg will read. |
| `--watch-mode` / `FRIGG_WATCH_MODE` | stdio `off`, HTTP `auto` | Controls the built-in watch worker: `auto`, `on`, or `off`. |
| `--watch-debounce-ms` / `FRIGG_WATCH_DEBOUNCE_MS` | `750` | Debounce delay before a watch-triggered refresh starts. |
| `--watch-retry-ms` / `FRIGG_WATCH_RETRY_MS` | `5000` | Retry delay after a failed watch refresh. |
| `--mcp-http-port` | unset | Enables HTTP transport on the given port. |
| `--mcp-http-host` | unset | Host bind address for HTTP transport. |
| `--allow-remote-http` | `false` | Required for non-loopback HTTP serving. |
| `--mcp-http-auth-token` / `FRIGG_MCP_HTTP_AUTH_TOKEN` | unset | Auth token for HTTP mode. Required for non-loopback HTTP. |
| `FRIGG_SEMANTIC_RUNTIME_ENABLED` | `false` | Enables optional semantic retrieval. |
| `FRIGG_SEMANTIC_RUNTIME_PROVIDER` | unset | Semantic provider: `openai` or `google`. |
| `FRIGG_SEMANTIC_RUNTIME_MODEL` | provider default | Optional embedding model override. |
| `FRIGG_SEMANTIC_RUNTIME_STRICT_MODE` | `false` | Tightens query-time semantic failure behavior. |

Provider defaults:

- `openai` -> `text-embedding-3-small`
- `google` -> `gemini-embedding-001`

## Safety And Boundaries

- Frigg does not modify source files. Workspace/index maintenance tools (`workspace_prepare`, `workspace_reindex`) are confirm-gated and operate on Frigg state.
- Frigg only reads inside configured workspace roots.
- Frigg keeps its primary state locally in SQLite.
- Optional semantic search may call an external embedding provider if you enable it.
- External SCIP artifacts improve precision when available, but they are optional.

Session adoption and watcher leases are runtime/session state. `workspace_current.repositories` is session-local, while `list_repositories` is the global known-repository catalog. For repo-aware tools with omitted `repository_id`, Frigg scopes to the session default first, then the remaining adopted repositories.

Frigg has been tested against larger real-world repositories across its supported language set, but the product boundary stays intentionally narrow: local code evidence over MCP, not a full IDE or framework runtime.
