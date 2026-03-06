# Design — 38-single-crate-consolidation

## Scope
- Cargo/workspace manifests:
  - `/Users/bnomei/Sites/frigg/Cargo.toml`
  - `/Users/bnomei/Sites/frigg/crates/*/Cargo.toml`
- Runtime code currently split across:
  - `/Users/bnomei/Sites/frigg/crates/core/`
  - `/Users/bnomei/Sites/frigg/crates/config/`
  - `/Users/bnomei/Sites/frigg/crates/graph/`
  - `/Users/bnomei/Sites/frigg/crates/storage/`
  - `/Users/bnomei/Sites/frigg/crates/embeddings/`
  - `/Users/bnomei/Sites/frigg/crates/index/`
  - `/Users/bnomei/Sites/frigg/crates/search/`
  - `/Users/bnomei/Sites/frigg/crates/mcp/`
  - `/Users/bnomei/Sites/frigg/crates/cli/`
  - `/Users/bnomei/Sites/frigg/crates/testkit/`
- Operational surfaces:
  - `/Users/bnomei/Sites/frigg/Justfile`
  - `/Users/bnomei/Sites/frigg/scripts/`
  - `/Users/bnomei/Sites/frigg/docs/`

## Normative excerpt (in-repo)
- Current artifact model is already one binary (`frigg`), while implementation is split across internal crates.
- Current release/security/benchmark checks include package-targeted commands (`cargo test -p ...`, `cargo bench -p ...`) and crate-path assumptions in scripts/docs.
- Publishability concerns are currently driven by internal crate graph and crate-name collisions on crates.io.

## Current dependency DAG (internal crates)
- `domain` -> none
- `settings` -> `domain`
- `graph` -> `domain`
- `storage` -> `domain`, `settings`
- `embeddings` -> `storage`
- `indexer` -> `domain`, `embeddings`, `graph`, `settings`, `storage`
- `searcher` -> `domain`, `embeddings`, `indexer`, `settings`, `storage`
- `mcp` -> `domain`, `embeddings`, `graph`, `indexer`, `searcher`, `settings`, `storage`
- `frigg` -> `indexer`, `mcp`, `settings`, `storage`

## Consolidation approach
1. Keep `frigg` package as the single target crate and migrate code into its module tree in DAG order.
2. Use temporary compatibility shims (module re-exports) during transition to keep code compiling between moves.
3. Rewrite imports/tests/benches/scripts to monolith module paths.
4. Remove obsolete internal crates and path dependencies only after all validations pass.

## Target structure (single crate)
- `crates/cli/src/lib.rs` (new) exposing modules:
  - `pub mod domain;`
  - `pub mod settings;`
  - `pub mod graph;`
  - `pub mod storage;`
  - `pub mod embeddings;`
  - `pub mod indexer;`
  - `pub mod searcher;`
  - `pub mod mcp;`
  - `pub mod test_support;` (from `testkit`, test-only or gated)
- `crates/cli/src/main.rs` remains binary entrypoint and uses internal modules instead of external workspace crates.

## Risk controls
- Merge order follows DAG to avoid cyclical breakage.
- Validate after each merge unit with `cargo check --workspace` (or `cargo check -p frigg` once single package is active).
- Keep benchmark binaries and integration tests runnable during migration.
- Update path-sensitive test fixtures (`CARGO_MANIFEST_DIR` assumptions) before deleting old crate folders.

## Rollout phases
1. Baseline and guardrails.
2. Foundational module migration (`domain/settings/graph/storage`).
3. Retrieval stack migration (`embeddings/indexer/searcher`).
4. MCP + CLI integration migration.
5. Tooling/docs command migration.
6. Manifest cleanup and publish dry-run verification.
