# Contracts Changelog

Deterministic reverse-chronological log for public contract and behavior changes.

## 2026-03-06

- spec: `44-integrated-local-watch-mode`
- change_set: `integrated-local-watch-mode.t001`
- summary: added built-in watch config (`watch.mode`, `watch.debounce_ms`, `watch.retry_ms`) with deterministic CLI/env wiring and local-first activation defaults (`auto` enables stdio and loopback HTTP only).
- summary: introduced a built-in watcher supervisor over the existing changed-only reindex path with per-root debounce, one in-flight background refresh across the process, and retry-on-failure scheduling.
- summary: documented built-in versus external watcher usage so multi-repo fan-out and editor-owned scheduling remain explicit external-tool cases.

- spec: `45-watch-driven-changed-reindex-correctness`
- change_set: `watch-driven-changed-reindex-correctness.t001`
- summary: watch-triggered startup now reuses the existing changed-only reindex pipeline to bootstrap missing manifests while preserving valid latest snapshots without forced startup refresh.
- summary: added regression coverage for watcher scheduler behavior, ignored internal paths, startup initial-sync behavior, and fresh-manifest startup no-op behavior.
- summary: synchronized config and README/operator guidance with the latest-successful-snapshot serving model used during background watch-triggered refreshes.

- spec: `40-symbol-resolution-indexes`
- change_set: `symbol-resolution-indexes.t001`
- summary: added warm-corpus symbol indexes for stable ids, exact names, lowercase names, and per-path symbol lists so `search_symbol` and navigation target resolution narrow exact/prefix work before any infix scan.
- summary: changed location-based navigation targeting to resolve within path-local symbol indexes (including same-line disambiguation with `column`) while preserving deterministic tie-break ordering.
- summary: added regression coverage for indexed symbol ranking, manifest-backed corpus refresh, and location-based navigation target resolution.

- spec: `41-partial-precise-degradation`
- change_set: `partial-precise-degradation.t001`
- summary: mixed-success SCIP ingest now retains successful precise graph data instead of clearing repository-wide precise state, and cached precise metadata records explicit `coverage` (`full|partial|none`).
- summary: read-only navigation/reference notes now distinguish `precision=precise_partial` from heuristic fallback, and partial precise absence reports `precise_absence_reason=precise_partial_non_authoritative_absence`.
- summary: updated regression coverage so oversized sibling SCIP artifacts no longer suppress retained precise matches, while partial empty-lookups still fall back heuristically.

- spec: `42-manifest-freshness-validation`
- change_set: `manifest-freshness-validation.t001`
- summary: persisted manifest fast paths now validate snapshot entry metadata (`path`, `size_bytes`, `mtime_ns`) against the active filesystem before reusing symbol corpora or manifest-backed search candidate sets.
- summary: stale manifest snapshots now deterministically rebuild live metadata and avoid reusing stale in-memory corpus/search state after edited-in-place files diverge from the persisted snapshot.
- summary: synchronized manifest-backed search and MCP symbol-corpus regression coverage around edited-in-place stale snapshot invalidation.

- spec: `43-document-symbols-byte-guards`
- change_set: `document-symbols-byte-guards.t001`
- summary: `document_symbols` now enforces `max_file_bytes` before materializing full source contents and returns typed `invalid_params` metadata for over-budget files.
- summary: preserved existing Rust/PHP-only tree-sitter outline behavior for in-budget files while eliminating avoidable whole-file allocation spikes on oversized inputs.
- summary: updated public error/tool docs and regression coverage for deterministic over-budget `document_symbols` failures.

- spec: `35-semantic-runtime-mcp-surface`
- change_set: `semantic-runtime-mcp-surface.t008`
- summary: semantic runtime config now applies provider-specific default embedding models when `semantic_runtime.model` is omitted (`openai` -> `text-embedding-3-small`, `google` -> `gemini-embedding-001`).
- summary: synchronized semantic/config contract docs and startup/config tests so explicit blank model values remain invalid while provider defaults keep semantic enablement ergonomic.

- spec: `39-performance-memory-hardening`
- change_set: `performance-memory-hardening.t001`
- summary: replaced manifest digest whole-file reads with buffered streaming and changed `read_file` line-range reads to stream only requested lines, eliminating full-buffer hot paths for large files.
- summary: changed text and hybrid search candidate discovery to prefer persisted manifest snapshots when they still resolve under the active workspace root, with deterministic fallback to live repository walks for stale `.frigg` manifests.
- summary: added precise graph secondary indexes plus cached SCIP discovery reuse, and reduced semantic retrieval/update memory pressure via lean embedding projections, late chunk-text hydration, and changed-only semantic snapshot advancement.

