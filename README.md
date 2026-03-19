# frigg

Frigg is a local-first, read-only MCP server for code understanding. It scans a repository, stores a synchronized index in a local SQLite database, and gives AI agents fast, source-backed search and navigation across Rust, PHP, Blade, TypeScript / TSX, Python, Go, Kotlin / KTS, Lua, Roc, and Nim.

It is built for the moment when an agent needs more than `rg`: definitions, references, implementations, callers, structural queries, document outlines, and better answers to “which files matter here?”. Under the hood Frigg combines deterministic file manifests, Tree-sitter parsing, optional SCIP overlays for more precise navigation, and optional semantic retrieval. It is not a replacement for shell tools or your IDE. It is a context engine that brings more IDE-like code intelligence into MCP.

## What To Use Frigg For

- finding where a symbol is defined or declared
- tracing who calls a function and which types implement an interface
- asking natural-language questions about a codebase without losing source-backed anchors
- keeping a local code index warm for an MCP client instead of re-reading many files on every step
- working across one or more local repositories without requiring a remote indexing service by default

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

## Use Cases

### Standard code search and navigation

This is the default Frigg workflow:

1. Run `frigg init` once from the repository root.
2. Optionally run `frigg reindex` to prewarm the index before first use.
3. Start one persistent Frigg HTTP service with `frigg serve`.
4. Let your agent adopt repositories session-locally with `workspace_attach`.
5. Use `workspace_prepare` or `workspace_reindex` from MCP only when you intentionally want Frigg to initialize or refresh repository state from inside the client.
6. Use `search_hybrid` for broad questions, then narrow with symbol or navigation tools when you need precise anchors.

Typical prompts:

- “Where is authentication bootstrapped?”
- “Show me implementations of `ProviderInterface`.”
- “Who calls `handleWebhook`?”
- “Which files are relevant to the checkout flow?”

### Optional semantic search

Semantic retrieval is off by default. When enabled, it improves recall for natural-language queries, but Frigg still grounds answers in local lexical and graph evidence.

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

Frigg can consume external SCIP artifacts, but it does not generate them itself. If you want more precise definitions, references, implementations, and call navigation, place generated `.scip` or `.json` files under:

```text
.frigg/scip/
```

Example:

```bash
mkdir -p .frigg/scip
```

Good starting points for generating SCIP artifacts:

- Overview of supported indexers: [Sourcegraph indexers](https://sourcegraph.com/docs/code-search/code-navigation/references/indexers)
- Rust: [rust-analyzer](https://github.com/rust-lang/rust-analyzer)
- PHP: [scip-php](https://github.com/davidrjenni/scip-php)
- TypeScript / JavaScript: [scip-typescript](https://github.com/sourcegraph/scip-typescript)
- Python: [scip-python](https://github.com/sourcegraph/scip-python)

Typical examples:

Rust:

```bash
mkdir -p .frigg/scip
rust-analyzer scip . > .frigg/scip/rust.scip
```

PHP:

```bash
mkdir -p .frigg/scip
composer require --dev davidrjenni/scip-php
vendor/bin/scip-php
mv index.scip .frigg/scip/php.scip
```

Frigg distills those artifacts into snapshot-scoped retrieval projections on the next `frigg reindex`. Server startup alone does not change retrieval state. If you do not provide SCIP data, Frigg still works with heuristic and source-backed navigation plus path and AST-derived retrieval summaries.

### Built-in watch worker

Frigg includes a built-in watch worker that keeps the index fresh with changed-only refreshes.

- `frigg serve` defaults to loopback HTTP with `--watch-mode auto` on `127.0.0.1:37444`
- the service can start empty or with explicit startup roots
- startup roots become globally known repositories
- watchers activate only while active sessions hold watcher leases for adopted repositories
- `workspace_attach` accepts `path` or `repository_id` and adopts repositories session-locally
- attaching a repository by path can register it dynamically after the service has already started
- `workspace_detach` removes session adoption and may release the watcher lease
- the worker refreshes the manifest first and only runs a semantic follow-up when needed
- the watch worker updates the same `.frigg/storage.sqlite3` database instead of creating a separate sidecar index
- if you already run an external watcher, start Frigg with `--watch-mode off` to avoid duplicate work

Runtime cache contract:

- cross-request reuse is keyed by repository freshness, not by wall-clock age alone
- watcher, attach, detach, manifest validation, and reindex transitions can invalidate only the affected repository cache scopes without restarting the server
- snapshot-scoped projection state is the preferred reusable tier; request-local graph fallbacks stay request-bound
- response caches are bounded and opportunistic, so multiple `stdio` servers may miss independently without affecting correctness

Example:

```bash
frigg serve
```

## Frigg Vs Shell Search

Use shell tools like `rg` for quick literal scans, file listings, and normal repository work.

Use Frigg when the question is repository-aware:

- definitions
- references
- implementations
- call relationships
- structural queries
- natural-language discovery across many files
- source-backed answers that need fewer manual file hops

Frigg works best when your agent is told to prefer Frigg for repo-aware search and navigation, and plain shell tools for trivial literal tasks.

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

## MCP Tools

### Core tools

- `list_repositories`: list globally known repositories in the runtime catalog.
- `workspace_attach`: adopt a repository into the current session by `path` or `repository_id`.
- `workspace_detach`: remove a repository adoption from the current session and potentially release a watch lease.
- `workspace_prepare`: confirm-gated workspace/index preparation for an adopted repository.
- `workspace_reindex`: confirm-gated full or changed reindex for an adopted repository.
- `workspace_current`: inspect session-local repository adoption, defaults, health, and runtime status.
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
- `search_structural`: run Tree-sitter structural queries over supported languages.

### Extended profile tools

Set `FRIGG_MCP_TOOL_SURFACE_PROFILE=extended` to expose these additional tools:

- `explore`: bounded follow-up exploration for a single artifact after discovery.
- `deep_search_run`: run a deeper multi-step search workflow.
- `deep_search_replay`: replay a prior deep-search trace.
- `deep_search_compose_citations`: build citation payloads from deep-search output.

## Supported Languages

Frigg currently supports:

- Rust
- PHP
- Blade
- TypeScript / TSX
- Python
- Go
- Kotlin / KTS
- Lua
- Roc
- Nim

These languages participate in text search, symbol search, structural search, document outlines, and hybrid retrieval. Blade support is source-based and bounded. Frigg does not boot Laravel or emulate a full framework runtime.

## Safety And Boundaries

- Frigg does not modify source files. Workspace/index maintenance tools (`workspace_prepare`, `workspace_reindex`) are confirm-gated and operate on Frigg state.
- Frigg only reads inside configured workspace roots.
- Frigg keeps its primary state locally in SQLite.
- Optional semantic search may call an external embedding provider if you enable it.
- External SCIP artifacts improve precision when available, but they are optional.

Session adoption and watcher leases are runtime/session state. `workspace_current.repositories` is session-local, while `list_repositories` is the global known-repository catalog. For repo-aware tools with omitted `repository_id`, Frigg scopes to the session default first, then the remaining adopted repositories.

Frigg has been tested against larger real-world repositories across its supported language set, but the product boundary stays intentionally narrow: local code evidence over MCP, not a full IDE or framework runtime.
