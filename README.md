# frigg: local Sourcegraph-style MCP (not just `rg`)

Local-first MCP server for deterministic code search and code navigation across one or more workspace roots.

Frigg is:
- CLI-first: one deployable binary (`frigg`) built from a Rust workspace.
- MCP-first: a small, explicit tool surface with versioned JSON schemas.
- Local-first: it indexes local repositories into SQLite-backed artifacts.

## What Frigg Does

Frigg helps AI agents and developer tools answer code questions with reproducible, source-backed results:

1. discover repositories (`list_repositories`),
2. read files safely inside allowed roots (`read_file`),
3. search text and symbols (`search_text`, `search_symbol`),
4. navigate references (`find_references`),
5. persist provenance/events for replay and auditing.

## Core Concepts

- `workspace roots`: local directories Frigg is allowed to index/read.
- `repository IDs`: runtime IDs (`repo-001`, `repo-002`, ...) derived from workspace-root order.
- `snapshots + file manifests`: persisted index state used for deterministic reindex behavior.
- `provenance events`: stored tool-call evidence for replay/debugging.
- `deterministic contracts`: versioned tool schemas and error taxonomy in `contracts/`.

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
- `--changed` rebuilds the current manifest, diffs it against the latest persisted snapshot, and treats `added + modified` files as changed.
- Deleted files are tracked separately.
- If nothing changed and a prior manifest exists, Frigg reuses the previous `snapshot_id` instead of writing a new one.
- Built-in watch mode now exists for local MCP runs. With `--watch-mode auto` (the default), Frigg starts a background changed-only watcher for stdio and loopback HTTP, but keeps it disabled for non-loopback HTTP.
- If the latest manifest is missing or stale at startup, built-in watch mode queues one immediate changed-only refresh before waiting for new filesystem events.
- External watchers are still useful for multi-repo fan-out, editor-owned lifecycle, or when you want reindex scheduling outside the Frigg process.
- When Frigg serves MCP over stdio and `RUST_LOG` is unset, it defaults tracing to `error` so raw clients do not need special stderr-drain handling. Set `RUST_LOG=info` if you want startup/watch logs.

Built-in watch options:
```bash
# defaults shown explicitly:
cargo run -p frigg -- --watch-mode auto --watch-debounce-ms 750 --watch-retry-ms 5000

# disable built-in watch mode for the current run:
cargo run -p frigg -- --watch-mode off
```

External watcher example:
```bash
watchexec -w crates -w docs -w README.md -e rs,toml,md -- just reindex-changed .
```

If you pair an external watcher with a running Frigg server, disable the built-in watcher with `--watch-mode off` to avoid double scheduling.

`watchexec` is usually not the bottleneck here; `reindex --changed` still scans the workspace to rebuild the manifest, and semantic indexing can dominate runtime when enabled.

### 3) Run as MCP server

Stdio transport (default):
```bash
just run
# or: cargo run -p frigg --
```

Notes:
- When `RUST_LOG` is unset, stdio MCP launches default to an `error` tracing filter so raw clients do not need to drain routine startup/watch logs from stderr.
- Set `RUST_LOG=info` or `RUST_LOG=debug` if you want startup and watch diagnostics over stdio.
- Pass `--watch-mode off` if you want a stdio session with no built-in changed-only reindex scheduling.

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

### 4) Optional: enable semantic retrieval

Semantic runtime is disabled by default.

Why:
- enabling it without a configured provider, model, and API key would make startup fail,
- semantic indexing/search can issue external provider calls and add latency/cost,
- Frigg should still start and work in lexical/graph-only mode with zero provider setup.

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

Enable and populate embeddings for an existing workspace:
```bash
just reindex .
just run
```

Equivalent direct commands:
```bash
frigg reindex --workspace-root .
frigg --workspace-root .
```

Important:
- run a full `reindex` once when turning semantic runtime on for an already indexed workspace,
- do not use `reindex --changed` for the first semantic backfill if nothing changed on disk,
- after the first semantic population, `reindex --changed` is fine for incremental updates.

If semantic runtime is enabled correctly and embeddings exist, `search_hybrid` can return non-zero semantic scores.
If startup succeeds but `search_hybrid` still reports `semantic_status=disabled`, the running Frigg process is not seeing the semantic runtime env/config you expect.

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
- env `FRIGG_MCP_TOOL_SURFACE_PROFILE` (`core` default, `extended` enables deep-search runtime tools)

## MCP Tool Surface (v1)

Public MCP tools:
- `list_repositories`
- `read_file`
- `search_text`
- `search_hybrid`
- `search_symbol`
- `find_references`

Noise-control tip:
- `search_text` searches normal repository files broadly. When you only want docs/runtime evidence, add `path_regex`, for example `^(README\.md|crates/cli/src/.*)$`.
- `search_hybrid` is the broad natural-language entrypoint for mixed doc/runtime questions. Expect contracts, README, runtime, and tests to coexist in top hits; when you need concrete runtime anchors, follow with `search_symbol` or scoped `search_text`.

Optional deep-search tools (when `FRIGG_MCP_TOOL_SURFACE_PROFILE=extended`):
- `deep_search_run`
- `deep_search_replay`
- `deep_search_compose_citations`

Schema files:
- `contracts/tools/v1/list_repositories.v1.schema.json`
- `contracts/tools/v1/read_file.v1.schema.json`
- `contracts/tools/v1/search_text.v1.schema.json`
- `contracts/tools/v1/search_symbol.v1.schema.json`
- `contracts/tools/v1/find_references.v1.schema.json`

Contract notes:
- these are the canonical `v1` public tools,
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

You ship one artifact (`frigg`), not one artifact per workspace crate.