## 2026-03-05

- spec: `24-find-references-resource-budgets`
- change_set: `find-references-resource-budgets.t001`
- summary: raised `FriggConfig.max_file_bytes` default from `512 * 1024` to `2 * 1024 * 1024` to provide higher out-of-the-box resource ceilings for large repositories.
- summary: added deterministic startup override wiring for `max_file_bytes` via CLI (`--max-file-bytes`) and env (`FRIGG_MAX_FILE_BYTES`) across both MCP serving and utility commands (`init`, `verify`, `reindex`).
- summary: synchronized configuration contract and startup tests to lock the new default and override behavior.

- spec: `37-public-surface-parity-gates`
- change_set: `public-surface-parity-gates.t005`
- summary: added runtime tool-surface profile selection via env (`FRIGG_MCP_TOOL_SURFACE_PROFILE=core|extended`) so live MCP sessions can verify core vs extended `tools/list` deltas without code changes.
- summary: updated server instructions and parity tests to report/validate the active runtime profile deterministically, including deep-search runtime tool exposure when `extended` is selected.
- summary: synchronized README/tools-contract docs with the explicit runtime profile toggle and deep-search enablement path.

- spec: `34-readonly-ide-navigation-tools`
- change_set: `readonly-ide-navigation-tools.t003`
- summary: strengthened `find_implementations` heuristic fallback to derive Rust `impl Trait for Type` and `impl Type` matches directly from symbol extraction when precise SCIP artifacts are absent.
- summary: improved fallback precision metadata so `implementation_count` reflects actual heuristic match count instead of a hardcoded zero.
- summary: added regression coverage for deterministic heuristic implementation fallback behavior and metadata.

- spec: `34-readonly-ide-navigation-tools`
- change_set: `readonly-ide-navigation-tools.t004`
- summary: enriched `find_references` note metadata with deterministic `target_selection` anchors/counters so ambiguous symbol-only queries remain auditable (`candidate_count`, `same_rank_candidate_count`, `ambiguous_query`).
- summary: added deterministic `precise_absence_reason` in `find_references` heuristic fallback notes to distinguish missing SCIP artifacts, ingest failures, and symbol-level precise misses.
- summary: added regression coverage for ambiguous symbol-name target selection metadata.

- spec: `34-readonly-ide-navigation-tools`
- change_set: `readonly-ide-navigation-tools.t005`
- summary: added deterministic SCIP discovery diagnostics to precise navigation notes (`candidate_directories`, `discovered_artifacts`, `failed_artifacts`) so `precise_absence_reason=no_scip_artifacts_discovered` is directly actionable at runtime.
- summary: extended precise ingest bookkeeping to capture sampled discovery paths and sampled artifact read/ingest failures without changing fallback semantics.
- summary: added regression coverage asserting discovery metadata for both discovered and undiscovered SCIP artifact scenarios.

- spec: `34-readonly-ide-navigation-tools`
- change_set: `readonly-ide-navigation-tools.t006`
- summary: extended precise artifact discovery to include both `.json` and binary `.scip` files under `.frigg/scip`, eliminating `.json`-only discovery blind spots.
- summary: added native protobuf SCIP ingest path in `SymbolGraph` with typed decode diagnostics and deterministic mapping into existing precise symbol/occurrence/relationship records.
- summary: added regression coverage for `.scip` artifact discovery and precise reference resolution from binary SCIP payloads.

- spec: `34-readonly-ide-navigation-tools`
- change_set: `readonly-ide-navigation-tools.t007`
- summary: changed precise-only SCIP budget exceed handling to deterministic heuristic degradation (with `note.precise.failed_artifacts` diagnostics) instead of hard-failing read-only navigation calls.
- summary: retained typed timeout behavior for source-file heuristic budget overages while decoupling precise artifact budget pressure from overall tool availability.
- summary: updated regression coverage to assert oversized SCIP artifacts now produce usable heuristic fallback responses with explicit budget-failure metadata.

- spec: `35-semantic-runtime-mcp-surface`
- change_set: `semantic-runtime-mcp-surface.t007`
- summary: improved `search_hybrid` degraded/disabled semantic behavior by adding deterministic bounded token-regex lexical recall expansion for multi-token natural-language queries.
- summary: dampened lexical frequency dominance via deterministic saturation + path-quality weighting so source-code evidence is less likely to be overshadowed by playbook/doc self-reference in lexical-only fallback mode.
- summary: added regression coverage for lexical recall expansion and source-over-playbook ordering in hybrid ranking tests.

