# Contracts Changelog

Deterministic reverse-chronological log for public contract and behavior changes.

## 2026-03-12

- spec: `123-busy-repo-semantic-storage-bounding`
- change_set: `busy-repo-semantic-storage-bounding.t004`
- summary: semantic storage now keeps one live corpus per `(repository, provider, model)` keyed by `semantic_head`, and steady-state semantic reads no longer fall back to older snapshot-partitioned corpora when the active model is missing coverage.
- summary: manifest snapshot retention is now bounded to the latest `8` by default while protecting any active semantic-head-covered snapshot, and provenance retention is bounded to the latest `10_000` events.
- summary: `embedding_vectors` is now treated as a derived sqlite-vec live projection over the active semantic corpus, and MCP/workspace health plus storage repair now surface and fix `semantic_vector_partition_out_of_sync` by rebuilding sqlite-vec from live semantic rows.

## 2026-03-11

- spec: `108-search-stage-attribution-and-corroborating-ranker`
- change_set: `search-stage-attribution-and-corroborating-ranker.t003`
- summary: `search_hybrid` benchmark and diagnostic surfaces now expose additive stage attribution for candidate intake, freshness validation, scan, witness scoring, graph expansion, semantic retrieval, anchor blending, document aggregation, and final diversification.
- summary: hybrid ranking now preserves strongest-anchor identity for returned matches, aggregates corroborating anchors at document level before scoring, and runs one final diversification pass after aggregation.

## 2026-03-10

- spec: `106-semantic-sqlite-vec-topk-and-lean-storage`
- change_set: `semantic-sqlite-vec-topk-and-lean-storage.t003`
- summary: storage hot-path benchmarks now include a deterministic sqlite-vec semantic top-k workload with one bounded payload batch lookup, while search benchmark docs explicitly keep semantic hybrid coverage scoped to disabled/degraded control paths.
- summary: storage and README contracts now describe `embedding_vectors` as a derived sqlite-vec projection over canonical semantic rows, including vector-only padding for shorter legacy embeddings and deterministic KNN `k` clamping at sqlite-vec's local limit.

- spec: `58-hybrid-graph-channel-and-path-unification`
- change_set: `hybrid-graph-channel-and-path-unification.t004`
- summary: search latency release artifacts now include a deterministic graph-backed `search_hybrid` workload (`search_latency/hybrid/graph-php-target-evidence`) that exercises bounded PHP target-evidence edges with semantic disabled.
- summary: benchmark methodology and budgets now distinguish graph-backed hybrid grounding from semantic-disabled/degraded paths, and the direct `search_latency/hybrid/*` budgets now reflect the heavier raw searcher hot path instead of MCP-layer tool timings.

- spec: `100-persistent-runtime-task-resource-surface`
- change_set: `persistent-runtime-task-resource-surface.t001`
- summary: `workspace_current` now exposes additive `repositories` and `runtime` status blocks, including explicit runtime profile, watch availability, active/recent runtime tasks, and recent provenance summaries.
- summary: Frigg now distinguishes `stdio_ephemeral`, `stdio_attached`, `http_loopback_service`, and `http_remote_service` in the public runtime status surface while preserving the existing stdio and attach-first compatibility flow.
- summary: persistent local-service guidance now treats loopback HTTP as the preferred warm profile and keeps non-loopback HTTP behind explicit remote-bind plus auth-token requirements.

## 2026-03-08

- spec: `67-php-core-name-resolution-and-target-evidence`
- change_set: `php-core-name-resolution-and-target-evidence.t001`
- summary: PHP indexing now records deterministic canonical class-like names, namespace modules, enum cases, and local type-hint evidence so `search_symbol` and read-only graph flows can resolve framework-heavy repositories more reliably.
- summary: deterministic PHP target evidence now captures attributes, `Foo::class`, `new Foo(...)`, callable literals, and cheap literal structure, while keeping the public MCP surface framework-neutral and free of Laravel-specific overlays.

