# Changelog

## Unreleased

- Trust-preserving composers: `search_batch` now exposes fixed
  `merge_strategy=reciprocal_rank_fusion`, `merge_algorithm_version`, per-row evidence,
  consensus, RRF, and derived strength instead of implying cross-kind raw-score comparison.
  Its opaque continuation binds the normalized probes, scope, snapshots, and merge version.
  The historical `merge="rank_by_probe_hit_strength"` input remains accepted only for two minor
  releases, normalizes to RRF, and returns a compatibility note; the canonical schema no longer
  presents merge as a choice.

- Fail-closed impact composition: `impact_bundle` now resolves copied `target_ref` values once,
  reports legacy same-rank ambiguity without running children, and exposes section execution,
  trust, completeness, and section-qualified proof targets. Test mentions are explicit via
  `include_test_mentions=true`; outgoing calls remain outside the bundle. These additive fields
  preserve direct symbol compatibility and do not require persisted migration or backfill.

- Stable result targets: search and navigation rows now add optional `target_ref`; navigation
  parameters add optional `target`; and `impact_bundle` accepts exactly one of `target` or legacy
  non-empty `symbol`. Result-match targets are session/source scoped, stable-symbol targets are
  repository/corpus scoped, and target-mode impact resolves the supplied target once without
  reranking it. Existing direct symbol/location clients remain compatible.

- Typed executable follow-ups: `next_actions` is authoritative and names existing MCP tools with
  exact `arguments`; hosts choose role/order/dependencies and retain authorization. Compact and
  full modes carry identical actions. Stale/mixed proof retries use typed origins and fresh match
  ids. `suggested_next` remains a deprecated lossy projection for at least two minor releases.

- Exact-search completeness rollout: bounded MCP collections now expose typed `completeness`
  (`unit`, page-local `returned`, exact-or-absent `total`, complete/truncated state, typed
  reasons, and canonical v2 `continuation`); valid legacy `resume_from` remains accepted during
  its compatibility window. Documented request/snapshot-bound continuation recovery, raw
  occurrence `search_text.total_matches` versus shaped row totals, hybrid ranked-discovery
  honesty, per-probe/per-section propagation, and the corrected pre-page active-mode meaning of
  `find_references.total_matches`.
- Hygiene (EXP-when-to-grow-surface / EXP-split-mega-modules): skill surface-growth filter (skill → internal → thin composer → new tool last); operator runbook contributor rules for opportunistic module splits (no split-for-LOC, one registration manifest); `mcp/server.rs` module-doc pointer to that policy.
- Hybrid graph channel honesty (EXP-nav-hybrid-graph-channel): per-match `graph_mode`, ranking_note when graph contributes (“ranking signal not nav call edges”), full channel `pipeline: hybrid_ephemeral`; skill/operator dual-pipeline docs. Hybrid graph remains separate from MCP navigation.
- Google Gemini embeddings positioned as **credential_peer** (use when `GEMINI_API_KEY` is already present; not an unmeasured preferred-quality cloud default) in catalog, README, operator runbook, and skill.
- Local MiniLM positioned as **offline_smoke** (zero-key general embedder) in catalog, README, operator runbook, and skill; semantic document embeddings for **all** providers now include a compact `path` + `language` envelope (stored excerpt body stays pure source; run a **full** `frigg index` after upgrade so partitions do not mix envelopes).
- Semantic provider `openai_compat`: OpenAI-protocol embeddings against a configurable full POST URL (`--semantic-runtime-openai-compat-endpoint` / `FRIGG_SEMANTIC_RUNTIME_OPENAI_COMPAT_ENDPOINT`), Bearer via `FRIGG_OPENAI_COMPAT_API_KEY` (falls back to `OPENAI_API_KEY`), distinct storage partition from `openai`. Catalog/preset `openai-compat-selfhost` on `frigg://policy/semantic-models.json`.
- Hybrid compact `ranking_note` signals `lexical_only (semantic not contributing)` when semantic is off or empty (product default remains semantic-off; no readiness metadata dump in compact).
- MCP policy resource `frigg://policy/semantic-models.json`: curated embedding-model catalog (defaults, **real** `native_dimensions` only, pad flag, offline, credentials; `quality_scores: curated` — no retained public leaderboard) plus soft intent presets (`offline-small`, `cloud-openai`, `cloud-google`, `openai-compat-selfhost`) that expand to provider+model (not CLI aliases). Docs: why sqlite-vec pads short vectors to projection width.
- Skill + operator runbook: probe-on-spawn for Task/subagent Frigg registration; classify `Tool not found: frigg__*` as harness inheritance (FUT-003), not ranking/product P0.
- Soft PreToolUse `HOOK_NUDGE` teaches preferred Frigg next steps (`search_text` / `search_batch` / …) while remaining soft-only (no `permissionDecision` deny; no strict mode).
- Adopt: best-effort `--skill-provider {claude,codex,cursor,copilot}` copies `frigg-first-code-search` only when the host parent skills directory already exists (never creates `…/skills`).
- Adopt: dropped `gemini-md` target; kept `copilot` for CI and expanded host notes (install triangle + Cursor MCP/hooks guidance).

## 0.8.0 - 2026-07-09

- Promoted a scenario-first `frigg-first-code-search` skill and lightweight default AGENTS adopt; use `frigg adopt --target agents-md --policy expanded` for a compact in-repo routing policy.
- Added core MCP tools `search_batch` (concurrent multi-probe merge) and `impact_bundle` (combined impact navigation).
- Added structured zero-hit / recovery fields, workspace gate actions (dirty + path-scoped live-disk), and scoped handle STALE/MIXED failures on `read_match`.
- Added `presentation_mode=citation` on `read_file` / `read_match` for `LINE|content` citation output.
- Added local opt-in routing stats via `FRIGG_ROUTING_STATS` and `frigg stats` (MCP resource `frigg://stats/routing`; no cloud telemetry).
- Added competitive search-latency proof boards (`cargo test -p frigg --test futura_bench`; release gate `cargo futura-bench`): warm `search_text` p95 ≤ local `rg` p95 × 1.5 noise budget (debug soft only).
- Added optional harness policy templates under `policy-pack/frigg-harness/`.
- Hardened batch probe scope/handles, go_to recovery, result-handle detach invalidation, precise spawn re-queue, and corpus re-read path containment.
- Deferred (documented): live `EvidencePacket` tool, HTTP tools/call suite, large-repo SLO, hot-reindex lag p95, `FriggUnavailable` emit path.

## 0.6.3 - 2026-07-05

- Hardened MCP search, navigation, SCIP ingest, and adopt paths against symlink and workspace-boundary escapes.
- Fixed workspace attach/index edge cases, including Git-root validation, default attach rejection for non-Git parents, and stale precise-generation lifecycle reporting.
- Improved precise-generation failure diagnostics, including spawn failures and generator argument handling.
- Refined `search_text` guidance and compatibility by accepting the legacy `pattern` alias while documenting `query`.
- Polished CLI/TUI badge and intro-color rendering.

## 0.6.2 - 2026-07-05

- Fixed the release build process for Intel macOS assets and Docker image publishing.
- Refreshed the release packaging defaults and install examples for `v0.6.2`.

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