- spec: `22-tool-path-semantics-unification`
- change_set: `tool-path-semantics-unification.t002`
- summary: extended `read_file` params with optional line-range slicing (`line_start`, `line_end`) using one-based inclusive semantics for targeted inspection workflows.
- summary: for sliced reads, `max_bytes` is now enforced on returned slice content so large files can be inspected without all-or-nothing size failures.
- summary: added typed `invalid_params` range validation and regression coverage for read-file line slicing behavior.

- spec: `38-single-crate-consolidation`
- change_set: `single-crate-consolidation.t010`
- summary: removed sqlite fallback backend semantics from storage readiness; sqlite-vec is now the only accepted vector backend path for initialize/verify/startup gates.
- summary: added explicit sqlite-vec FFI auto-extension registration at SQLite connection bootstrap so `vec_version()` readiness checks run deterministically in-process.
- summary: updated smoke/contracts docs to treat fallback-style `embedding_vectors` schemas as legacy migration errors that require operator reset/reinit.

- spec: `35-semantic-runtime-mcp-surface`
- change_set: `semantic-runtime-mcp-surface.t006`
- summary: added deterministic semantic runtime latency workload coverage for hybrid semantic toggle/degraded execution paths in both `search_latency` and `mcp_tool_latency` Criterion harnesses.
- summary: synchronized benchmark budget contracts and methodology docs for new workload IDs (`search_latency/hybrid/*`, `mcp_tool_latency/search_hybrid/*`) with explicit p50/p95/p99 acceptance targets.
- summary: refreshed benchmark rollout/status documentation (`benchmarks/latest-report.md`, overview readiness addendum) to include semantic runtime benchmarking facts in release-readiness artifacts.

- spec: `37-public-surface-parity-gates`
- change_set: `public-surface-parity-gates.t004`
- summary: reconciled tool-surface parity drift by aligning `docs/overview.md` profile-marked runtime tool list with the active `core` manifest to include `search_hybrid`.
- summary: clarified runtime profile semantics in overview docs to reflect the current read-only surface cardinality (`core=13`, `extended=16`) and the feature-gated deep-search additions.
- summary: verified contract/docs parity gates now converge runtime manifests, schema set, and docs profile sections with deterministic diagnostics.

- spec: `35-semantic-runtime-mcp-surface`
- change_set: `semantic-runtime-mcp-surface.t001`
- summary: introduced a typed `FriggConfig.semantic_runtime` contract (`enabled`, `provider`, `model`, `strict_mode`) with deterministic validation and explicit provider enum support (`openai|google`).
- summary: wired semantic runtime composition inputs into CLI startup with explicit CLI/env mappings and fail-fast semantic startup validation before serving when semantic mode is enabled.
- summary: documented deterministic semantic startup credential/error behavior (`OPENAI_API_KEY`/`GEMINI_API_KEY`, `invalid_params` startup failure code) in semantic and config contracts.

- spec: `36-deep-search-runtime-tools`
- change_set: `deep-search-runtime-tools.t001`
- summary: added `v1` schema contracts for `deep_search_run`, `deep_search_replay`, and `deep_search_compose_citations`.
- summary: introduced typed MCP wrapper params/responses for deep-search runtime tooling, including typed playbook/trace/citation payload contracts and replay-output wrappers.
- summary: expanded schema parity coverage and schema-doc presence checks to include the three feature-gated deep-search read-only tool contracts.

- spec: `34-readonly-ide-navigation-tools`
- change_set: `readonly-ide-navigation-tools.t002`
- summary: finalized docs sync for the public runtime `v1` tool surface to enumerate all twelve read-only tools in `docs/overview.md` and `contracts/tools/v1/README.md`.
- summary: documented deterministic behavior/limit guarantees for the seven new IDE navigation tools (precise-first fallback metadata, canonical path semantics, limit clamping, Rust/PHP-only `document_symbols` and `search_structural`, and structural query max-length guard).
- summary: synchronized readiness/benchmark narrative with expanded MCP workloads and current deterministic report summary (`pass=30 fail=0 missing=0`).

