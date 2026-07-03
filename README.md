# frigg

[![Crates.io Version](https://img.shields.io/crates/v/frigg)](https://crates.io/crates/frigg)
[![Build Status](https://github.com/bnomei/frigg/actions/workflows/ci.yml/badge.svg)](https://github.com/bnomei/frigg/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20AND%20MPL--2.0-blue)](#license)
[![Discord](https://flat.badgen.net/badge/discord/bnomei?color=7289da&icon=discord&label)](https://discordapp.com/users/bnomei)
[![Buymecoffee](https://flat.badgen.net/badge/icon/donate?icon=buymeacoffee&color=FF813F&label)](https://www.buymeacoffee.com/bnomei)

Frigg is the local code evidence layer for AI coding agents.

It gives Codex, Claude, Cursor, and other MCP clients fast, source-backed repository search, symbol navigation, structural search, and bounded reads without pushing the whole codebase through every prompt.

Frigg is local-first OSS. It builds a repository model under `.frigg/`, serves it over MCP, and keeps the useful parts plain: a CLI, a local SQLite store, explicit client adoption, cacheable runtime state, and source-backed answers.

## Why Frigg

For individuals, Frigg keeps coding agents grounded in local source instead of broad scans, whole-file dumps, and model-memory guesses. It helps an agent find the right files, read only the source windows it needs, follow symbols, and answer with concrete repository evidence.

For teams, Frigg standardizes how agents inspect a repository. `frigg adopt`, shared MCP config, CI-cacheable `.frigg/` state, local safety boundaries, and repeatable search/navigation tools give every agent session the same evidence layer and vocabulary across large or unfamiliar codebases.

Frigg is the default for code discovery, navigation, exact code search, and bounded source reads. Start with Frigg MCP tools when you need to find code, inspect symbols, follow relationships, search exact source text, or read bounded source windows from an attached repository. Use shell tools for non-code files, git and filesystem inspection, and trivial one-off checks where a direct command is faster and does not replace code discovery or bounded source reads.

## What Frigg is not

Frigg is not an AI pair programmer, hosted code intelligence platform, IDE replacement, Copilot alternative, or generic semantic search product. MCP is the delivery channel, not the category; SQLite, Tree-sitter, semantic recall, and SCIP are implementation proof points, not the reason to adopt Frigg.

The narrow promise is local code evidence for AI agents: repository-aware search, navigation, and bounded reads that make agent work easier to verify.

## What Frigg provides

- One local MCP service that can serve multiple adopted repositories.
- Local state in `.frigg/storage.sqlite3`.
- Tree-sitter-backed document symbols and structural search for Rust, PHP, Blade, TypeScript / TSX, Python, Go, Kotlin / KTS, Java, Lua, Roc, and Nim.
- Direct literal or regex code search with `search_text`.
- Broad discovery with `search_hybrid`, blending lexical, path, graph, witness, optional semantic, and code-aware ranking signals.
- Known-identifier lookup with `search_symbol`.
- Bounded source reads with `read_file` and `read_match`.
- Definitions, declarations, references, implementations, incoming calls, and outgoing calls.
- Optional semantic indexing with local, OpenAI, or Google embedding providers.
- Optional SCIP artifact ingestion and generator assistance for more precise navigation.
- Built-in watch refreshes behind `frigg serve`.

## Quickstart

### 1. Install Frigg

Install a pinned release on macOS or GNU/glibc Linux:

```bash
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
  https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh \
  | FRIGG_VERSION=0.5.0 sh
```

The installer downloads the matching GitHub Release archive, verifies its `.sha256`, and installs the `frigg` binary to `$HOME/.local/bin` unless `FRIGG_INSTALL_DIR` is set. When `FRIGG_VERSION` is unset, it resolves the latest GitHub Release. The installer supports macOS and GNU/glibc Linux on x86_64 and aarch64; Windows users should download the release `.zip` asset manually.

Rust developer fallback:

```bash
cargo install frigg
```

Build from a local checkout:

```bash
git clone https://github.com/bnomei/frigg.git
cd frigg
cargo build --release -p frigg
target/release/frigg --version
```

Expected output:

```text
frigg 0.5.0
```

Frigg's source currently requires Rust 1.88 or newer.

### 2. Index a repository

Run these commands from the repository you want Frigg to index:

```bash
frigg init
frigg index
```

Expected successful output includes:

```text
ok init: complete status=ok
ok index: complete status=ok
```

From outside the repository, pass an explicit root:

```bash
frigg init --workspace-root /absolute/path/to/repo
frigg index --workspace-root /absolute/path/to/repo
```

### 3. Start the MCP service

Run:

```bash
frigg serve
```

By default, `frigg serve` listens on loopback HTTP:

```text
http://127.0.0.1:37444/mcp
```

Keep this process running while clients use Frigg. A server can start without startup roots, and MCP clients can attach repositories later. To preload known repositories at startup:

```bash
frigg serve \
  --workspace-root /absolute/path/to/repo-a \
  --workspace-root /absolute/path/to/repo-b
```

### 4. Add client configuration

Use `frigg adopt` to add managed Frigg instructions and MCP config entries to a project:

```bash
frigg adopt --target agents-md --target mcp-project --dry-run
frigg adopt --target agents-md --target mcp-project
```

Useful targets include `agents-md`, `claude-md`, `gemini-md`, `copilot`, `cursor`, `mcp-project`, `mcp-cursor`, and `hook`. Use `--all` to update every supported non-hook target, `--check` for a CI drift check, `--uninstall` to remove Frigg-managed entries, and `--force` to replace a diverged Frigg MCP JSON entry.

Manual MCP configuration for clients that accept JSON:

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

Manual CLI examples:

```bash
claude mcp add --transport http frigg http://127.0.0.1:37444/mcp
codex mcp add frigg --url http://127.0.0.1:37444/mcp
```

### 5. Use Frigg from an MCP client

The normal MCP loop is:

1. Call `list_repositories`.
2. Call `workspace_attach` if the session is detached or if you want a specific session-default repository.
3. Call `workspace_current` when you need repository health, index freshness, precise status, or runtime task state.
4. Use `search_hybrid` for broad discovery when you do not have an exact string, symbol, or path anchor.
5. Use `search_text` for literal or regex matches.
6. Use `search_symbol` for known identifiers.
7. Use `read_match` when a search or navigation result returned `result_handle` plus `match_id`.
8. Use `read_file` when you already know the canonical repository-relative path.
9. Use navigation and structure tools for definitions, references, implementations, calls, outlines, syntax trees, and structural queries.

Example prompts:

- "Where is authentication bootstrapped?"
- "Show me implementations of `ProviderInterface`."
- "Who calls `handleWebhook`?"
- "Which files are relevant to the checkout flow?"

For agent-facing usage guidance, use [skills/frigg-mcp-search-navigation](skills/frigg-mcp-search-navigation/). For runtime diagnosis, use the [Frigg Operator Runbook](docs/operator-runbook.md).

## CLI reference

| Command | Purpose |
| --- | --- |
| `frigg serve` | Start the MCP service over loopback HTTP by default. |
| `frigg adopt` | Add or remove managed Frigg entries in agent docs and MCP configs. |
| `frigg init` | Create or repair `.frigg/storage.sqlite3` without scanning source files. |
| `frigg index` | Scan files, refresh the local search index, and refresh semantic rows when semantic runtime is enabled. |
| `frigg index --changed` | Recheck files changed since the last index. |
| `frigg hash` | Print the stable CI cache fingerprint as `frigg-hash=<hex>`. |
| `frigg context` | Summarize context-efficiency JSONL logs when logging is enabled. |

`frigg reindex` remains a compatibility alias for `frigg index`.

## MCP tool surface

Frigg exposes the `extended` MCP tool surface by default. Set `FRIGG_MCP_TOOL_SURFACE_PROFILE=core` to restrict the server to the stable core set.

Core tool groups:

- Workspace lifecycle: `list_repositories`, `workspace_attach`, `workspace_detach`, `workspace_prepare`, `workspace_index`, `workspace_current`.
- Source reads: `read_file`, `read_match`.
- Discovery: `search_text`, `search_hybrid`, `search_symbol`.
- Navigation: `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`.
- Structure: `document_symbols`, `inspect_syntax_tree`, `search_structural`.

Extended-only tools in default builds:

- `explore`

Feature-gated extended-only playbook tools are available only when Frigg is compiled with `--features playbook`:

- `playbook_run`
- `playbook_replay`
- `playbook_compose_citations`

Read tools default to text-first output. Request `presentation_mode=json` only when the caller needs structured fields such as path, byte ranges, or context-efficiency metadata. Search and navigation tools default to compact responses; request `response_mode=full` when diagnostics, freshness details, or selection notes matter.

## Configuration

Precedence is `CLI flag > environment variable > default`.

| Flag / environment variable | Default | Meaning |
| --- | --- | --- |
| `--workspace-root` | Utility commands use the current directory; `serve` can start empty | Repository root Frigg may read and index. Repeatable. |
| `--max-file-bytes` / `FRIGG_MAX_FILE_BYTES` | `2097152` | Maximum file size Frigg reads. |
| `--full-scip-ingest` / `FRIGG_FULL_SCIP_INGEST` | `true` | Ingest full SCIP artifacts when present. |
| `--mcp-http-port` | `37444` for `frigg serve` | HTTP port. |
| `--mcp-http-host` | `127.0.0.1` when HTTP is enabled | HTTP bind host. |
| `--allow-remote-http` | `false` | Required for non-loopback HTTP binds. |
| `--mcp-http-auth-token` / `FRIGG_MCP_HTTP_AUTH_TOKEN` | unset | Bearer token for HTTP MCP requests. Required for non-loopback binds. |
| `--watch-mode` / `FRIGG_WATCH_MODE` | `auto` | Watch mode: `auto`, `on`, or `off`. |
| `--watch-debounce-ms` / `FRIGG_WATCH_DEBOUNCE_MS` | `2000` | Delay before a watch-triggered refresh starts. |
| `--watch-retry-ms` / `FRIGG_WATCH_RETRY_MS` | `5000` | Retry delay after a failed watch refresh. |
| `--lexical-backend` / `FRIGG_LEXICAL_BACKEND` | `auto` | Lexical backend: `auto`, `native`, or `ripgrep`. |
| `--ripgrep-executable` / `FRIGG_RIPGREP_EXECUTABLE` | PATH lookup | `rg` executable used by the ripgrep backend. |
| `FRIGG_MCP_TOOL_SURFACE_PROFILE` | `extended` | MCP surface profile: `extended` or `core`. |
| `FRIGG_SQLITE_BUSY_TIMEOUT_MS` | `30000` | SQLite wait timeout for transient writer contention. |
| `FRIGG_CONTEXT_EFFICIENCY_LOG` | `false` | Append compact context-efficiency rows to `.frigg/context.jsonl`. |
| `--semantic-runtime-enabled` / `FRIGG_SEMANTIC_RUNTIME_ENABLED` | `false` | Enable semantic indexing and recall. |
| `--semantic-runtime-provider` / `FRIGG_SEMANTIC_RUNTIME_PROVIDER` | `local` when semantic runtime is enabled | Semantic provider: `local`, `openai`, or `google`. |
| `--semantic-runtime-model` / `FRIGG_SEMANTIC_RUNTIME_MODEL` | provider default | Embedding model override. |
| `--semantic-runtime-strict-mode` / `FRIGG_SEMANTIC_RUNTIME_STRICT_MODE` | `false` | Convert semantic provider failures into user-visible errors instead of graceful fallback. |

Built-in watch mode runs behind `frigg serve` and refreshes adopted repositories while active MCP sessions hold watcher leases. It updates `.frigg/storage.sqlite3`; it does not create a separate sidecar index.

## Semantic search

Semantic retrieval is off by default. When enabled, it improves natural-language recall, but Frigg still grounds answers in local lexical, path, graph, symbol, structural, and source evidence.

Local provider:

```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=local
frigg index
frigg serve
```

The local provider uses `all-MiniLM-L6-v2` by default and does not require an API key. Missing local model artifacts are prepared automatically during semantic runtime startup. Set `FRIGG_SEMANTIC_MODEL_CACHE` to choose the local model cache root. If `HF_HOME` is set and local model loading fails, unset `HF_HOME` so Frigg's cache selection controls the prepared artifacts.

OpenAI provider:

```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=openai
export OPENAI_API_KEY=<API_KEY>
frigg index
```

Google provider:

```bash
export FRIGG_SEMANTIC_RUNTIME_ENABLED=true
export FRIGG_SEMANTIC_RUNTIME_PROVIDER=google
export GEMINI_API_KEY=<API_KEY>
frigg index
```

Provider defaults:

| Provider | Default model | Credential |
| --- | --- | --- |
| `local` | `all-MiniLM-L6-v2` | none |
| `openai` | `text-embedding-3-small` | `OPENAI_API_KEY` |
| `google` | `gemini-embedding-001` | `GEMINI_API_KEY` |

After enabling semantic search for an existing repository, or after changing provider or model, run a full `frigg index`.

## Precise navigation with SCIP

Frigg works without SCIP data by using Tree-sitter, lexical search, structural search, and source-backed heuristics. For more precise definitions, references, implementations, and call navigation, place SCIP artifacts in:

```text
.frigg/scip/
```

Frigg can ingest `.scip` protobuf and `.json` SCIP artifacts. CLI `init` and `index`, plus MCP workspace flows, can also run supported precise generators when the necessary tools are available. Generator assistance currently covers Rust, Go, TypeScript / JavaScript, PHP, Python, and Kotlin. Java source is supported by Tree-sitter; JVM layouts that need SCIP precision can provide manual artifacts.

Optional repository-local precise configuration lives at `.frigg/precise.json`:

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

Use `workspace_current` to inspect precise state, generator availability, failures, and recommended actions.

Useful SCIP starting points:

- [Sourcegraph indexers](https://sourcegraph.com/docs/code-search/code-navigation/references/indexers)
- Rust: [rust-analyzer](https://github.com/rust-lang/rust-analyzer)
- PHP: [scip-php](https://github.com/davidrjenni/scip-php)
- Laravel: [scip-laravel](https://github.com/bnomei/scip-laravel)
- TypeScript / JavaScript: [scip-typescript](https://github.com/sourcegraph/scip-typescript)
- Python: [scip-python](https://github.com/sourcegraph/scip-python)
- Kotlin / Gradle and Java / JVM: [scip-java](https://sourcegraph.github.io/scip-java/docs/getting-started.html)

## GitHub Actions

Use a pinned Frigg release in CI:

```yaml
name: Frigg

on:
  pull_request:
  push:
    branches: [main]

env:
  FRIGG_VERSION: 0.5.0
  FRIGG_INSTALL_DIR: ${{ github.workspace }}/.frigg-bin

jobs:
  frigg:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5

      - name: Install Frigg
        run: |
          curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
            https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh \
            | FRIGG_VERSION="${FRIGG_VERSION}" FRIGG_INSTALL_DIR="${FRIGG_INSTALL_DIR}" sh

      - run: frigg init
      - run: frigg index
```

For larger repositories, cache `.frigg/` as regenerable build output. Restore first, then run `frigg init` and `frigg index --changed` to validate and refresh restored state. Save the cache only from trusted events:

```yaml
name: Frigg

on:
  pull_request:
  push:
    branches: [main]

env:
  FRIGG_VERSION: 0.5.0
  FRIGG_INSTALL_DIR: ${{ github.workspace }}/.frigg-bin

jobs:
  frigg:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5

      - name: Install Frigg
        run: |
          curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
            https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh \
            | FRIGG_VERSION="${FRIGG_VERSION}" FRIGG_INSTALL_DIR="${FRIGG_INSTALL_DIR}" sh

      - name: Compute Frigg cache hash
        id: frigg_hash
        run: frigg hash

      - name: Restore Frigg state
        id: frigg_cache
        uses: actions/cache/restore@v4
        with:
          path: .frigg/
          key: frigg-${{ runner.os }}-${{ runner.arch }}-${{ env.FRIGG_VERSION }}-${{ steps.frigg_hash.outputs.frigg-hash }}-${{ github.sha }}
          restore-keys: |
            frigg-${{ runner.os }}-${{ runner.arch }}-${{ env.FRIGG_VERSION }}-${{ steps.frigg_hash.outputs.frigg-hash }}-

      - run: frigg init
      - run: frigg index --changed

      - name: Save Frigg state
        if: github.event_name == 'push' && github.ref == 'refs/heads/main'
        uses: actions/cache/save@v4
        with:
          path: .frigg/
          key: frigg-${{ runner.os }}-${{ runner.arch }}-${{ env.FRIGG_VERSION }}-${{ steps.frigg_hash.outputs.frigg-hash }}-${{ github.sha }}
```

Treat `.frigg/` as cacheable runtime state, not as source and not as a secret store.

## Safety and boundaries

- Frigg reads and indexes files only inside configured workspace roots.
- Frigg honors root `.gitignore` and `.ignore` files and hard-excludes `.frigg`, `.git`, and `target`.
- Put secrets and generated private artifacts in ignored paths before indexing. Frigg stores indexed source-derived state locally.
- Normal indexing does not edit project source files.
- `workspace_attach` defaults to `index_mode=ensure`, which may refresh stale or missing lexical and semantic state under `.frigg/` and waits up to the attach timeout.
- `index_mode=skip` skips lexical and semantic refresh only; it still attaches the repository and can preserve precise-generation scheduling behavior.
- MCP `workspace_prepare` and `workspace_index` are confirm-gated because they operate on ignored `.frigg/` state.
- Optional OpenAI and Google semantic providers call external embedding APIs. The local provider does not require an API key.
- Optional precise generators may execute repo-local or PATH-discovered tools and write `.frigg/scip/` artifacts. Inspect `workspace_current` for generator status and touch-risk details.
- Non-loopback HTTP serving requires `--allow-remote-http` and `--mcp-http-auth-token`.

Frigg's product boundary is intentionally narrow: local code evidence over MCP. It is not a full IDE, hosted code intelligence platform, or framework runtime.

## Project layout

| Path | Purpose |
| --- | --- |
| [crates/cli/src/cli_args.rs](crates/cli/src/cli_args.rs) | Public CLI flags and commands. |
| [crates/cli/src/mcp](crates/cli/src/mcp) | MCP server, tool contracts, runtime state, and guidance resources. |
| [crates/cli/src/indexer](crates/cli/src/indexer) | Manifest, symbol, semantic, and index refresh logic. |
| [crates/cli/src/searcher](crates/cli/src/searcher) | Lexical, hybrid, semantic, ranking, projection, and policy code. |
| [crates/cli/src/storage](crates/cli/src/storage) | SQLite schema, manifests, semantic rows, vectors, and provenance. |
| [crates/cli/src/languages](crates/cli/src/languages) | Language registry, Tree-sitter support, and language-specific heuristics. |
| [crates/cli/src/embeddings](crates/cli/src/embeddings) | Local, OpenAI, and Google embedding providers. |
| [crates/cli/src/watch](crates/cli/src/watch) | Built-in watch runtime. |
| [crates/cli/tests](crates/cli/tests) | Integration and MCP tool-handler tests. |
| [crates/cli/benches](crates/cli/benches) | Criterion benchmark harnesses. |
| [scripts](scripts) | Release packaging, install, smoke, and helper scripts. |
| [showcases](showcases) | Public corpus of 52 repository question catalogs. |
| [docs/operator-runbook.md](docs/operator-runbook.md) | Runtime diagnosis and operator states. |
| [skills/frigg-mcp-search-navigation](skills/frigg-mcp-search-navigation/) | Agent-facing Frigg usage skill. |

## Development

Rust requirements and package metadata live in [Cargo.toml](Cargo.toml). Common local commands:

```bash
just fmt
just test
just build
just build-release
```

Equivalent CI-grade checks:

```bash
cargo fmt --all -- --check
cargo clippy -p frigg --all-targets -- -D warnings
cargo test -p frigg --all-targets
cargo test --locked -p frigg --doc
cargo bench --locked -p frigg --no-run
sh scripts/test-install.sh
```

Run a development server from source:

```bash
just serve
```

Index a repository from source:

```bash
just init /absolute/path/to/repo
just index /absolute/path/to/repo
```

## Troubleshooting

No repositories appear in a client:

1. Confirm `frigg serve` is running.
2. Call `list_repositories`.
3. Call `workspace_attach` with the repository path or repository id.
4. Check `workspace_current` for the session default and repository health.

Search results look stale:

1. Call `workspace_current` and inspect `index_lifecycle`.
2. Reattach with `index_mode=ensure`, or run `workspace_index` with confirmation.
3. If you use the CLI, run `frigg index --changed`.

Semantic recall is unavailable or weak:

1. Confirm `FRIGG_SEMANTIC_RUNTIME_ENABLED=true`.
2. Confirm the provider and credentials for `openai` or `google`.
3. For `local`, check `FRIGG_SEMANTIC_MODEL_CACHE` and unset `HF_HOME` if model loading reports a cache mismatch.
4. Run a full `frigg index` after provider or model changes.

SQLite reports `database is locked`:

1. Stop duplicate Frigg processes that are writing the same `.frigg/storage.sqlite3`.
2. Increase `FRIGG_SQLITE_BUSY_TIMEOUT_MS` for test-heavy local workflows.
3. Disable watch with `--watch-mode off` if another process already maintains the index.

Non-loopback HTTP bind fails:

1. Pass `--allow-remote-http`.
2. Set `--mcp-http-auth-token` or `FRIGG_MCP_HTTP_AUTH_TOKEN`.
3. Prefer loopback unless a remote client genuinely needs access.

## Source anchors

- Workspace metadata: [Cargo.toml](Cargo.toml)
- CLI contract: [crates/cli/src/cli_args.rs](crates/cli/src/cli_args.rs)
- MCP tool names and public state contracts: [crates/cli/src/mcp/types.rs](crates/cli/src/mcp/types.rs)
- HTTP serving rules: [crates/cli/src/http_runtime.rs](crates/cli/src/http_runtime.rs)
- Semantic provider defaults: [crates/cli/src/settings/semantic_runtime.rs](crates/cli/src/settings/semantic_runtime.rs)
- Workspace configuration defaults: [crates/cli/src/settings/frigg_config.rs](crates/cli/src/settings/frigg_config.rs)
- Installer behavior: [scripts/install.sh](scripts/install.sh)
- CI checks: [.github/workflows/ci.yml](.github/workflows/ci.yml)

## License

Frigg's crate metadata declares `MIT AND MPL-2.0`. The root [LICENSE](LICENSE) file contains the MIT license text. The MPL-2.0 text is bundled at [crates/cli/LICENSES/MPL-2.0.txt](crates/cli/LICENSES/MPL-2.0.txt).
