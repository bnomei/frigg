# frigg

[![Crates.io Version](https://img.shields.io/crates/v/frigg)](https://crates.io/crates/frigg)
[![Build Status](https://github.com/bnomei/frigg/actions/workflows/ci.yml/badge.svg)](https://github.com/bnomei/frigg/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20AND%20MPL--2.0-blue)](#license)
[![Discord](https://flat.badgen.net/badge/discord/bnomei?color=7289da&icon=discord&label)](https://discordapp.com/users/bnomei)
[![Buymecoffee](https://flat.badgen.net/badge/icon/donate?icon=buymeacoffee&color=FF813F&label)](https://www.buymeacoffee.com/bnomei)

Frigg gives AI agents local, source-backed code search and navigation without sending whole repositories through every prompt.

It turns each repository into searchable, navigable context for Codex, Claude, Cursor, and other MCP clients: lexical and hybrid search, symbols, structural queries, definitions, references, bounded reads, and runtime health through one local MCP service.

Frigg is local-first OSS. It builds a repository model under `.frigg/`, serves it over MCP, and keeps the useful parts plain: a CLI, a local SQLite store, explicit client adoption, cacheable runtime state, and source-backed answers.

[![asciicast](https://raw.githubusercontent.com/bnomei/frigg/main/frigg.gif)](https://asciinema.org/a/1260121)

## Why Frigg

AI coding agents need retrieval as much as generation: the right answer starts with the right repository evidence. [Code Context Engine](https://elara-labs.github.io/code-context-engine/) shows the local-index pattern for AI coding agents, where the agent searches through MCP instead of rereading whole files. [Sourcegraph Code Search](https://sourcegraph.com/docs/code-search) documents production code-intelligence primitives such as full-text search, regular expressions, symbol search, repository scoping, ranking, and code navigation. [turbopuffer Hybrid Search](https://turbopuffer.com/docs/hybrid) shows why lexical/BM25 and vector signals are often combined with rank fusion and re-ranking. Frigg brings that shape to a local OSS MCP service for source-backed agent context.

For individuals, Frigg keeps coding agents grounded in local source instead of broad scans, whole-file dumps, and model-memory guesses. It helps an agent find the right files, read only the source windows it needs, follow symbols, and answer with concrete repository evidence.

For teams, Frigg standardizes how agents inspect a repository. `frigg adopt`, shared MCP config, CI-cacheable `.frigg/` state, local safety boundaries, and repeatable search/navigation tools give every agent session the same evidence layer and vocabulary across large or unfamiliar codebases.

Use Frigg when shell scans stop being enough: broad discovery, exact source windows, symbol and call navigation, structural queries, optional semantic recall, and optional SCIP-backed precision.

## What Frigg is not

Frigg is not an AI pair programmer, hosted code intelligence platform, IDE replacement, Copilot alternative, or generic semantic search product. MCP is the delivery channel, not the category; SQLite, Tree-sitter, semantic recall, and SCIP are implementation proof points, not the reason to adopt Frigg.

The narrow promise is source-backed context for AI agents: repository-aware search, navigation, and bounded reads that make agent work easier to verify.

## What Frigg provides

- One local MCP service that can serve multiple adopted repositories.
- Local state in `.frigg/storage.sqlite3`.
- Tree-sitter-backed document symbols and structural search for Rust, PHP, Blade, TypeScript / TSX, Python, Go, Kotlin / KTS, Java, Lua, Roc, and Nim.
- Direct literal, safe-regex, and `rg`-shaped code scans with `search_text`.
- Broad discovery with `search_hybrid`, blending lexical, path, graph, witness, optional semantic, and code-aware ranking signals.
- Known-identifier lookup with `search_symbol`.
- Bounded source reads with `read_file` and `read_match` (including citation mode).
- Multi-probe search with `search_batch` and combined impact navigation with `impact_bundle`.
- Definitions, declarations, references, implementations, incoming calls, and outgoing calls.
- Optional semantic indexing with local, OpenAI, or Google embedding providers.
- Optional SCIP artifact ingestion and generator assistance for more precise navigation.
- Built-in watch refreshes behind `frigg serve`.

## Quickstart

### 1. Install Frigg

Fast path on macOS or GNU/glibc Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh | FRIGG_VERSION=0.8.0 sh
```

The installer downloads the matching GitHub Release archive, verifies its `.sha256`, and installs the `frigg` binary to `$HOME/.local/bin` unless `FRIGG_INSTALL_DIR` is set. When `FRIGG_VERSION` is unset, it resolves the latest GitHub Release.

Other install surfaces:

| Channel | Command |
| --- | --- |
| GitHub Release installer | `curl -fsSL https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh \| sh` |
| Cargo prebuilt binary | `cargo binstall frigg` |
| Cargo source fallback | `cargo install frigg` |
| npm wrapper | `npx @bnomei/frigg --version` |
| Docker image | `docker run --rm ghcr.io/bnomei/frigg:0.8.0 --version` |
| Scoop | `scoop bucket add frigg https://github.com/bnomei/scoop-frigg && scoop install frigg` |

The prebuilt paths are the point: one local binary, no Python 3.11+ runtime, no local C compiler, and no ONNX model download unless you explicitly enable the local semantic runtime.

Build from a local checkout:

```bash
git clone https://github.com/bnomei/frigg.git
cd frigg
cargo build --release -p frigg
target/release/frigg --version
```

Expected output:

```text
frigg 0.8.0
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

### 3. Choose and start the MCP transport

Frigg supports stdio and HTTP MCP transports. Use this rule before choosing a client config:

| Workflow | Transport |
| --- | --- |
| One local MCP client, no subagents, and the client should own the Frigg process | Stdio |
| More than one client, subagents, shared workflows, or long-running use | HTTP |

Stdio works, but it is a convenience transport for one local client. It is not the best match for Frigg once work becomes shared, long-running, or agentic. A stdio config makes the MCP client spawn and own a Frigg process. Another client or subagent usually means another Frigg process with its own lifecycle, session state, in-memory caches, and access to `.frigg/storage.sqlite3`.

HTTP gives Frigg a shared runtime:

- One endpoint for all clients and subagents. Codex, Claude, Cursor, and worker agents can connect to the same `127.0.0.1:37444/mcp` service instead of each spawning their own Frigg process.
- Warm process state. Repository registry state, runtime task tracking, search/navigation caches, compiled safe-regex caches, and precise graph caches can live in one service process across client sessions.
- Better freshness behavior. `frigg serve` is the long-lived service path, and built-in watch refreshes are designed around that shape. Stdio defaults to an ephemeral profile with watch disabled unless you explicitly opt in.
- Lower local contention. One service coordinates access to `.frigg/storage.sqlite3`; several stdio processes can duplicate work and make SQLite lock contention harder to understand.
- Clearer operations. A single HTTP service has one lifecycle, one log stream, one endpoint, loopback-by-default binding, and optional bearer auth for non-loopback use.

Use stdio only when one local MCP client should launch Frigg directly and no subagent or second client will share the same repository. Use HTTP for normal Frigg use.

For HTTP shared-service mode, run:

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

`frigg serve` is the HTTP service path. Do not use it in a stdio MCP config.

### 4. Add client configuration

Use `frigg adopt` to add managed Frigg instructions and MCP config entries to a project:

```bash
frigg adopt --target agents-md --target mcp-project --dry-run
frigg adopt --target agents-md --target mcp-project
frigg adopt --target agents-md --policy expanded
```

Useful targets include `agents-md`, `claude-md`, `gemini-md`, `copilot`, `cursor`, `mcp-project`, `mcp-cursor`, and `hook`. Use `--all` to update every supported non-hook target, `--check` for a CI drift check, `--uninstall` to remove Frigg-managed entries, and `--force` to replace a diverged Frigg MCP JSON entry.

Managed markdown defaults to a **lightweight** Frigg-first pointer to the `frigg-first-code-search` skill. Pass `--policy expanded` for a compact routing policy (scenario picker + shell→Frigg one-liners) when you want more detail in-repo without loading the skill.

The managed MCP JSON entries use loopback HTTP, so keep `frigg serve` running while clients use Frigg.

Manual HTTP configuration for clients that accept JSON:

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

Manual stdio configuration for a single local client:

```json
{
  "mcpServers": {
    "frigg": {
      "command": "frigg",
      "args": ["--workspace-root", "/absolute/path/to/repo"]
    }
  }
}
```

This uses stdio because there is no `serve` subcommand and no `--mcp-http-port`. The MCP client owns the Frigg process lifetime. Use this only for one local client with no subagents; switch to HTTP as soon as another client or subagent needs the same Frigg runtime.

### 5. Use Frigg from an MCP client

The normal MCP loop is:

1. Omit `repository_id` in normal single-repo work. Frigg uses the session default, adopted repositories, or auto-adopts a sensible default when possible.
2. Call `workspace` only when you want compact workspace status or need to adopt a specific `path` or `repository_id`.
3. Use `list_files` when you would otherwise run `rg --files`, `find`, or `fd`.
4. Use `workspace` when you need the session default, known repositories, or runtime task state.
5. Use `search_hybrid` for broad discovery when you do not have an exact string, symbol, or path anchor.
6. Use `search_text` for literal, safe-regex, and `rg`-shaped matches.
7. Use `search_symbol` for known identifiers.
8. Use `search_batch` when you have several search hypotheses and want concurrent probes merged in one call.
9. Use `read_match` when a search or navigation result returned `result_handle` plus `match_id`.
10. Use `read_file` when you already know the canonical repository-relative path.
11. Use navigation and structure tools for definitions, references, implementations, calls, outlines, syntax trees, and structural queries. Prefer `impact_bundle` when one symbol needs combined impact.

Shell replacement map:

| Shell habit | Frigg tool |
| --- | --- |
| `rg --files` | `list_files` |
| `rg -n "text"` | `search_text` |
| `rg -n "foo\|bar"` | `search_text` with `pattern_type=regex` |
| `rg -n "text" path/` | `search_text` with `path_regex` |
| identifier/API/type/class/function lookup | `search_symbol` |
| `cat path` | `read_file` |
| `sed -n '10,80p' path` | `read_file` with `start_line`, `end_line`, or `line_count` |
| follow definitions/references/calls | navigation tools |

`search_text` is the Frigg surface for the same class of code scans agents often run with `rg`: known literals, safe regexes, grouped alternation, `path_regex` narrowing, context windows, per-file limits (`max_count_per_file`), and "which files contain this?" probes with `files_with_matches`. Frigg may execute the lexical work with its native scanner or with its ripgrep accelerator; either path still returns repository-scoped matches, canonical paths, `result_handle`, and per-row `match_id` values.

For example, this `rg` scan:

```bash
rg -n 'PathBuf::from\(&.*path\)|Path::new\(&.*path\)|current_dir\(' crates
```

maps to:

```json
{
  "query": "PathBuf::from\\(&.*path\\)|Path::new\\(&.*path\\)|current_dir\\(",
  "pattern_type": "regex",
  "path_regex": "^crates/.*\\.rs$",
  "context_lines": 1,
  "limit": 100
}
```

Use shell only for git state and diffs, non-code files, build/test output, generated or unindexed files, explicit live-disk verification, ripgrep-specific flags outside `search_text`, or unavailable Frigg results. When shell finds relevant code, return to Frigg for `read_match`, `read_file`, symbols, or navigation when possible.

Example prompts:

- "Where is authentication bootstrapped?"
- "Show me implementations of `ProviderInterface`."
- "Who calls `handleWebhook`?"
- "Which files are relevant to the checkout flow?"

For agent-facing usage guidance, use [skills/frigg-first-code-search](skills/frigg-first-code-search/). For runtime diagnosis, use the [Frigg Operator Runbook](docs/operator-runbook.md).

### Agent evidence contracts (0.8)

0.8 strengthens the Frigg-first path agents use for code search:

- Multi-hypothesis probes: `search_batch` (concurrent merge of several search probes).
- Impact navigation: `impact_bundle` when one symbol needs defs/refs/calls together.
- Structured zero-hit / recovery fields and workspace freshness gates (dirty paths, path-scoped live-disk when needed).
- Citation-friendly reads: `presentation_mode=citation` on `read_file` / `read_match` (`LINE|content`).
- Local routing stats: `FRIGG_ROUTING_STATS=1`, then `frigg stats` (or MCP resource `frigg://stats/routing`).

Proof and optional policy:

- Skill: [skills/frigg-first-code-search](skills/frigg-first-code-search/)
- Test contracts: `cargo test -p frigg --all-targets`
- Release latency gate: CI runs the opt-in search-latency bench against local `rg`
- Optional harness templates: [policy-pack/frigg-harness](policy-pack/frigg-harness/)

## CLI reference

| Command | Purpose |
| --- | --- |
| `frigg` | Start the MCP server over stdio when no subcommand and no HTTP port are provided. |
| `frigg serve` | Start the shared MCP service over loopback HTTP by default. |
| `frigg adopt` | Add or remove managed Frigg entries in agent docs and MCP configs. |
| `frigg init` | Create or repair `.frigg/storage.sqlite3` without scanning source files. |
| `frigg index` | Scan files, refresh the local search index, and refresh semantic rows when semantic runtime is enabled. |
| `frigg index --changed` | Recheck files changed since the last index. |
| `frigg hash` | Print the stable CI cache fingerprint as `frigg-hash=<hex>`. |
| `frigg context` | Summarize context-efficiency JSONL logs when logging is enabled. |
| `frigg stats` | Show local opt-in routing stats when `FRIGG_ROUTING_STATS` is enabled on the MCP process. |

`frigg reindex` remains a compatibility alias for `frigg index`.

## MCP tool surface

Frigg exposes the `extended` MCP tool surface by default. Set `FRIGG_MCP_TOOL_SURFACE_PROFILE=core` to restrict the server to the stable core set.

Core tool groups:

- Workspace status and adoption: `workspace`.
- File listing: `list_files`.
- Source reads: `read_file`, `read_match`.
- Discovery: `search_text`, `search_hybrid`, `search_symbol`, `search_batch`.
- Navigation: `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`, `impact_bundle`.
- Structure: `document_symbols`, `inspect_syntax_tree`, `search_structural`.

Extended-only tools in default builds:

- `explore`

Feature-gated extended-only playbook tools are available only when Frigg is compiled with `--features playbook`:

- `playbook_run`
- `playbook_replay`
- `playbook_compose_citations`

Read tools default to text-first output. Request `presentation_mode=json` only when the caller needs structured fields such as path, byte ranges, or context-efficiency metadata. Use `presentation_mode=citation` for `LINE|content` lines when writing user-facing citations. Search and navigation tools default to compact responses; request `response_mode=full` when diagnostics or selection notes matter.

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
| `--watch-manifest-fast-concurrency` / `FRIGG_WATCH_MANIFEST_FAST_CONCURRENCY` | `1` | Maximum concurrent manifest-fast watch refreshes. |
| `--watch-semantic-followup-concurrency` / `FRIGG_WATCH_SEMANTIC_FOLLOWUP_CONCURRENCY` | `1` | Maximum concurrent semantic-followup watch refreshes. |
| `--lexical-backend` / `FRIGG_LEXICAL_BACKEND` | `auto` | Lexical backend: `auto`, `native`, or `ripgrep`. |
| `--ripgrep-executable` / `FRIGG_RIPGREP_EXECUTABLE` | PATH lookup | `rg` executable used by the ripgrep backend. |
| `FRIGG_MCP_TOOL_SURFACE_PROFILE` | `extended` | MCP surface profile: `extended` or `core`. |
| `FRIGG_SQLITE_BUSY_TIMEOUT_MS` | `30000` | SQLite wait timeout for transient writer contention. |
| `FRIGG_CONTEXT_EFFICIENCY_LOG` | `false` | Append compact context-efficiency rows to `.frigg/context.jsonl`. |
| `--semantic-runtime-enabled` / `FRIGG_SEMANTIC_RUNTIME_ENABLED` | `false` | Enable semantic indexing and recall. |
| `--semantic-runtime-provider` / `FRIGG_SEMANTIC_RUNTIME_PROVIDER` | `local` when semantic runtime is enabled | Semantic provider: `openai`, `google`, or `local`. |
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

The local provider uses `all-MiniLM-L6-v2` by default and does not require an API key. When `provider=local`, Frigg prepares missing local model artifacts automatically during startup. If local model preparation fails, startup fails with `local_model_prepare_failed`. Set `FRIGG_SEMANTIC_MODEL_CACHE` to choose the local model cache root. If `HF_HOME` is set and local model loading fails, unset `HF_HOME` so Frigg's cache selection controls the prepared artifacts.

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

Defaults: `local` -> `all-MiniLM-L6-v2`, `openai` -> `text-embedding-3-small`, `google` -> `gemini-embedding-001`.

| Provider | Default model | Credential |
| --- | --- | --- |
| `local` | `all-MiniLM-L6-v2` | none |
| `openai` | `text-embedding-3-small` | `OPENAI_API_KEY` |
| `google` | `gemini-embedding-001` | `GEMINI_API_KEY` |

After enabling semantic search for an existing repository, or after changing the semantic provider or model, run one semantic index pass with `frigg index`.

Zero-cloud semantic behavior applies only when `provider=local`.

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

Use `workspace` to adopt a repository and inspect runtime task state. CLI `frigg index` remains the explicit maintenance path when you want a repository refresh outside normal auto-readiness behavior.

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
  FRIGG_VERSION: 0.8.0
  FRIGG_INSTALL_DIR: ${{ github.workspace }}/.frigg-bin

jobs:
  frigg:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5

      - name: Install Frigg
        run: |
          curl -fsSL https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh \
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
  FRIGG_VERSION: 0.8.0
  FRIGG_INSTALL_DIR: ${{ github.workspace }}/.frigg-bin

jobs:
  frigg:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5

      - name: Install Frigg
        run: |
          curl -fsSL https://raw.githubusercontent.com/bnomei/frigg/main/scripts/install.sh \
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
- `workspace` may refresh lexical and semantic state under `.frigg/` and waits up to the attach timeout when adopting a target.
- Explicit maintenance remains available through CLI commands such as `frigg init` and `frigg index`.
- Optional OpenAI and Google semantic providers call their external embedding APIs. The local provider does not require an API key.
- Optional precise generators may execute repo-local or PATH-discovered tools and write `.frigg/scip/` artifacts. Inspect `workspace` runtime status or CLI output for generator progress and failures.
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
| [Dockerfile](Dockerfile) | Release-binary container image definition. |
| [npm/frigg](npm/frigg) | npm wrapper package for `npx @bnomei/frigg`. |
| [packaging](packaging) | Release packaging notes. |
| [showcases](showcases) | Public corpus of 52 repository question catalogs. |
| [docs/operator-runbook.md](docs/operator-runbook.md) | Runtime diagnosis and operator states. |
| [skills/frigg-first-code-search](skills/frigg-first-code-search/) | Agent-facing Frigg-first code search skill. |

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

1. For HTTP, confirm `frigg serve` is running. For stdio, confirm the MCP client launches `frigg` with the intended `--workspace-root`.
2. Call `workspace`.
3. If needed, call `workspace` with the repository path or repository id.
4. Check the `workspace` response for the session default and known repositories.

Search misses a recent change:

1. Call `workspace` with the repository path or id, or run `frigg index --changed`.
2. Inspect runtime task state only if the refresh did not fix the result.
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
- MCP stdio and HTTP dispatch: [crates/cli/src/cli_dispatch.rs](crates/cli/src/cli_dispatch.rs)
- MCP tool names and public state contracts: [crates/cli/src/mcp/types.rs](crates/cli/src/mcp/types.rs)
- HTTP serving rules: [crates/cli/src/http_runtime.rs](crates/cli/src/http_runtime.rs)
- Semantic provider defaults: [crates/cli/src/settings/semantic_runtime.rs](crates/cli/src/settings/semantic_runtime.rs)
- Workspace configuration defaults: [crates/cli/src/settings/frigg_config.rs](crates/cli/src/settings/frigg_config.rs)
- Installer behavior: [scripts/install.sh](scripts/install.sh)
- CI checks: [.github/workflows/ci.yml](.github/workflows/ci.yml)

## License

Frigg's crate metadata declares `MIT AND MPL-2.0`. The root [LICENSE](LICENSE) file contains the MIT license text. The MPL-2.0 text is bundled at [crates/cli/LICENSES/MPL-2.0.txt](crates/cli/LICENSES/MPL-2.0.txt).
