# frigg

[![Crates.io Version](https://img.shields.io/crates/v/frigg)](https://crates.io/crates/frigg)
[![Build Status](https://github.com/bnomei/frigg/actions/workflows/ci.yml/badge.svg)](https://github.com/bnomei/frigg/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20AND%20MPL--2.0-blue)](#license)
[![Discord](https://flat.badgen.net/badge/discord/bnomei?color=7289da&icon=discord&label)](https://discordapp.com/users/bnomei)
[![Buymecoffee](https://flat.badgen.net/badge/icon/donate?icon=buymeacoffee&color=FF813F&label)](https://www.buymeacoffee.com/bnomei)

Frigg is a local-first MCP server for code understanding. It is the default agent surface for code discovery, navigation, exact code search, and bounded source reads across Rust, PHP, Blade, TypeScript / TSX, Python, Go, Kotlin / KTS, Java, Lua, Roc, and Nim.

It builds a synchronized SQLite repository model for broad discovery, exact source windows, document outlines, symbols, definitions, references, implementations, callers, structural queries, optional semantic recall, and optional SCIP-backed precision.

Frigg is source read-only during normal indexing. It stores its own state under `.frigg/`, but it does not edit project source files as part of ordinary search and navigation.

## What You Get

- one local MCP service that can serve multiple adopted repositories
- local SQLite state for manifests, search projections, semantic rows, and navigation data
- Tree-sitter-backed symbol, document outline, and structural search for all supported languages
- hybrid discovery that blends lexical matches, path and surface witnesses, graph evidence, optional semantic recall, and code-aware reranking
- bounded `read_file` and `read_match` output so agents inspect smaller source slices instead of repeatedly reading whole files
- optional SCIP artifact ingestion and best-effort generation for more precise definitions, references, implementations, and call navigation
- built-in watch mode behind `frigg serve` for changed-only refreshes while sessions are active

## Quickstart

### 1. Install Frigg

With the Unix installer on macOS or GNU/glibc Linux:

```bash
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
  https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh \
  | FRIGG_VERSION=0.5.0 sh
```

`FRIGG_VERSION` accepts `0.5.0` or `v0.5.0`. When it is unset, the installer resolves the latest GitHub Release. The installer downloads the matching `frigg-v<VERSION>-<TARGET>.tar.gz` archive, verifies its `.sha256`, and installs only the `frigg` binary.

For manual release-asset downloads, Unix archives use the same `frigg-v<VERSION>-<TARGET>.tar.gz` naming pattern.

With Homebrew:

```bash
brew install bnomei/frigg/frigg
```

With Cargo as a Rust/developer fallback:

```bash
cargo install frigg
```

From a local checkout:

```bash
git clone https://github.com/bnomei/frigg.git
cd frigg
cargo build --release -p frigg
```

### 2. Prepare a repository

Run these commands inside the repository you want Frigg to index:

```bash
frigg init
frigg verify
frigg reindex
frigg serve
```

When these commands run inside a repository root, Frigg uses the current directory as the workspace root. From another directory, pass `--workspace-root /absolute/path/to/repo`.

### 3. Start the shared MCP service

If you did not start it during setup, run:

```bash
frigg serve
```

`frigg serve` listens on loopback HTTP by default:

```text
http://127.0.0.1:37444/mcp
```

Keep the process running in a terminal tab or background session. The service can start with zero startup roots, so clients can adopt repositories as needed.

If you want repositories globally known at startup, pass them explicitly:

```bash
frigg serve \
  --workspace-root /absolute/path/to/repo-a \
  --workspace-root /absolute/path/to/repo-b
```

### 4. Connect an MCP client

Claude Code:

```bash
claude mcp add --transport http frigg http://127.0.0.1:37444/mcp
```

Codex:

```bash
codex mcp add frigg --url http://127.0.0.1:37444/mcp
```

OpenCode:

```bash
opencode mcp add
```

Then choose a remote MCP server and enter:

- name: `frigg`
- url: `http://127.0.0.1:37444/mcp`

Other JSON-configured clients:

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

The exact config file and field names vary by client. The important part is that the client connects to the running Frigg service. The MCP client is not expected to spawn `frigg serve`.

### 5. Use it from the client

The normal first-use loop is:

1. Call `workspace_attach` for the repository.
2. Check `workspace_current` when you need freshness, precise-generator, or runtime status.
3. Start broad questions with `search_hybrid`.
4. Open source with `read_match` when a search result returned `result_handle` and `match_id`.
5. Pivot to `search_symbol`, `document_symbols`, `go_to_definition`, `find_references`, `find_implementations`, `incoming_calls`, `outgoing_calls`, or `search_structural` when you need exact navigation.

Example prompts:

- "Where is authentication bootstrapped?"
- "Show me implementations of `ProviderInterface`."
- "Who calls `handleWebhook`?"
- "Which files are relevant to the checkout flow?"

## GitHub Actions

Use a pinned Frigg release in CI. The default path avoids caching Frigg state and refreshes repository state every run:

```yaml
name: Frigg

on:
  pull_request:
  push:
    branches: [main]

env:
  FRIGG_VERSION: 0.5.0

jobs:
  frigg:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5

      - name: Install Frigg
        run: |
          curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
            https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh \
            | FRIGG_VERSION="${FRIGG_VERSION}" sh

      - run: frigg init
      - run: frigg reindex
      - run: frigg verify
```

For larger repositories, optionally cache `.frigg/`. Treat `.frigg/` as regenerable build output, not as a secret store. Always restore first, then refresh and verify the restored state. Save the cache only from trusted events:

```yaml
name: Frigg

on:
  pull_request:
  push:
    branches: [main]

env:
  FRIGG_VERSION: 0.5.0

jobs:
  frigg:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5

      - name: Install Frigg
        run: |
          curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
            https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh \
            | FRIGG_VERSION="${FRIGG_VERSION}" sh

      - name: Compute Frigg cache hash
        id: frigg-hash
        run: frigg hash

      - name: Restore Frigg state
        uses: actions/cache/restore@v4
        with:
          path: .frigg/
          key: frigg-${{ runner.os }}-${{ runner.arch }}-${{ env.FRIGG_VERSION }}-${{ steps.frigg-hash.outputs.frigg-hash }}-${{ github.sha }}
          restore-keys: |
            frigg-${{ runner.os }}-${{ runner.arch }}-${{ env.FRIGG_VERSION }}-${{ steps.frigg-hash.outputs.frigg-hash }}-

      - run: frigg init
      - run: frigg reindex --changed
      - run: frigg verify

      - name: Save Frigg state
        if: github.event_name == 'push' && github.ref == 'refs/heads/main'
        uses: actions/cache/save@v4
        with:
          path: .frigg/
          key: frigg-${{ runner.os }}-${{ runner.arch }}-${{ env.FRIGG_VERSION }}-${{ steps.frigg-hash.outputs.frigg-hash }}-${{ github.sha }}
```

As an advanced optimization, you can cache the installed `frigg` binary separately. Use an exact cache key built only from the Frigg version and runner platform, and do not use broad restore keys for cached executables.

## Why This Exists

AI coding cost and quality are often context problems before they are model problems. Tools can spend much of a request budget sending files, broad search results, and repeated repository context before the model writes a single line back. Projects such as [Code Context Engine](https://github.com/elara-labs/code-context-engine) make that pattern concrete: index code locally, let the agent search the index, and send only the relevant code slices instead of repeatedly feeding whole files.

Frigg follows the same local-index principle, but aims it at source-backed navigation rather than token accounting. It gives agents concrete anchors to cite, inspect, and follow: matched source windows, symbols, outlines, definitions, references, implementations, callers, structural matches, and repository health.

This pattern also shows up in production coding tools. [Turbopuffer](https://turbopuffer.com), [Cursor](https://cursor.com), and [Sourcegraph](https://sourcegraph.com) all point at the same broad lesson: agents do better when grep, indexes, semantic search, symbols, and precise code intelligence work together. Frigg brings a compact version of that shape to a local MCP server: smaller than Sourcegraph, less IDE-coupled than Cursor, and designed for terminal-based assistants that need source-backed code evidence.

The Frigg-specific layer is `search_hybrid`: a local reranking flow that gathers broad candidates, blends lexical, path, graph, witness, and optional semantic evidence, then applies code-aware selection rules for runtime files, entrypoints, configs, tests, build surfaces, navigation companions, and framework-specific witnesses.

## How Frigg Uses Your Workspace

For each indexed repository, Frigg creates and maintains:

- `.frigg/storage.sqlite3`: local SQLite state for manifests, snapshot-scoped retrieval projections, search state, navigation data, and semantic data

Frigg can also read:

- source files under configured workspace roots
- optional `.frigg/scip/*.scip` or `.frigg/scip/*.json` artifacts for precise navigation
- optional `.frigg/precise.json` generator configuration

By default, `workspace_attach` uses `index_mode=ensure`: it adopts the repository, refreshes stale or missing lexical and semantic state when possible, waits up to 30 seconds, and reports `index_lifecycle`. Use `index_mode=skip` only when stale or missing indexed state is acceptable.

Attach is not side-effect free. It can create or update `.frigg/storage.sqlite3`, update session state, report repository health, and schedule precise-generator discovery or generation when a generator applies. Optional precise generation may write `.frigg/scip/` artifacts, execute repo-local generator tools, or apply generator-specific compatibility patches.

For runtime diagnosis, see the [Frigg Operator Runbook](docs/operator-runbook.md).

## Agent Workflow

Frigg is the default for code discovery, navigation, exact code search, and bounded source reads.

Start with Frigg MCP tools when you need to find code, inspect symbols, follow relationships, search exact source text, or read bounded source windows from an attached repository.

Use shell tools for non-code files, git and filesystem inspection, and trivial one-off checks where a direct command is faster and does not replace code discovery or bounded source reads.

Use Frigg for repository-aware workflows:

- broad natural-language discovery across many files
- canonical repository-relative paths and bounded source reads
- definitions, declarations, references, implementations, callers, and callees
- document outlines and Tree-sitter structural queries
- source-backed answers that need fewer manual file hops
- multi-repository context from one shared service

On macOS and Linux, Frigg can use `rg` internally as an optional lexical accelerator for `search_text` and the lexical stage of `search_hybrid`. It stays inside Frigg's candidate scope and falls back to the native scanner when `rg` is missing, disabled, or fails.

## Bundled Skill

Frigg ships a search-and-navigation skill in [skills/frigg-mcp-search-navigation](skills/frigg-mcp-search-navigation/).

Use it as the repo-backed instruction bundle for assistants that support local or Git-backed skills. It explains:

- when to use Frigg instead of plain shell reads or scans
- how to adopt repositories and move through search, symbol, and navigation flows
- how to treat lexical-only hybrid results, call-graph answers, and other weaker surfaces
- how to use `read_match`, structural queries, and bounded follow-up tools efficiently

## Optional Semantic Search

Semantic retrieval is off by default. When enabled, it improves recall for natural-language queries, but Frigg still grounds answers in local lexical, path, graph, symbol, and structural evidence.

Semantic refresh participates in reindex and watch-driven updates. If semantic search is enabled, Frigg may call the configured embedding provider automatically as the workspace changes, not only when you run a manual reindex.

OpenAI:

```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=openai
export OPENAI_API_KEY=<API_KEY>
```

Google:

```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=google
export GEMINI_API_KEY=<API_KEY>
```

Local:

```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
# Optional: local is the default provider when semantic runtime is enabled.
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=local
```

The local provider runs embeddings on this machine with the default model `all-MiniLM-L6-v2`. Zero-cloud semantic behavior applies only when `provider=local`; OpenAI and Google semantic providers call their external embedding APIs.

Optional model override:

```bash
export FRIGG_SEMANTIC_RUNTIME_MODEL=text-embedding-3-small
```

When `provider=local`, Frigg prepares missing local model artifacts automatically during startup. If a download is needed, startup reports `semantic_model_prepare status=started` and `status=finished`; stdio MCP mode sends those lines to stderr so stdout remains reserved for protocol frames. If local artifacts are corrupt or unavailable and cannot be prepared, startup fails with `local_model_prepare_failed` so the cache issue can be fixed explicitly.

After enabling semantic search for an existing repository, or after changing the semantic provider or model, run one semantic reindex pass:

```bash
frigg reindex
```

Provider defaults:

- `openai` -> `text-embedding-3-small`
- `google` -> `gemini-embedding-001`
- `local` -> `all-MiniLM-L6-v2`

## Optional SCIP Artifacts

Frigg can consume external SCIP artifacts for more precise definitions, references, implementations, and call navigation. It can also automatically detect and invoke supported generator tools during `workspace_attach` and MCP `workspace_reindex` flows for Rust, Go, TypeScript / JavaScript, Python, PHP, and Kotlin.

Java source support is available, but current JVM auto-generation is intentionally scoped to Gradle/KTS workspaces with Kotlin source files. Java/JVM and other Kotlin/JVM layouts should use manual `.frigg/scip/` artifact drops.

Manual artifacts belong in:

```text
.frigg/scip/
```

Create the directory if you want to pre-populate artifacts yourself:

```bash
mkdir -p .frigg/scip
```

Useful SCIP starting points:

- [Sourcegraph indexers](https://sourcegraph.com/docs/code-search/code-navigation/references/indexers)
- Rust: [rust-analyzer](https://github.com/rust-lang/rust-analyzer)
- PHP: [scip-php](https://github.com/davidrjenni/scip-php)
- Laravel: [scip-laravel](https://github.com/bnomei/scip-laravel)
- TypeScript / JavaScript: [scip-typescript](https://github.com/sourcegraph/scip-typescript)
- Python: [scip-python](https://github.com/sourcegraph/scip-python)
- Kotlin / Gradle, Java / JVM: [scip-java](https://sourcegraph.github.io/scip-java/docs/getting-started.html)

Laravel PHP workspaces prefer repo-local `vendor/bin/scip-laravel` when `bootstrap/app.php` is present. Otherwise Frigg uses the existing PHP `vendor/bin/scip-php` or `scip-php` lookup.

Frigg distills existing artifacts into snapshot-scoped retrieval projections on the next `frigg reindex` or MCP `workspace_reindex`. Server startup alone does not change retrieval state. Without SCIP data, Frigg still works with heuristic and source-backed navigation plus path and AST-derived retrieval summaries.

Optional repository-local precise config lives at `.frigg/precise.json`:

```json
{
  "precise": {
    "disabled_generators": ["python"],
    "generation_excludes": ["vendor/**", "generated/**"],
    "ingest_excludes": ["**/python-tests.scip"],
    "generator_extra_args": {
      "python": ["--target-only", "src/app"]
    }
  }
}
```

`workspace_attach` and `workspace_reindex` support `wait_for_precise`, which defaults to `true`. Pass `wait_for_precise=false` to return without waiting for precise generation. This skips only the wait; it does not disable attach-time index refresh, generator discovery, or generation scheduling.

## Built-In Watch Mode

Frigg includes a built-in watch worker behind `frigg serve` that keeps indexed repositories fresh with changed-only refreshes.

- watchers activate only while active sessions hold watcher leases for adopted repositories
- refreshes update `.frigg/storage.sqlite3`, not a separate sidecar index
- repository-scoped caches are invalidated only for the repository that changed
- if you already run an external watcher, start Frigg with `--watch-mode off`

Default watch behavior:

- `frigg serve` over loopback HTTP: `auto`
- stdio-style utility commands: `off`
- explicit `--watch-mode on` or `--watch-mode off`: overrides the transport default

## MCP Tool Surface

Frigg exposes the `extended` MCP tool surface by default. Set `FRIGG_MCP_TOOL_SURFACE_PROFILE=core` when you need the restricted stable subset without `explore` or deep-search tools.

Core profile tool groups:

- workspace lifecycle: `list_repositories`, `workspace_attach`, `workspace_detach`, `workspace_prepare`, `workspace_reindex`, `workspace_current`
- source reads: `read_file`, `read_match`
- discovery: `search_text`, `search_hybrid`, `search_symbol`
- navigation: `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`
- structure: `document_symbols`, `inspect_syntax_tree`, `search_structural`

Read tools default to text-first output. Pass `presentation_mode=json` when a caller needs the structured compatibility payload.

Structural follow-up suggestions are opt-in with `include_follow_up_structural=true`. Phase 1 covers `inspect_syntax_tree` and `search_structural`; phase 2 covers `document_symbols`, `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, and `outgoing_calls`. `search_hybrid` and `search_symbol` do not emit these suggestions.

## Context Efficiency

Frigg can report context efficiency for bounded source-returning tools. The feature describes how much indexed readable surface was available in the latest repository manifest, how much source was returned, and the resulting narrowing from repository surface to response evidence.

The response field is opt-in per tool call. Pass `include_context_efficiency=true` to include `context_efficiency` metadata in supported MCP responses. When the flag is omitted or false, Frigg keeps those response fields out of the tool payload.

Context-efficiency metadata is computed from the current response and existing index state. It is not stored in SQLite. The indexed readable surface comes from latest manifest metadata, and returned full-file totals use manifest sizes for the unique returned paths. This is separate from `returned_source_bytes_estimate`, which counts the returned source windows, excerpts, or read contents assembled for the response. `narrowing_ratio_estimate` is an estimate derived from those returned source bytes against the indexed readable bytes.

Set `FRIGG_CONTEXT_EFFICIENCY_LOG=true` to append compact JSONL rows under the active repository's `.frigg/context.jsonl`. This logging control is independent from `include_context_efficiency=true`: logging can be enabled while response metadata remains omitted, and response metadata can be requested without enabling JSONL logging.

Summarize local logs with:

```bash
frigg context
frigg context --since 2026-06-01 --until 2026-07-01
```

`frigg context` reads `.frigg/context.jsonl` for configured workspace roots and emits compact summary JSON, not raw event rows. Without date filters it summarizes the last 30 days. The output includes root `date_since` and `date_until` fields for the resolved range.

Context-efficiency v1 covers `search_hybrid`, `search_text`, `read_file`, `read_match`, and `explore`.

Extended-only tools:

- `explore`: bounded follow-up exploration for a single artifact after discovery
- `deep_search_run`: run a deeper multi-step search workflow
- `deep_search_replay`: replay a prior deep-search trace
- `deep_search_compose_citations`: build citation payloads from deep-search output

For operational behavior, use the [Frigg Operator Runbook](docs/operator-runbook.md). For agent-facing usage guidance, use the [bundled skill](skills/frigg-mcp-search-navigation/).

## Configuration

Precedence is `CLI flag > env var > default`.

| Flag / Env | Default | Meaning |
| --- | --- | --- |
| `--quiet` / `--verbose` | normal output | Controls CLI command output. Normal mode prints stable summaries/results, `--quiet` prints errors only except required machine/protocol stdout, and `--verbose` adds progress and diagnostics on stderr. |
| `--workspace-root` | utility commands default to current directory; serving mode can start empty | Limits what Frigg can read and index. Repeatable. In serving mode, roots become the global known-repository catalog. |
| `--max-file-bytes` / `FRIGG_MAX_FILE_BYTES` | `2097152` | Maximum file size Frigg will read. |
| `--full-scip-ingest` / `FRIGG_FULL_SCIP_INGEST` | `true` | Disables precise navigation SCIP ingest budgets. This is the default. |
| `--watch-mode` / `FRIGG_WATCH_MODE` | stdio `off`, HTTP `auto` | Controls the built-in watch worker: `auto`, `on`, or `off`. |
| `--watch-debounce-ms` / `FRIGG_WATCH_DEBOUNCE_MS` | `2000` | Debounce delay before a watch-triggered refresh starts. |
| `--watch-retry-ms` / `FRIGG_WATCH_RETRY_MS` | `5000` | Retry delay after a failed watch refresh. |
| `--mcp-http-port` | `37444` for `frigg serve`, unset otherwise | Enables HTTP transport on the given port. |
| `--mcp-http-host` | `127.0.0.1` when HTTP is enabled | Host bind address for HTTP transport. |
| `--allow-remote-http` | `false` | Required for non-loopback HTTP serving. |
| `--mcp-http-auth-token` / `FRIGG_MCP_HTTP_AUTH_TOKEN` | unset | Bearer token for HTTP mode. Required for non-loopback HTTP. |
| `--lexical-backend` / `FRIGG_LEXICAL_BACKEND` | `auto` | Lexical backend: `auto`, `native`, or `ripgrep`. |
| `--ripgrep-executable` / `FRIGG_RIPGREP_EXECUTABLE` | unset | Path to an `rg` executable used when the ripgrep backend is selected. |
| `FRIGG_MCP_TOOL_SURFACE_PROFILE` | `extended` | MCP tool surface profile: `extended` or `core`. |
| `FRIGG_CONTEXT_EFFICIENCY_LOG` | `false` | When truthy, appends compact context-efficiency rows to `.frigg/context.jsonl` independently of response metadata opt-in. |
| `--semantic-runtime-enabled` / `FRIGG_SEMANTIC_RUNTIME_ENABLED` | `false` | Enables optional semantic retrieval. |
| `--semantic-runtime-provider` / `FRIGG_SEMANTIC_RUNTIME_PROVIDER` | `local` when semantic runtime is enabled | Semantic provider: `openai`, `google`, or `local`. |
| `--semantic-runtime-model` / `FRIGG_SEMANTIC_RUNTIME_MODEL` | provider default | Optional embedding model override. |
| `--semantic-runtime-strict-mode` / `FRIGG_SEMANTIC_RUNTIME_STRICT_MODE` | `false` | Converts semantic provider failures into user-visible errors instead of graceful fallback. |

For local performance work, Frigg ships a small Criterion harness:

```bash
just bench
just bench native_lexical_search
```

## Showcases

The [showcases/](showcases/) directory contains 52 public example catalogs for real repositories. Each JSON file records realistic questions and the paths a good Frigg answer should surface.

## Safety And Boundaries

- Frigg indexes source files only inside configured workspace roots.
- Frigg keeps primary state locally in SQLite.
- Frigg avoids editing source files during normal indexing.
- Optional semantic search may call an external embedding provider if you enable it.
- Optional precise generators may write `.frigg/scip/` artifacts, execute repo-local or PATH-discovered tools, or apply generator-specific compatibility patches. Those external tools have their own filesystem behavior outside Frigg's source-indexing boundary.
- Workspace and index maintenance tools such as `workspace_prepare` and `workspace_reindex` are confirm-gated and operate on Frigg state.
- Session adoption and watcher leases are runtime/session state. `workspace_current.repositories` is session-local; `list_repositories` is the global known-repository catalog.

Frigg's product boundary is intentionally narrow: local code evidence over MCP, not a full IDE, hosted code intelligence platform, or framework runtime.

## License

Frigg's crate metadata declares `MIT AND MPL-2.0`. The root [LICENSE](LICENSE) file contains the MIT license text. The MPL-2.0 text is bundled at [crates/cli/LICENSES/MPL-2.0.txt](crates/cli/LICENSES/MPL-2.0.txt).
