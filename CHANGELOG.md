# Changelog

## Unreleased

## 0.6.0 - 2026-07-04

- Added a canonical Frigg-first directive and aligned the README, MCP guidance, and bundled skill guidance around it.
- Added a GitHub Release installer with SHA-256 verification and install-cache CI support.
- Added `frigg adopt` for managed agent docs, MCP config entries, and Claude PreToolUse hook setup.
- Added local semantic embeddings with optional FastEmbed support and automatic local model preparation.
- Added context-efficiency telemetry for MCP search/read surfaces and a `frigg context` summary command.
- Renamed `reindex` / `workspace_reindex` to `index` / `workspace_index`; `frigg reindex` remains as a CLI compatibility alias.
- Improved hybrid search exact pivots, compact context savings, and freshness metadata across MCP search/navigation responses.
- Improved runtime performance with bounded MCP caches, semantic provider reuse, incremental semantic refreshes, tuned SQLite index passes, and watch refresh backoff/concurrency controls.
- Standardized CLI/TUI output modes, quiet/verbose behavior, timing details, tool-call completion events, terminal progress, and watch/semantic refresh status reporting.
- Hardened workspace write/path handling, manifest snapshot validation, read-only MCP workspace tools, leaner workspace responses, and text-mode reads.
- Renamed the internal trace MCP tools to feature-gated `playbook_*` tools so they are not exposed in default builds.
- Removed legacy Cursor `.cursorrules` adoption support, the manual `prepare-semantic-model` command, and SQLite provenance event storage.
- Raised the Rust MSRV, upgraded `rmcp`, updated dependencies, and trimmed unused feature/dependency surface.

## 0.5.0 - 2026-06-29

- Hardened MCP workspace adoption, path containment, provenance, workload-corpus redaction, and HTTP auth handling to close security and data-leak edge cases.
- Improved index, watch, semantic refresh, precise-generation, and cache lifecycle handling across detach/reattach races, stale snapshots, unreadable files, active tasks, and startup recovery.
- Fixed user-facing tool contracts for `find_references` totals, bounded line-window reads, hybrid lexical-only metadata, playbook regression exit status, and stale index lifecycle reporting.

## 0.4.7 - 2026-06-18

- Fixed semantic reindex recovery for stale deleted manifest paths from moved or removed workspace roots, preventing full rebuilds from failing on missing source path canonicalization and forcing safe full semantic rebuilds when changed-only deletes cannot be mapped to the current workspace.

## 0.4.6 - 2026-06-17

- Added a Frigg CI scorecard workflow and release artifact smoke checks to improve release confidence.
- Added the operator runbook and updated README guidance for workspace adoption, safety boundaries, and precise-generator side effects.
- Added semantic provider redaction regression tests for OpenAI and Google transport diagnostics.
- Documented and tested the `workspace_attach` side-effect contract, including `index_mode=skip` behavior and `wait_for_precise=false` precise-generation scheduling.
- Added a precise-generator diagnostics scorecard with discovery state, failure classes, recommended actions, duration, artifact count/byte metrics, and repo-local touch-risk reporting.

## 0.4.5 - 2026-06-07

- Fixed compact MCP responses for navigation, symbol search, `document_symbols`, `inspect_syntax_tree`, and `search_structural` to omit absent `metadata` and `note` fields instead of serializing them as `null`, keeping payloads compatible with the object-only `metadata` output schema used by strict clients.

## 0.4.4 - 2026-06-05

- Changed `workspace_attach` to default to `index_mode=ensure`: stale or missing lexical/semantic indexed state is refreshed and waited on for up to 30s before returning. Use `index_mode=skip` for lightweight adoption without attach-time indexing, or `index_mode=defer` to start recovery and return quickly. Attach/current responses now include `index_lifecycle`.
- Changed default for `wait_for_precise` on `workspace_attach` and `workspace_reindex` from `false` to `true` (now waits up to 30s for precise generation by default, matching the `waited_for_completion` behavior previously only available when explicitly passing `true`). Use `wait_for_precise=false` to restore the previous fast/non-blocking return. Updated docs and README.

## 0.4.2 - 2026-05-27

- Fixed semantic reindex recovery for deleted files recorded with absolute paths when indexing from a relative workspace root, allowing stale semantic chunks to be skipped or cleaned instead of failing on path canonicalization.
- Updated dependencies.

## 0.4.1 - 2026-05-24

- Fixed scoped MCP search to use runtime repository IDs for manifest and semantic storage lookups while preserving stable public repository IDs in responses.
- Fixed adjacent symbol/navigation manifest lookup, hybrid exact-pivot repository scoping, and root generated SCIP artifact watch churn.

## 0.4.0 - 2026-05-18

- Updated dependencies and removed the unused `gix` workspace dependency.

## 0.3.2 - 2026-04-17

- Upgraded dependencies

## 0.2.2 - 2026-04-17

- Replaced permissive `outputSchema.properties.metadata` boolean schemas with explicit object schemas for the affected MCP navigation and symbol-search tools, improving compatibility with strict clients such as Cursor.

## 0.2.1

- Upgraded `rmcp` from `1.2.0` to `1.4.0`.

## 0.2.0 - 2026-03-23

- Upgraded `rmcp` from `1.1.0` to `1.2.0`.
- Verified the `frigg` crate builds and its package test suite passes against `rmcp 1.2.0`.
- Restored bounded-SCIP coverage in max-file-bytes tool-handler tests by disabling `full_scip_ingest` in the test helper that exercises budgeted paths.
- Updated the `document_symbols` unsupported-extension expectation to include `.java`, matching the current language registry.