- spec: `34-readonly-ide-navigation-tools`
- change_set: `readonly-ide-navigation-tools.t001`
- summary: added `v1` schema contracts for seven new read-only IDE navigation tools: `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, `outgoing_calls`, `document_symbols`, and `search_structural`.
- summary: added MCP wrapper param/response types and schema parity coverage for the seven new read-only navigation tools.
- summary: expanded read-only tool surface contract enumeration to include all twelve public `v1` read-only tools and keep schema-file presence deterministic.

- spec: `33-regex-trigram-bitmap-acceleration`
- change_set: `regex-trigram-bitmap-acceleration.t002`
- summary: expanded `search_latency` regex workload coverage with deterministic sparse-hit and no-hit required-literal cases to exercise trigram/bitmap prefilter acceleration paths.
- summary: synchronized search benchmark methodology and `budgets.v1.json` workload contracts for the new regex sparse/no-hit acceleration workloads.
- summary: updated overview readiness language to reflect Slice 4 regex trigram/bitmap acceleration implementation + benchmark coverage status.

- spec: `32-sqlite-vec-production-hardening`
- change_set: `sqlite-vec-production-hardening.t002`
- summary: CLI startup path now enforces strict vector readiness before serving MCP and aborts when vector readiness fails or active backend is `sqlite_fallback`.
- summary: operational smoke checks now include deterministic startup-gate validation against a seeded fallback-vector fixture and assert typed startup failure summaries.
- summary: updated storage/overview contracts to mark sqlite-vec startup hardening as implemented and remove it from open readiness items.

- spec: `31-write-surface-security-gates`
- change_set: `write-surface-security-gates.t001`
- summary: documented canonical future write-surface confirmation policy markers (`confirm`, `confirmation_required`, no side effects without explicit confirmation) in contracts and security docs while keeping the public tool surface read-only.
- summary: added release-readiness checklist/gate requirements that enforce write-surface policy marker alignment across `contracts/errors.md`, `contracts/tools/v1/README.md`, and `docs/security/threat-model.md`.

## 2026-03-04

- spec: `28-storage-error-trace-diff-corrections`
- change_set: `storage-error-trace-diff-corrections.t001`
- summary: storage vector-dimension validation now maps `expected_dimensions <= 0` to typed `invalid_params` (`FriggError::InvalidInput`) instead of `internal`.
- summary: deep-search trace replay diff now validates trace step-vector structural consistency (`step_count` vs `steps.len()`) before step-wise zip comparison and emits deterministic mismatch diagnostics.
- summary: updated `contracts/storage.md` failure taxonomy to distinguish invalid dimension input from runtime vector readiness failures.

- spec: `22-tool-path-semantics-unification`
- change_set: `tool-path-semantics-unification.t001`
- summary: unified `read_file`, `search_text`, `search_symbol`, and `find_references` path semantics to repository-relative canonical paths with `/` separators.
- summary: `read_file` now returns canonical repository-relative `path` values even when called with an absolute in-workspace path (input compatibility preserved).
- summary: added MCP regression coverage for canonical `read_file` response paths and documented the shared v1 path contract in `contracts/tools/v1/README.md`.

- spec: `27-doc-contract-sync-wave2`
- change_set: `doc-contract-sync-wave2.t001`
- summary: aligned `contracts/semantic.md` with implemented embeddings runtime behavior (caller-owned provider/model wiring, concrete retry policy, and typed failure shapes).
- summary: aligned `contracts/storage.md` with implemented storage/runtime behavior (vector backend fallback semantics, verify probes, and reindex-vs-provenance responsibilities).
- summary: synchronized benchmark methodology docs with active workload set and budgets, including search low-limit/high-cardinality, MCP precise/provenance, and indexer reindex latency workloads.
- summary: refreshed `specs/06-embeddings-and-vector-store` + `docs/overview.md` language to remove stale config-key assumptions and reflect current contract status.

- spec: `21-vector-backend-migration-safety`
- change_set: `vector-backend-migration-safety.t001`
- summary: storage vector readiness now uses deterministic schema-first backend selection to keep existing fallback stores on `sqlite_fallback` even if sqlite-vec becomes available later.
- summary: sqlite-vec-backed stores now fail with a deterministic migration-safety error when sqlite-vec is unavailable instead of implicitly transitioning to fallback.
- summary: documented vector backend transition safety semantics in `contracts/storage.md`.

- spec: `20-reindex-resilience-diagnostics`
- change_set: `reindex-resilience-diagnostics.t001`
- summary: `reindex_repository` now continues across unreadable files by emitting typed deterministic manifest diagnostics (`walk`/`read`) instead of hard-failing the reindex operation.
- summary: CLI `reindex` output now includes per-repository and aggregate deterministic diagnostics counts (`diagnostics_total`, `diagnostics_walk`, `diagnostics_read`) plus ordered diagnostic lines.
- summary: documented non-fatal reindex file read/traversal diagnostic semantics in `contracts/storage.md`.

- spec: `13-contract-and-doc-drift-closure`
- change_set: `contract-and-doc-drift-closure.initial`
- summary: added `contracts/changelog.md` and release-readiness gate enforcement for this artifact.
- summary: aligned `contracts/config.md` to implemented `FriggConfig` keys only.
- summary: documented positional `repository_id` stability semantics for `list_repositories`.
- summary: marked deep-search playbook harness as internal/test-only (not a public MCP tool surface).