- spec: `66-blade-livewire-flux-runtime-symbol-surface`
- change_set: `blade-livewire-flux-runtime-symbol-surface.t001`
- summary: `.blade.php` now participates as first-class `blade` language support across runtime symbol corpora, `search_symbol`, `document_symbols`, and `search_structural`.
- summary: Blade responses now expose bounded source-only metadata for template relations plus literal Livewire and Flux discovery, without claiming Laravel runtime overlays such as routes, providers, policies, validation, or Eloquent semantics.

- spec: `69-semantic-hit-count-retained-candidate-semantics`
- change_set: `semantic-hit-count-retained-candidate-semantics.t001`
- summary: `search_hybrid.metadata.semantic_candidate_count` now represents the broader raw semantic chunk candidate pool, while `metadata.semantic_hit_count` now reports retained semantic documents after query-relevance pruning and before final hybrid ranking.
- summary: semantic-ok warnings now distinguish between a healthy semantic channel with no retained query-relevant hits and a healthy channel whose retained semantic hits did not contribute to the returned top results.

- spec: `68-hybrid-exact-anchor-tail-ranking-and-concrete-witnesses`
- change_set: `hybrid-exact-anchor-tail-ranking-and-concrete-witnesses.t001`
- summary: `search_hybrid` now expands query-overlap tokens from exact anchors such as `build_pipeline_runner` and `ProviderInterface`, so mixed intent-plus-symbol queries keep stronger path and excerpt pressure beyond rank 1.
- summary: concrete runtime/example/test witness queries now apply stronger bounded overlap boosts for runtime/support/test paths, while weak-overlap generic docs and generic anchors like `server` or `discoverer` are damped more aggressively under witness intent.

- spec: `65-mcp-response-deduplication-and-optional-omission`
- change_set: `mcp-response-deduplication-and-optional-omission.t001`
- summary: live `search_hybrid` responses now treat structured `metadata` as the canonical semantic-diagnostics payload and omit the duplicated top-level semantic mirrors plus the legacy JSON-string `note`.
- summary: `workspace_attach` now keeps `storage` only at the top level while still returning repository `health`, avoiding duplicate storage blocks in attach responses.
- summary: optional MCP fields such as `artifact_count`, `reason`, compatibility snapshot ids, provider/model, and legacy semantic mirrors are now omitted when unset instead of serializing `null`.

- spec: `64-rust-entrypoint-build-flow-ranking-and-health-counts`
- change_set: `rust-entrypoint-build-flow-ranking-and-health-counts.t001`
- summary: `search_hybrid` now recognizes Rust entrypoint/build-flow queries such as “where the app starts and builds the pipeline runner”, promotes canonical entrypoints like `src/main.rs`, and prefers concrete build-anchor excerpts over nearby fake/mock helper witnesses.
- summary: workspace index `health.lexical.artifact_count` now reports manifest entry counts when a snapshot is known, and `health.semantic.artifact_count` now reports semantic embedding row counts for the active or fallback semantic snapshot when known.

- spec: `63-php-runtime-witness-intent-and-diversification`
- change_set: `php-runtime-witness-intent-and-diversification.t001`
- summary: `search_hybrid` now recognizes implementation-oriented runtime-witness queries such as `initialize`, `subscriptions`, `completion providers`, `handlers`, `transport`, and `resource updated`.
- summary: under that intent, `search_hybrid` keeps bounded token recall active even when phrase-level lexical matches already filled the requested top-k, so concrete runtime witnesses can still enter the candidate set.
- summary: under that intent, repeated generic docs and `composer.json` are downweighted while runtime/support/test/example witnesses receive bounded boosts and diversification pressure.

- spec: `62-language-support-kernel-and-shared-relation-hooks`
- change_set: `language-support-kernel-and-shared-relation-hooks.t001`
- summary: centralized Rust/PHP language alias parsing, extension matching, and tool-capability checks behind a shared internal language-support surface so source filters, `document_symbols`, and `search_structural` stay aligned.
- summary: shared PHP declaration-relation and heuristic implementation helper surfaces are now reused across search and MCP paths instead of maintaining parallel resolution logic.

