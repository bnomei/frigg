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
- `deterministic contracts`: versioned tool schemas and error taxonomy in `docs/contracts/`.

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
- There is no built-in watch mode yet. For local auto-reindex, use an external watcher such as `watchexec`.

Example watcher:
```bash
watchexec -w crates -w docs -w README.md -e rs,toml,md -- just reindex-changed .
```

`watchexec` is usually not the bottleneck here; `reindex --changed` still scans the workspace to rebuild the manifest, and semantic indexing can dominate runtime when enabled.

### 3) Run as MCP server

Stdio transport (default):
```bash
just run
# or: cargo run -p frigg --
```

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
- env `FRIGG_MCP_TOOL_SURFACE_PROFILE` (`core` default, `extended` enables deep-search runtime tools)

## MCP Tool Surface (v1)

Public MCP tools:
- `list_repositories`
- `read_file`
- `search_text`
- `search_symbol`
- `find_references`

Optional deep-search tools (when `FRIGG_MCP_TOOL_SURFACE_PROFILE=extended`):
- `deep_search_run`
- `deep_search_replay`
- `deep_search_compose_citations`

Schema files:
- `docs/contracts/tools/v1/list_repositories.v1.schema.json`
- `docs/contracts/tools/v1/read_file.v1.schema.json`
- `docs/contracts/tools/v1/search_text.v1.schema.json`
- `docs/contracts/tools/v1/search_symbol.v1.schema.json`
- `docs/contracts/tools/v1/find_references.v1.schema.json`

Contract notes:
- these are the canonical `v1` public tools,
- paths in responses are canonical repository-relative paths,
- breaking schema changes require a new major version directory.

## Public Contracts

- Tool schemas/versioning: `docs/contracts/tools/v1/README.md`
- Config contract: `docs/contracts/config.md`
- Error taxonomy: `docs/contracts/errors.md`
- Storage contract: `docs/contracts/storage.md`
- Semantic embeddings contract: `docs/contracts/semantic.md`
- Contract changelog: `docs/contracts/changelog.md`

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

Benchmark docs and targets live in `docs/benchmarks/`.

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