- spec: `61-semantic-snapshot-coherence-after-failed-reindex`
- change_set: `semantic-snapshot-coherence-after-failed-reindex.t001`
- summary: failed semantic reindex no longer leaves a newer manifest snapshot active without matching semantic embeddings; Frigg now rolls back the just-created manifest snapshot before surfacing the semantic failure.
- summary: `search_hybrid` now falls back to the newest older manifest snapshot that still has semantic embeddings for the active provider/model when the latest manifest snapshot has none.
- summary: split manifest/semantic snapshot recovery now surfaces `semantic_status=degraded` with deterministic snapshot IDs in `semantic_reason` instead of silently reporting `ok` with zero semantic contribution.
- summary: older semantic fallback snapshots are now filtered against the latest manifest paths so removed files do not resurface from stale semantic storage.
- summary: `search_hybrid` now surfaces `semantic_status=unavailable` when semantic runtime is enabled but no semantic corpus exists for the active repository/provider/model combination, separating missing-index states from true degraded provider/fallback states.
- summary: `workspace_attach` and repository listings now keep `storage.index_state=uninitialized` for schema-only repo-local databases that have no manifest snapshot yet, while per-component `health` continues to report the more specific missing-index reason.

- spec: `59-semantic-transport-diagnostics-and-json-send-path`
- change_set: `semantic-transport-diagnostics-and-json-send-path.t001`
- summary: semantic embedding provider failures now include sanitized request diagnostics (`inputs`, `input_chars_total`, `body_bytes`, `body_blake3`, `trace_id`) instead of opaque upstream-only error text.
- summary: semantic reindex now wraps embedding failures with deterministic batch context (`batch_index`, `total_batches`, `batch_size`, first/last chunk anchors) so operators can isolate failing corpus slices quickly.
- summary: live embedding requests now use reqwest's native JSON send path while preserving the existing OpenAI/Google payload shapes validated by local transport tests.

- spec: `56-incoming-call-parity-and-symbol-discovery-filters`
- change_set: `incoming-call-parity-and-symbol-discovery-filters.t001`
- summary: `incoming_calls` precise occurrence fallback now emits `relation="calls"` for callable targets when the recovered source context is call-like, while preserving `refers_to` for non-call occurrences.
- summary: `search_symbol` now accepts additive `path_class` and `path_regex` filters, and same-rank discovery results prefer runtime code under `src/` before project/support paths.

- spec: `55-search-hybrid-semantic-contribution-observability`
- change_set: `search-hybrid-semantic-contribution-observability.t001`
- summary: `search_hybrid.semantic_enabled` now means semantic evidence actually contributed non-zero score to at least one returned match, not merely that the semantic channel executed successfully.
- summary: `search_hybrid` success responses now expose additive `semantic_hit_count` and `semantic_match_count` counters at the top level and in the legacy JSON-string `note`.
- summary: `search_hybrid.warning` can now appear for `semantic_status=ok` when semantic retrieval returned no hits or when semantic hits existed but none contributed to the returned top results.

- spec: `54-precise-target-pinning-and-hybrid-recall-recovery`
- change_set: `precise-target-pinning-and-hybrid-recall-recovery.t001`
- summary: symbol-targeted precise navigation now pins same-name precise candidates to the already-selected target anchor, so returned precise definitions/references no longer drift away from `note.target_selection`.
- summary: `search_hybrid` now keeps bounded lexical recall expansion active when the semantic channel is healthy but empty, preventing semantic-enabled empty result sets when lexical recovery can still find matches.

- spec: `53-navigation-correctness-wave2`
- change_set: `navigation-correctness-wave2.t001`
- summary: `find_implementations` now uses a second precise recovery path that derives implementation matches from precise occurrences plus enclosing-definition context when direct SCIP implementation edges are missing.
- summary: symbol-only navigation now ranks runtime code ahead of `benches/`, `examples/`, and `tests/` for ambiguous exact-name matches, and deterministic `note.target_selection` metadata now records the selected path class.
- summary: `outgoing_calls` now returns callable edges only, labels surviving occurrence-derived precise matches as `calls`, and preserves an empty result instead of widening to non-callable reference noise.

- spec: `52-navigation-disambiguation-and-hybrid-clarity`
- change_set: `navigation-disambiguation-and-hybrid-clarity.t001`
- summary: `find_references` now accepts symbol-or-location targeting in the public `v1` contract, prefers location resolution when `path`/`line` are supplied, and records the chosen resolver path in deterministic `note.resolution_source` metadata.
- summary: `outgoing_calls` now recovers precise occurrence-derived `refers_to` matches when explicit SCIP call relationships are absent, aligning precise fallback behavior with `incoming_calls` before heuristic degradation.
- summary: `search_hybrid` success responses now surface deterministic `warning` text for `disabled` and `degraded` semantic states at the top level and in the legacy JSON-string `note`, and the error/docs contract now clarifies how callers should interpret those states.

## 2026-03-06

- spec: `47-session-workspace-attach-and-stdio-defaults`
- change_set: `session-workspace-attach-and-stdio-defaults.t001`
- summary: added `workspace_attach` and `workspace_current` to the public MCP surface so HTTP sessions can start empty, attach repo-local workspaces on demand, and keep a session-default repository for omitted `repository_id` calls.
- summary: changed MCP serving startup/config behavior so utility commands still require explicit `--workspace-root` values, while serving mode may start with zero roots and stdio auto-attaches cwd or Git root as a one-shot session default.
- summary: changed stdio built-in watch defaults to `off`, kept HTTP defaults at `auto`, and synchronized config/tools/README contracts around the new attach-first workflow.

- spec: `35-semantic-runtime-mcp-surface`
- change_set: `semantic-runtime-mcp-surface.t010`
- summary: clarified `search_hybrid` and `search_symbol` runtime guidance across `tools/list`, server instructions, schema field descriptions, and public contract docs so first-time clients know to use `search_hybrid` for broad doc/runtime questions and pivot to `search_symbol` or scoped `search_text.path_regex` for concrete runtime anchors.

- spec: `36-deep-search-runtime-tools`
- change_set: `deep-search-runtime-tools.t002`
- summary: expanded the deep-search `v1` contract docs with machine-readable step tool schema references so first-time clients can resolve nested playbook step params and serialized trace evidence without inspecting Rust source.
- summary: documented the recommended raw-stdio startup profile for deep-search sessions (`FRIGG_MCP_TOOL_SURFACE_PROFILE=extended`, `RUST_LOG=error`, `--watch-mode off`) and repo-specific `list_repositories`-first authoring guidance in the public contract docs.

- spec: `35-semantic-runtime-mcp-surface`
- change_set: `semantic-runtime-mcp-surface.t009`
- summary: `search_hybrid` success responses now mirror semantic probe fields (`semantic_requested`, `semantic_enabled`, `semantic_status`, `semantic_reason`) at the top level while retaining the existing JSON-encoded `note` metadata for backward compatibility.
- summary: clarified the `v1` tool contract that wrapper `note` metadata remains JSON-encoded string payloads, and added schema/test coverage so `search_hybrid.note` cannot silently drift to object transport.

- spec: `46-stdio-startup-quiet-defaults`
- change_set: `stdio-startup-quiet-defaults.t001`
- summary: MCP stdio launches now default to tracing filter `error` when `RUST_LOG` is unset, keeping stderr quiet for raw clients while preserving explicit `RUST_LOG` overrides.
- summary: preserved existing `info` defaults for utility commands and HTTP serving so operator-facing startup diagnostics remain available outside the raw stdio MCP path.
- summary: documented the stdio quiet-default behavior and the `RUST_LOG=info|debug` override in the README.

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
