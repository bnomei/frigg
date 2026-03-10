use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::domain::FriggError;
use crate::domain::model::{ReferenceMatch, SymbolMatch};
use crate::graph::{
    PreciseRelationshipKind, RelationKind, ScipIngestError, ScipResourceBudgets, SymbolGraph,
};
use crate::indexer::{
    FLUX_REGISTRY_VERSION, FileMetadataDigest, HeuristicReferenceConfidence,
    HeuristicReferenceEvidence, HeuristicReferenceResolver, ManifestBuilder,
    ManifestDiagnosticKind, ReindexMode, SourceSpan, SymbolDefinition, SymbolExtractionOutput,
    extract_blade_source_evidence_from_source, extract_php_source_evidence_from_source,
    extract_symbols_for_paths, extract_symbols_from_source, mark_local_flux_overlays,
    navigation_symbol_target_rank, php_declaration_relation_edges_for_file,
    php_heuristic_implementation_candidates_for_target, register_symbol_definitions,
    reindex_repository_with_runtime_config, resolve_blade_relation_evidence_edges,
    resolve_php_target_evidence_edges, search_structural_in_source,
    semantic_chunk_language_for_path,
};
use crate::language_support::{
    HeuristicImplementationStrategy, LanguageCapability, SymbolLanguage,
    heuristic_implementation_strategy, parse_supported_language, supported_language_for_path,
};
use crate::manifest_validation::validate_manifest_digests_for_root;
use crate::path_class::{repository_path_class, repository_path_class_rank};
use crate::searcher::{
    HybridChannelWeights, SearchDiagnosticKind, SearchFilters, SearchHybridQuery, SearchTextQuery,
    TextSearcher, compile_safe_regex,
};
use crate::settings::FriggConfig;
use crate::settings::SemanticRuntimeCredentials;
use crate::storage::{Storage, ensure_provenance_db_parent_dir, resolve_provenance_db_path};
use protobuf::Enum;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::transport::{
    StreamableHttpServerConfig, StreamableHttpService,
    streamable_http_server::session::local::LocalSessionManager,
};
use rmcp::{ErrorData, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use scip::types::symbol_information::Kind as ScipSymbolKind;
use serde_json::{Value, json};
use tokio::task;
use tracing::warn;

use crate::mcp::deep_search::{
    DeepSearchHarness, DeepSearchPlaybook, DeepSearchTraceArtifact, DeepSearchTraceOutcome,
};
use crate::mcp::explorer::{
    DEFAULT_CONTEXT_LINES, DEFAULT_MAX_MATCHES, ExploreMatcher, ExploreScopeRequest,
    LossyLineSliceError, MAX_CONTEXT_LINES, line_window_around_anchor, read_line_slice_lossy,
    scan_file_scope_lossy, validate_anchor, validate_cursor,
};
use crate::mcp::provenance_cache::{ProvenancePersistenceStage, ProvenanceStorageCacheKey};
use crate::mcp::server_state::{
    CachedPreciseGraph, DeterministicSignatureHasher, ExploreExecution, FindReferencesExecution,
    FindReferencesResourceBudgets, NavigationToolExecution, PreciseArtifactFailureSample,
    PreciseCoverageMode, PreciseGraphCacheKey, PreciseIngestStats, RankedSymbolMatch,
    ReadFileExecution, RepositoryDiagnosticsSummary, RepositorySymbolCorpus,
    ResolvedNavigationTarget, ResolvedSymbolTarget, ScipArtifactDigest, ScipArtifactDiscovery,
    ScipArtifactFormat, ScipCandidateDirectoryDigest, SearchHybridExecution, SearchSymbolExecution,
    SearchTextExecution, SymbolCandidate, SymbolCorpusCacheKey,
};
use crate::mcp::tool_surface::{
    TOOL_SURFACE_PROFILE_ENV, ToolSurfaceParityDiff, ToolSurfaceProfile,
    active_runtime_tool_surface_profile, diff_runtime_against_profile_manifest,
};
use crate::mcp::types::{
    CallHierarchyMatch, DeepSearchComposeCitationsParams, DeepSearchComposeCitationsResponse,
    DeepSearchReplayParams, DeepSearchReplayResponse, DeepSearchRunParams, DeepSearchRunResponse,
    DocumentSymbolsParams, DocumentSymbolsResponse, ExploreMatch, ExploreMetadata,
    ExploreOperation, ExploreParams, ExploreResponse, ExploreWindow, FindDeclarationsParams,
    FindDeclarationsResponse, FindImplementationsParams, FindImplementationsResponse,
    FindReferencesParams, FindReferencesResponse, GoToDefinitionParams, GoToDefinitionResponse,
    ImplementationMatch, IncomingCallsParams, IncomingCallsResponse, ListRepositoriesParams,
    ListRepositoriesResponse, NavigationLocation, OutgoingCallsParams, OutgoingCallsResponse,
    ReadFileParams, ReadFileResponse, RepositorySummary, SearchHybridChannelWeightsParams,
    SearchHybridMatch, SearchHybridParams, SearchHybridResponse, SearchPatternType,
    SearchStructuralParams, SearchStructuralResponse, SearchSymbolParams, SearchSymbolPathClass,
    SearchSymbolResponse, SearchTextParams, SearchTextResponse, WorkspaceAttachParams,
    WorkspaceAttachResponse, WorkspaceCurrentParams, WorkspaceCurrentResponse,
    WorkspaceIndexComponentState, WorkspaceIndexComponentSummary, WorkspaceIndexHealthSummary,
    WorkspaceResolveMode, WorkspaceStorageIndexState, WorkspaceStorageSummary,
};
use crate::mcp::workspace_registry::{AttachedWorkspace, WorkspaceRegistry};

pub type FriggMcpService = StreamableHttpService<FriggMcpServer, LocalSessionManager>;

#[derive(Clone)]
pub struct FriggMcpServer {
    config: Arc<FriggConfig>,
    tool_router: ToolRouter<Self>,
    workspace_registry: Arc<RwLock<WorkspaceRegistry>>,
    session_default_repository_id: Arc<RwLock<Option<String>>>,
    symbol_corpus_cache: Arc<RwLock<BTreeMap<SymbolCorpusCacheKey, Arc<RepositorySymbolCorpus>>>>,
    precise_graph_cache: Arc<RwLock<BTreeMap<PreciseGraphCacheKey, Arc<CachedPreciseGraph>>>>,
    latest_precise_graph_cache: Arc<RwLock<BTreeMap<String, Arc<CachedPreciseGraph>>>>,
    provenance_storage_cache: Arc<RwLock<BTreeMap<ProvenanceStorageCacheKey, Arc<Storage>>>>,
    provenance_best_effort: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceSemanticRefreshPlan {
    latest_snapshot_id: String,
    compatible_snapshot_id: String,
    reason: &'static str,
}

impl FriggMcpServer {
    const EXTENDED_ONLY_TOOL_NAMES: [&str; 4] = [
        "explore",
        "deep_search_compose_citations",
        "deep_search_replay",
        "deep_search_run",
    ];
    const PROVENANCE_MAX_TEXT_CHARS: usize = 512;
    const PROVENANCE_BEST_EFFORT_ENV: &str = "FRIGG_MCP_PROVENANCE_BEST_EFFORT";
    const FIND_REFERENCES_MAX_SCIP_ARTIFACTS: usize = 2_048;
    const FIND_REFERENCES_MAX_SOURCE_FILES: usize = 20_000;
    const FIND_REFERENCES_SCIP_ARTIFACT_BYTES_MULTIPLIER: usize = 8;
    const FIND_REFERENCES_SOURCE_FILE_BYTES_MULTIPLIER: usize = 4;
    const FIND_REFERENCES_TOTAL_BYTES_MULTIPLIER: usize = 128;
    const FIND_REFERENCES_SCIP_MAX_ELAPSED_MS: u64 = 5_000;
    const FIND_REFERENCES_SOURCE_MAX_ELAPSED_MS: u64 = 5_000;
    const FIND_REFERENCES_MIN_SCIP_DOCUMENT_BUDGET: usize = 1_024;
    const FIND_REFERENCES_DOCUMENT_BUDGET_MULTIPLIER: usize = 512;
    const PRECISE_FAILURE_SAMPLE_LIMIT: usize = 8;
    const PRECISE_DISCOVERY_SAMPLE_LIMIT: usize = 16;
    const SEARCH_STRUCTURAL_MAX_QUERY_CHARS: usize = 4_096;

    fn filtered_tool_router(enable_extended_tools: bool) -> ToolRouter<Self> {
        let mut router = Self::tool_router();
        if !enable_extended_tools {
            for tool_name in Self::EXTENDED_ONLY_TOOL_NAMES {
                router.remove_route(tool_name);
            }
        }
        router
    }

    fn with_error_metadata(error_code: &str, retryable: bool, detail: Option<Value>) -> Value {
        let mut payload = serde_json::Map::new();
        payload.insert(
            "error_code".to_owned(),
            Value::String(error_code.to_owned()),
        );
        payload.insert("retryable".to_owned(), Value::Bool(retryable));

        if let Some(detail) = detail {
            match detail {
                Value::Object(detail_map) => {
                    for (key, value) in detail_map {
                        payload.insert(key, value);
                    }
                }
                other => {
                    payload.insert("detail".to_owned(), other);
                }
            }
        }

        Value::Object(payload)
    }

    fn invalid_params(message: impl Into<String>, detail: Option<Value>) -> ErrorData {
        ErrorData::invalid_params(
            message.into(),
            Some(Self::with_error_metadata("invalid_params", false, detail)),
        )
    }

    fn resource_not_found(message: impl Into<String>, detail: Option<Value>) -> ErrorData {
        ErrorData::resource_not_found(
            message.into(),
            Some(Self::with_error_metadata(
                "resource_not_found",
                false,
                detail,
            )),
        )
    }

    fn access_denied(message: impl Into<String>, detail: Option<Value>) -> ErrorData {
        ErrorData::invalid_request(
            message.into(),
            Some(Self::with_error_metadata("access_denied", false, detail)),
        )
    }

    fn internal_with_code(
        message: impl Into<String>,
        error_code: &str,
        retryable: bool,
        detail: Option<Value>,
    ) -> ErrorData {
        ErrorData::internal_error(
            message.into(),
            Some(Self::with_error_metadata(error_code, retryable, detail)),
        )
    }

    fn internal(message: impl Into<String>, detail: Option<Value>) -> ErrorData {
        Self::internal_with_code(message, "internal", false, detail)
    }

    fn timeout(message: impl Into<String>, detail: Option<Value>) -> ErrorData {
        Self::internal_with_code(message, "timeout", true, detail)
    }

    fn usize_to_u64(value: usize) -> u64 {
        u64::try_from(value).unwrap_or(u64::MAX)
    }

    fn find_references_resource_budgets(&self) -> FindReferencesResourceBudgets {
        let source_max_file_bytes = self
            .config
            .max_file_bytes
            .saturating_mul(Self::FIND_REFERENCES_SOURCE_FILE_BYTES_MULTIPLIER)
            .max(self.config.max_file_bytes);
        let scip_max_artifact_bytes = self
            .config
            .max_file_bytes
            .saturating_mul(Self::FIND_REFERENCES_SCIP_ARTIFACT_BYTES_MULTIPLIER)
            .max(self.config.max_file_bytes);
        let source_max_total_bytes = source_max_file_bytes
            .saturating_mul(Self::FIND_REFERENCES_TOTAL_BYTES_MULTIPLIER)
            .max(source_max_file_bytes);
        let scip_max_total_bytes = scip_max_artifact_bytes
            .saturating_mul(Self::FIND_REFERENCES_TOTAL_BYTES_MULTIPLIER)
            .max(scip_max_artifact_bytes);
        let scip_max_documents_per_artifact = self
            .config
            .max_search_results
            .saturating_mul(Self::FIND_REFERENCES_DOCUMENT_BUDGET_MULTIPLIER)
            .max(Self::FIND_REFERENCES_MIN_SCIP_DOCUMENT_BUDGET);

        FindReferencesResourceBudgets {
            scip_max_artifacts: Self::FIND_REFERENCES_MAX_SCIP_ARTIFACTS,
            scip_max_artifact_bytes,
            scip_max_total_bytes,
            scip_max_documents_per_artifact,
            scip_max_elapsed_ms: Self::FIND_REFERENCES_SCIP_MAX_ELAPSED_MS,
            source_max_files: Self::FIND_REFERENCES_MAX_SOURCE_FILES,
            source_max_file_bytes,
            source_max_total_bytes,
            source_max_elapsed_ms: Self::FIND_REFERENCES_SOURCE_MAX_ELAPSED_MS,
        }
    }

    fn find_references_budget_metadata(budgets: FindReferencesResourceBudgets) -> Value {
        json!({
            "scip": {
                "max_artifacts": budgets.scip_max_artifacts,
                "max_artifact_bytes": budgets.scip_max_artifact_bytes,
                "max_total_bytes": budgets.scip_max_total_bytes,
                "max_documents_per_artifact": budgets.scip_max_documents_per_artifact,
                "max_elapsed_ms": budgets.scip_max_elapsed_ms,
            },
            "source": {
                "max_files": budgets.source_max_files,
                "max_file_bytes": budgets.source_max_file_bytes,
                "max_total_bytes": budgets.source_max_total_bytes,
                "max_elapsed_ms": budgets.source_max_elapsed_ms,
            },
        })
    }

    fn find_references_resource_budget_error(
        budget_scope: &str,
        budget_code: &str,
        message: impl Into<String>,
        detail: Value,
    ) -> ErrorData {
        let mut detail = match detail {
            Value::Object(object) => object,
            other => {
                let mut object = serde_json::Map::new();
                object.insert("detail".to_owned(), other);
                object
            }
        };
        detail.insert(
            "tool_name".to_owned(),
            Value::String("find_references".to_owned()),
        );
        detail.insert(
            "budget_scope".to_owned(),
            Value::String(budget_scope.to_owned()),
        );
        detail.insert(
            "budget_code".to_owned(),
            Value::String(budget_code.to_owned()),
        );

        Self::timeout(message, Some(Value::Object(detail)))
    }

    fn provenance_persistence_error(
        stage: ProvenancePersistenceStage,
        tool_name: &str,
        repository_id: Option<&str>,
        db_path: Option<&Path>,
        err: impl std::fmt::Display,
    ) -> ErrorData {
        let mut detail = serde_json::Map::new();
        detail.insert(
            "provenance_stage".to_owned(),
            Value::String(stage.as_str().to_owned()),
        );
        detail.insert("tool_name".to_owned(), Value::String(tool_name.to_owned()));
        if let Some(repository_id) = repository_id {
            detail.insert(
                "repository_id".to_owned(),
                Value::String(repository_id.to_owned()),
            );
        }
        if let Some(db_path) = db_path {
            detail.insert(
                "db_path".to_owned(),
                Value::String(db_path.display().to_string()),
            );
        }

        let raw_message = err.to_string();
        detail.insert(
            "provenance_error".to_owned(),
            Value::String(Self::bounded_text(&raw_message)),
        );

        Self::internal_with_code(
            format!("failed to persist provenance for tool {tool_name}"),
            "provenance_persistence_failed",
            stage.retryable(),
            Some(Value::Object(detail)),
        )
    }

    fn parse_env_flag(raw: &str) -> bool {
        let normalized = raw.trim().to_ascii_lowercase();
        matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
    }

    fn provenance_best_effort_from_env() -> bool {
        std::env::var(Self::PROVENANCE_BEST_EFFORT_ENV)
            .map(|raw| Self::parse_env_flag(&raw))
            .unwrap_or(false)
    }

    fn map_frigg_error(err: FriggError) -> ErrorData {
        match err {
            FriggError::InvalidInput(message) => Self::invalid_params(message, None),
            FriggError::NotFound(message) => Self::resource_not_found(message, None),
            FriggError::AccessDenied(message) => Self::access_denied(message, None),
            FriggError::Io(err) => Self::internal(err.to_string(), None),
            FriggError::Internal(message)
                if message.starts_with("semantic_status=strict_failure:") =>
            {
                let reason = message
                    .split_once(':')
                    .map(|(_, reason)| reason.trim())
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or("strict semantic channel failure");
                Self::internal_with_code(
                    format!("semantic channel strict failure: {reason}"),
                    "unavailable",
                    true,
                    Some(json!({
                        "semantic_status": "strict_failure",
                        "semantic_reason": Self::bounded_text(reason),
                    })),
                )
            }
            FriggError::Internal(message) => Self::internal(message, None),
        }
    }

    fn deep_search_budget_metadata_from_trace(trace: &DeepSearchTraceArtifact) -> Value {
        let mut resource_budgets = Vec::new();
        let mut resource_usage = Vec::new();

        for step in &trace.steps {
            let DeepSearchTraceOutcome::Ok { response_json } = &step.outcome else {
                continue;
            };
            let Ok(response) = serde_json::from_str::<Value>(response_json) else {
                continue;
            };
            let Some(note_raw) = response.get("note").and_then(Value::as_str) else {
                continue;
            };
            let Ok(note) = serde_json::from_str::<Value>(note_raw) else {
                continue;
            };
            let Some(note) = note.as_object() else {
                continue;
            };

            if let Some(step_budgets) = note.get("resource_budgets").cloned() {
                resource_budgets.push(json!({
                    "step_id": step.step_id,
                    "tool_name": step.tool_name,
                    "resource_budgets": step_budgets,
                }));
            }
            if let Some(step_usage) = note.get("resource_usage").cloned() {
                resource_usage.push(json!({
                    "step_id": step.step_id,
                    "tool_name": step.tool_name,
                    "resource_usage": step_usage,
                }));
            }
        }

        json!({
            "resource_budgets": resource_budgets,
            "resource_usage": resource_usage,
        })
    }

    async fn run_blocking_task<T, F>(operation: &'static str, task_fn: F) -> Result<T, ErrorData>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        task::spawn_blocking(task_fn).await.map_err(|err| {
            Self::internal(
                format!("blocking task join failure in {operation}: {err}"),
                Some(json!({
                    "operation": operation,
                    "join_error": Self::bounded_text(&err.to_string()),
                })),
            )
        })
    }

    fn relative_display_path(root: &Path, path: &Path) -> String {
        let normalized = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        normalized.trim_start_matches("./").to_owned()
    }

    fn symbol_name_match_rank(symbol_name: &str, query: &str, query_lower: &str) -> Option<u8> {
        if symbol_name == query {
            return Some(0);
        }

        let symbol_lower = symbol_name.to_ascii_lowercase();
        if symbol_lower == query_lower {
            return Some(1);
        }
        if symbol_lower.starts_with(query_lower) {
            return Some(2);
        }
        if symbol_lower.contains(query_lower) {
            return Some(3);
        }

        None
    }

    fn push_symbol_candidate(
        candidates: &mut Vec<SymbolCandidate>,
        corpus: &RepositorySymbolCorpus,
        symbol_index: usize,
        rank: u8,
    ) {
        let symbol = corpus.symbols[symbol_index].clone();
        let relative_path = Self::relative_display_path(&corpus.root, &symbol.path);
        let path_class = Self::navigation_path_class(&relative_path);
        candidates.push(SymbolCandidate {
            rank,
            path_class_rank: Self::navigation_path_class_rank(path_class),
            path_class,
            repository_id: corpus.repository_id.clone(),
            root: corpus.root.clone(),
            symbol,
        });
    }

    fn build_ranked_symbol_match(
        corpus: &RepositorySymbolCorpus,
        symbol_index: usize,
        rank: u8,
        path_class_filter: Option<SearchSymbolPathClass>,
        path_regex: Option<&regex::Regex>,
    ) -> Option<RankedSymbolMatch> {
        let symbol = &corpus.symbols[symbol_index];
        let path = Self::relative_display_path(&corpus.root, &symbol.path);
        if let Some(path_class_filter) = path_class_filter {
            if Self::navigation_path_class(&path) != path_class_filter.as_str() {
                return None;
            }
        }
        if let Some(path_regex) = path_regex {
            if !path_regex.is_match(&path) {
                return None;
            }
        }
        let path_class = Self::navigation_path_class(&path);
        Some(RankedSymbolMatch {
            rank,
            path_class_rank: Self::navigation_path_class_rank(path_class),
            matched: SymbolMatch {
                repository_id: corpus.repository_id.clone(),
                symbol: symbol.name.clone(),
                kind: symbol.kind.as_str().to_owned(),
                path,
                line: symbol.line,
            },
        })
    }

    fn sort_ranked_symbol_matches(ranked_matches: &mut [RankedSymbolMatch]) {
        ranked_matches.sort_by(|left, right| {
            left.rank
                .cmp(&right.rank)
                .then(left.path_class_rank.cmp(&right.path_class_rank))
                .then(left.matched.repository_id.cmp(&right.matched.repository_id))
                .then(left.matched.path.cmp(&right.matched.path))
                .then(left.matched.line.cmp(&right.matched.line))
                .then(left.matched.kind.cmp(&right.matched.kind))
                .then(left.matched.symbol.cmp(&right.matched.symbol))
        });
    }

    fn dedup_ranked_symbol_matches(ranked_matches: &mut Vec<RankedSymbolMatch>) {
        ranked_matches.dedup_by(|left, right| {
            left.matched.repository_id == right.matched.repository_id
                && left.matched.path == right.matched.path
                && left.matched.line == right.matched.line
                && left.matched.kind == right.matched.kind
                && left.matched.symbol == right.matched.symbol
        });
    }

    fn retain_bounded_ranked_symbol_match(
        ranked_matches: &mut Vec<RankedSymbolMatch>,
        limit: usize,
        candidate: RankedSymbolMatch,
    ) {
        if limit == 0 {
            return;
        }

        ranked_matches.push(candidate);
        Self::sort_ranked_symbol_matches(ranked_matches);
        if ranked_matches.len() > limit {
            ranked_matches.pop();
        }
    }

    fn resolve_navigation_symbol_target(
        corpora: &[Arc<RepositorySymbolCorpus>],
        symbol_query: &str,
        repository_id_hint: Option<&str>,
    ) -> Result<ResolvedSymbolTarget, ErrorData> {
        // Deterministic precedence: stable-id exact > name exact > case-insensitive name, then
        // repository/path/line/stable-id tie-breakers.
        let mut candidates = Vec::new();
        let query_lower = symbol_query.to_ascii_lowercase();
        let query_looks_canonical = symbol_query.contains('\\')
            || symbol_query.contains("::")
            || symbol_query.contains('$');
        for corpus in corpora {
            if let Some(symbol_index) = corpus.symbol_index_by_stable_id.get(symbol_query) {
                Self::push_symbol_candidate(&mut candidates, corpus, *symbol_index, 0);
            }
            if query_looks_canonical {
                if let Some(symbol_indices) =
                    corpus.symbol_indices_by_canonical_name.get(symbol_query)
                {
                    for symbol_index in symbol_indices {
                        Self::push_symbol_candidate(&mut candidates, corpus, *symbol_index, 1);
                    }
                }
                if let Some(symbol_indices) = corpus
                    .symbol_indices_by_lower_canonical_name
                    .get(&query_lower)
                {
                    for symbol_index in symbol_indices {
                        let Some(canonical_name) = corpus
                            .canonical_symbol_name_by_stable_id
                            .get(corpus.symbols[*symbol_index].stable_id.as_str())
                        else {
                            continue;
                        };
                        if canonical_name != symbol_query {
                            Self::push_symbol_candidate(&mut candidates, corpus, *symbol_index, 2);
                        }
                    }
                }
            }
            let name_rank_offset = if query_looks_canonical { 3 } else { 1 };
            if let Some(symbol_indices) = corpus.symbol_indices_by_name.get(symbol_query) {
                for symbol_index in symbol_indices {
                    let symbol = &corpus.symbols[*symbol_index];
                    if navigation_symbol_target_rank(symbol, symbol_query) == Some(1) {
                        Self::push_symbol_candidate(
                            &mut candidates,
                            corpus,
                            *symbol_index,
                            name_rank_offset,
                        );
                    }
                }
            }
            if let Some(symbol_indices) = corpus.symbol_indices_by_lower_name.get(&query_lower) {
                for symbol_index in symbol_indices {
                    let symbol = &corpus.symbols[*symbol_index];
                    if navigation_symbol_target_rank(symbol, symbol_query) == Some(2) {
                        Self::push_symbol_candidate(
                            &mut candidates,
                            corpus,
                            *symbol_index,
                            name_rank_offset + 1,
                        );
                    }
                }
            }
        }

        candidates.sort_by(|left, right| {
            left.rank
                .cmp(&right.rank)
                .then(left.path_class_rank.cmp(&right.path_class_rank))
                .then(left.repository_id.cmp(&right.repository_id))
                .then(left.symbol.path.cmp(&right.symbol.path))
                .then(left.symbol.line.cmp(&right.symbol.line))
                .then(left.symbol.stable_id.cmp(&right.symbol.stable_id))
        });
        let candidate_count = candidates.len();
        let candidate = candidates.first().cloned().ok_or_else(|| {
            Self::resource_not_found(
                "symbol not found",
                Some(json!({
                    "symbol": symbol_query,
                    "repository_id": repository_id_hint,
                })),
            )
        })?;
        let corpus = corpora
            .iter()
            .find(|corpus| corpus.repository_id == candidate.repository_id)
            .cloned()
            .ok_or_else(|| {
                Self::internal(
                    "target symbol repository was not present in corpus set",
                    Some(json!({
                        "repository_id": candidate.repository_id.clone(),
                        "symbol_id": candidate.symbol.stable_id.clone(),
                    })),
                )
            })?;
        let selected_rank_candidate_count = candidates
            .iter()
            .take_while(|resolved| resolved.rank == candidate.rank)
            .count();

        Ok(ResolvedSymbolTarget {
            candidate,
            corpus,
            candidate_count,
            selected_rank_candidate_count,
        })
    }

    fn navigation_path_class(relative_path: &str) -> &'static str {
        repository_path_class(relative_path)
    }

    fn navigation_path_class_rank(path_class: &str) -> u8 {
        repository_path_class_rank(path_class)
    }

    fn normalize_relative_input_path(raw_path: &str) -> String {
        raw_path
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_owned()
    }

    fn requested_location_path_for_corpus(
        corpus: &RepositorySymbolCorpus,
        raw_path: &str,
    ) -> String {
        let requested_path = PathBuf::from(raw_path);
        if requested_path.is_absolute() {
            Self::relative_display_path(&corpus.root, &requested_path)
        } else {
            Self::normalize_relative_input_path(raw_path)
        }
    }

    fn resolve_navigation_symbol_query_from_location(
        corpora: &[Arc<RepositorySymbolCorpus>],
        raw_path: &str,
        line: usize,
        column: Option<usize>,
        repository_id_hint: Option<&str>,
    ) -> Result<String, ErrorData> {
        if line == 0 {
            return Err(Self::invalid_params(
                "line must be greater than zero",
                Some(json!({
                    "line": line,
                })),
            ));
        }
        if column == Some(0) {
            return Err(Self::invalid_params(
                "column must be greater than zero when provided",
                Some(json!({
                    "column": column,
                })),
            ));
        }

        let mut candidates: Vec<(usize, usize, String, String, usize, usize, String)> = Vec::new();
        for corpus in corpora {
            let requested_path = Self::requested_location_path_for_corpus(corpus, raw_path);
            let Some(symbol_indices) = corpus.symbols_by_relative_path.get(&requested_path) else {
                continue;
            };
            for symbol_index in symbol_indices {
                let symbol = &corpus.symbols[*symbol_index];
                let symbol_path = Self::relative_display_path(&corpus.root, &symbol.path);
                if symbol.line > line {
                    break;
                }
                if let Some(column) = column {
                    if symbol.line == line && symbol.span.start_column > column {
                        break;
                    }
                }

                let line_distance = line.saturating_sub(symbol.line);
                let column_distance = if line_distance == 0 {
                    column
                        .map(|value| value.saturating_sub(symbol.span.start_column))
                        .unwrap_or(0)
                } else {
                    0
                };
                candidates.push((
                    line_distance,
                    column_distance,
                    corpus.repository_id.clone(),
                    symbol_path,
                    symbol.line,
                    symbol.span.start_column,
                    symbol.stable_id.clone(),
                ));
            }
        }

        candidates.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
                .then(left.3.cmp(&right.3))
                .then(right.4.cmp(&left.4))
                .then(right.5.cmp(&left.5))
                .then(left.6.cmp(&right.6))
        });

        candidates
            .first()
            .map(|candidate| candidate.6.clone())
            .ok_or_else(|| {
                Self::resource_not_found(
                    "symbol not found at location",
                    Some(json!({
                        "path": raw_path,
                        "line": line,
                        "column": column,
                        "repository_id": repository_id_hint,
                    })),
                )
            })
    }

    fn resolve_navigation_target(
        corpora: &[Arc<RepositorySymbolCorpus>],
        symbol: Option<&str>,
        path: Option<&str>,
        line: Option<usize>,
        column: Option<usize>,
        repository_id_hint: Option<&str>,
    ) -> Result<ResolvedNavigationTarget, ErrorData> {
        if let Some(symbol) = symbol {
            let query = symbol.trim();
            if query.is_empty() {
                return Err(Self::invalid_params("symbol must not be empty", None));
            }
            let target =
                Self::resolve_navigation_symbol_target(corpora, query, repository_id_hint)?;
            return Ok(ResolvedNavigationTarget {
                symbol_query: query.to_owned(),
                target,
                resolution_source: "symbol",
            });
        }

        let raw_path = path.ok_or_else(|| {
            Self::invalid_params("either `symbol` or (`path` + `line`) is required", None)
        })?;
        if raw_path.trim().is_empty() {
            return Err(Self::invalid_params(
                "path must not be empty when provided",
                None,
            ));
        }
        let line = line
            .ok_or_else(|| Self::invalid_params("line is required when resolving by path", None))?;
        let symbol_query = Self::resolve_navigation_symbol_query_from_location(
            corpora,
            raw_path,
            line,
            column,
            repository_id_hint,
        )?;
        let target =
            Self::resolve_navigation_symbol_target(corpora, &symbol_query, repository_id_hint)?;
        Ok(ResolvedNavigationTarget {
            symbol_query,
            target,
            resolution_source: "location",
        })
    }

    fn try_cached_precise_definition_fast_path(
        &self,
        repository_id_hint: Option<&str>,
        raw_path: &str,
        line: usize,
        column: Option<usize>,
        limit: usize,
    ) -> Result<Option<(Json<GoToDefinitionResponse>, String, String, String)>, ErrorData>
    {
        let scoped_roots = self.roots_for_repository(repository_id_hint)?;
        if repository_id_hint.is_none() && scoped_roots.len() != 1 {
            return Ok(None);
        }

        let mut scoped_roots = scoped_roots;
        scoped_roots.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

        for (repository_id, root) in scoped_roots {
            let Some(cached_precise_graph) =
                self.try_reuse_latest_precise_graph_for_repository(&repository_id, &root)
            else {
                continue;
            };
            let relative_path = Self::canonicalize_navigation_path(&root, raw_path);
            let graph = cached_precise_graph.graph;
            let Some(precise_target) = graph.select_precise_symbol_for_location(
                &repository_id,
                &relative_path,
                line,
                column,
            ) else {
                continue;
            };

            let mut precise_matches = graph
                .precise_occurrences_for_symbol(&repository_id, &precise_target.symbol)
                .into_iter()
                .filter(|occurrence| occurrence.is_definition())
                .map(|occurrence| NavigationLocation {
                    symbol: if precise_target.display_name.is_empty() {
                        precise_target.symbol.clone()
                    } else {
                        precise_target.display_name.clone()
                    },
                    repository_id: repository_id.clone(),
                    path: Self::canonicalize_navigation_path(&root, &occurrence.path),
                    line: occurrence.range.start_line,
                    column: occurrence.range.start_column,
                    kind: Some(Self::display_symbol_kind(&precise_target.kind)),
                    precision: Some(
                        Self::precise_match_precision(cached_precise_graph.coverage_mode)
                            .to_owned(),
                    ),
                })
                .collect::<Vec<_>>();
            Self::sort_navigation_locations(&mut precise_matches);
            if precise_matches.is_empty() {
                continue;
            }
            if precise_matches.len() > limit {
                precise_matches.truncate(limit);
            }

            let precision =
                Self::precise_resolution_precision(cached_precise_graph.coverage_mode).to_owned();
            let metadata = json!({
                "precision": precision,
                "heuristic": false,
                "target_precise_symbol": precise_target.symbol.clone(),
                "resolution_source": "location_precise_cache",
                "precise": Self::precise_note_with_count(
                    cached_precise_graph.coverage_mode,
                    &cached_precise_graph.ingest_stats,
                    "definition_count",
                    precise_matches.len(),
                )
            });
            let (metadata, note) = Self::metadata_note_pair(metadata);
            return Ok(Some((
                Json(GoToDefinitionResponse {
                    matches: precise_matches,
                    metadata,
                    note,
                }),
                repository_id,
                precise_target.symbol,
                precision,
            )));
        }

        Ok(None)
    }

    fn canonicalize_navigation_path(root: &Path, raw_path: &str) -> String {
        let path = PathBuf::from(raw_path);
        let absolute_path = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        Self::relative_display_path(root, &absolute_path)
    }

    fn precise_definition_occurrence_for_symbol(
        graph: &SymbolGraph,
        repository_id: &str,
        symbol: &str,
    ) -> Option<crate::graph::PreciseOccurrenceRecord> {
        graph.precise_definition_occurrence_for_symbol(repository_id, symbol)
    }

    fn precise_navigation_candidate_anchor_rank(
        graph: &SymbolGraph,
        repository_id: &str,
        root: &Path,
        target_symbol: &SymbolDefinition,
        precise_target: &crate::graph::PreciseSymbolRecord,
    ) -> (u8, String, usize, usize) {
        let Some(definition) = Self::precise_definition_occurrence_for_symbol(
            graph,
            repository_id,
            &precise_target.symbol,
        ) else {
            return (4, String::new(), usize::MAX, usize::MAX);
        };

        let target_path = Self::relative_display_path(root, &target_symbol.path);
        let definition_path = Self::canonicalize_navigation_path(root, &definition.path);
        let rank = if definition_path == target_path
            && definition.range.start_line == target_symbol.line
            && definition.range.start_column == target_symbol.span.start_column
        {
            0
        } else if definition_path == target_path
            && definition.range.start_line == target_symbol.line
        {
            1
        } else if definition_path == target_path {
            2
        } else {
            3
        };

        (
            rank,
            definition_path,
            definition.range.start_line,
            definition.range.start_column,
        )
    }

    fn matching_precise_symbols_for_resolved_target(
        graph: &SymbolGraph,
        repository_id: &str,
        root: &Path,
        symbol_query: &str,
        target_symbol: &SymbolDefinition,
    ) -> Vec<crate::graph::PreciseSymbolRecord> {
        let mut candidates = graph.matching_precise_symbols_for_navigation(
            repository_id,
            symbol_query,
            &target_symbol.name,
        );
        candidates.sort_by(|left, right| {
            Self::precise_navigation_candidate_anchor_rank(
                graph,
                repository_id,
                root,
                target_symbol,
                left,
            )
            .cmp(&Self::precise_navigation_candidate_anchor_rank(
                graph,
                repository_id,
                root,
                target_symbol,
                right,
            ))
            .then(left.symbol.cmp(&right.symbol))
            .then(left.display_name.cmp(&right.display_name))
            .then(left.kind.cmp(&right.kind))
        });
        candidates
    }

    fn select_precise_symbol_for_resolved_target(
        graph: &SymbolGraph,
        repository_id: &str,
        root: &Path,
        symbol_query: &str,
        target_symbol: &SymbolDefinition,
    ) -> Option<crate::graph::PreciseSymbolRecord> {
        Self::matching_precise_symbols_for_resolved_target(
            graph,
            repository_id,
            root,
            symbol_query,
            target_symbol,
        )
        .into_iter()
        .next()
    }

    fn precise_relationships_to_symbol_by_kind(
        graph: &SymbolGraph,
        repository_id: &str,
        to_symbol: &str,
        kinds: &[PreciseRelationshipKind],
    ) -> Vec<crate::graph::PreciseRelationshipRecord> {
        graph.precise_relationships_to_symbol_by_kinds(repository_id, to_symbol, kinds)
    }

    fn map_lossy_line_slice_error(path: &Path, error: LossyLineSliceError) -> ErrorData {
        match error {
            LossyLineSliceError::Io(err) => Self::internal(
                format!("failed to read file {}: {err}", path.display()),
                None,
            ),
            LossyLineSliceError::LineStartOutside {
                line_start,
                line_end,
                total_lines,
            } => Self::invalid_params(
                "line_start is outside file bounds",
                Some(serde_json::json!({
                    "line_start": line_start,
                    "line_end": line_end,
                    "total_lines": total_lines,
                })),
            ),
        }
    }

    fn line_slice_budget_error(
        path: &str,
        bytes: usize,
        max_bytes: usize,
        line_start: usize,
        line_end: usize,
        total_lines: usize,
    ) -> ErrorData {
        Self::invalid_params(
            format!("selected line range exceeds max_bytes={max_bytes}"),
            Some(serde_json::json!({
                "path": path,
                "bytes": bytes,
                "max_bytes": max_bytes,
                "config_max_file_bytes": max_bytes,
                "line_start": line_start,
                "line_end": line_end,
                "total_lines": total_lines,
            })),
        )
    }

    fn sort_navigation_locations(matches: &mut [NavigationLocation]) {
        matches.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.path.cmp(&right.path))
                .then(left.line.cmp(&right.line))
                .then(left.column.cmp(&right.column))
                .then(left.symbol.cmp(&right.symbol))
                .then(left.kind.cmp(&right.kind))
                .then(left.precision.cmp(&right.precision))
        });
    }

    fn sort_implementation_matches(matches: &mut [ImplementationMatch]) {
        matches.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.path.cmp(&right.path))
                .then(left.line.cmp(&right.line))
                .then(left.column.cmp(&right.column))
                .then(left.symbol.cmp(&right.symbol))
                .then(left.kind.cmp(&right.kind))
                .then(left.relation.cmp(&right.relation))
                .then(left.precision.cmp(&right.precision))
                .then(left.fallback_reason.cmp(&right.fallback_reason))
        });
    }

    fn precise_implementation_matches_for_symbol(
        graph: &SymbolGraph,
        repository_id: &str,
        root: &Path,
        coverage_mode: PreciseCoverageMode,
        precise_target: &crate::graph::PreciseSymbolRecord,
    ) -> Vec<ImplementationMatch> {
        let precision = Self::precise_match_precision(coverage_mode).to_owned();
        let mut matches = Self::precise_relationships_to_symbol_by_kind(
            graph,
            repository_id,
            &precise_target.symbol,
            &[
                PreciseRelationshipKind::Implementation,
                PreciseRelationshipKind::TypeDefinition,
            ],
        )
        .into_iter()
        .filter_map(|relationship| {
            let implementation_symbol = graph
                .precise_symbol(repository_id, &relationship.from_symbol)?
                .clone();
            let definition = Self::precise_definition_occurrence_for_symbol(
                graph,
                repository_id,
                &relationship.from_symbol,
            )?;
            Some(ImplementationMatch {
                symbol: if implementation_symbol.display_name.is_empty() {
                    implementation_symbol.symbol
                } else {
                    implementation_symbol.display_name
                },
                kind: Self::display_symbol_kind(&implementation_symbol.kind),
                repository_id: repository_id.to_owned(),
                path: Self::canonicalize_navigation_path(root, &definition.path),
                line: definition.range.start_line,
                column: definition.range.start_column,
                relation: Some(relationship.kind.as_str().to_owned()),
                precision: Some(precision.clone()),
                fallback_reason: None,
            })
        })
        .collect::<Vec<_>>();
        Self::sort_implementation_matches(&mut matches);
        matches
    }

    fn precise_implementation_matches_from_occurrences(
        graph: &SymbolGraph,
        target_corpus: &RepositorySymbolCorpus,
        root: &Path,
        target_symbol_name: &str,
        coverage_mode: PreciseCoverageMode,
        precise_target: &crate::graph::PreciseSymbolRecord,
    ) -> Vec<ImplementationMatch> {
        let precision = Self::precise_match_precision(coverage_mode).to_owned();
        let target_name = if precise_target.display_name.is_empty() {
            target_symbol_name
        } else {
            precise_target.display_name.as_str()
        };

        let mut matches = graph
            .precise_references_for_symbol(&target_corpus.repository_id, &precise_target.symbol)
            .into_iter()
            .filter_map(|occurrence| {
                let enclosing_symbol = Self::precise_enclosing_symbol_for_occurrence(
                    target_corpus,
                    root,
                    &occurrence,
                    None,
                )?;
                if enclosing_symbol.kind.as_str() != "impl" {
                    return None;
                }

                let (implemented_trait, implementing_type) =
                    Self::parse_rust_impl_signature(enclosing_symbol.name.as_str())?;
                let (symbol, kind, path, line, column, relation) =
                    if let Some(implemented_trait) = implemented_trait {
                        if implemented_trait.eq_ignore_ascii_case(target_name) {
                            let implementing_symbol = graph.select_precise_symbol_for_navigation(
                                &target_corpus.repository_id,
                                implementing_type,
                                implementing_type,
                            )?;
                            let definition = Self::precise_definition_occurrence_for_symbol(
                                graph,
                                &target_corpus.repository_id,
                                &implementing_symbol.symbol,
                            )?;
                            (
                                if implementing_symbol.display_name.is_empty() {
                                    implementing_symbol.symbol
                                } else {
                                    implementing_symbol.display_name
                                },
                                Self::display_symbol_kind(&implementing_symbol.kind),
                                Self::canonicalize_navigation_path(root, &definition.path),
                                definition.range.start_line,
                                definition.range.start_column,
                                Some("implementation".to_owned()),
                            )
                        } else if implementing_type.eq_ignore_ascii_case(target_name) {
                            (
                                enclosing_symbol.name.clone(),
                                Self::display_symbol_kind(enclosing_symbol.kind.as_str()),
                                Self::relative_display_path(root, &enclosing_symbol.path),
                                enclosing_symbol.line,
                                enclosing_symbol.span.start_column,
                                Some("type_definition".to_owned()),
                            )
                        } else {
                            return None;
                        }
                    } else if implementing_type.eq_ignore_ascii_case(target_name) {
                        (
                            enclosing_symbol.name.clone(),
                            Self::display_symbol_kind(enclosing_symbol.kind.as_str()),
                            Self::relative_display_path(root, &enclosing_symbol.path),
                            enclosing_symbol.line,
                            enclosing_symbol.span.start_column,
                            Some("type_definition".to_owned()),
                        )
                    } else {
                        return None;
                    };

                Some(ImplementationMatch {
                    symbol,
                    kind,
                    repository_id: target_corpus.repository_id.clone(),
                    path,
                    line,
                    column,
                    relation,
                    precision: Some(precision.clone()),
                    fallback_reason: None,
                })
            })
            .collect::<Vec<_>>();
        Self::sort_implementation_matches(&mut matches);
        matches.dedup_by(|left, right| {
            left.repository_id == right.repository_id
                && left.path == right.path
                && left.line == right.line
                && left.column == right.column
                && left.symbol == right.symbol
                && left.kind == right.kind
                && left.relation == right.relation
                && left.precision == right.precision
                && left.fallback_reason == right.fallback_reason
        });
        matches
    }

    fn precise_incoming_matches_from_relationships(
        graph: &SymbolGraph,
        repository_id: &str,
        root: &Path,
        target_symbol_name: &str,
        coverage_mode: PreciseCoverageMode,
        precise_target: &crate::graph::PreciseSymbolRecord,
    ) -> Vec<CallHierarchyMatch> {
        let precision = Self::precise_match_precision(coverage_mode).to_owned();
        let mut matches = Self::precise_relationships_to_symbol_by_kind(
            graph,
            repository_id,
            &precise_target.symbol,
            &[PreciseRelationshipKind::Reference],
        )
        .into_iter()
        .filter_map(|relationship| {
            let caller_symbol = graph
                .precise_symbol(repository_id, &relationship.from_symbol)?
                .clone();
            let caller_definition = Self::precise_definition_occurrence_for_symbol(
                graph,
                repository_id,
                &relationship.from_symbol,
            )?;
            Some(CallHierarchyMatch {
                source_symbol: if caller_symbol.display_name.is_empty() {
                    caller_symbol.symbol
                } else {
                    caller_symbol.display_name
                },
                target_symbol: if precise_target.display_name.is_empty() {
                    target_symbol_name.to_owned()
                } else {
                    precise_target.display_name.clone()
                },
                repository_id: repository_id.to_owned(),
                path: Self::canonicalize_navigation_path(root, &caller_definition.path),
                line: caller_definition.range.start_line,
                column: caller_definition.range.start_column,
                relation: "calls".to_owned(),
                precision: Some(precision.clone()),
                call_path: None,
                call_line: None,
                call_column: None,
                call_end_line: None,
                call_end_column: None,
            })
        })
        .collect::<Vec<_>>();
        Self::sort_call_hierarchy_matches(&mut matches);
        matches
    }

    fn precise_enclosing_symbol_for_occurrence<'a>(
        target_corpus: &'a RepositorySymbolCorpus,
        root: &Path,
        occurrence: &crate::graph::PreciseOccurrenceRecord,
        exclude_symbol_id: Option<&str>,
    ) -> Option<&'a SymbolDefinition> {
        let occurrence_path = Self::canonicalize_navigation_path(root, &occurrence.path);
        target_corpus
            .symbols_by_relative_path
            .get(&occurrence_path)
            .into_iter()
            .flat_map(|indices| indices.iter())
            .map(|index| &target_corpus.symbols[*index])
            .filter(|symbol| {
                exclude_symbol_id
                    .map(|exclude| symbol.stable_id != exclude)
                    .unwrap_or(true)
            })
            .filter(|symbol| {
                Self::source_span_contains_precise_range(&symbol.span, &occurrence.range)
            })
            .min_by(|left, right| {
                let left_span = left.span.end_line.saturating_sub(left.span.start_line);
                let right_span = right.span.end_line.saturating_sub(right.span.start_line);
                let left_column_span = if left_span == 0 {
                    left.span.end_column.saturating_sub(left.span.start_column)
                } else {
                    usize::MAX
                };
                let right_column_span = if right_span == 0 {
                    right
                        .span
                        .end_column
                        .saturating_sub(right.span.start_column)
                } else {
                    usize::MAX
                };
                left_span
                    .cmp(&right_span)
                    .then(left_column_span.cmp(&right_column_span))
                    .then(left.span.start_line.cmp(&right.span.start_line))
                    .then(left.span.start_column.cmp(&right.span.start_column))
                    .then(left.stable_id.cmp(&right.stable_id))
            })
    }

    fn precise_incoming_matches_from_occurrences(
        graph: &SymbolGraph,
        target_corpus: &RepositorySymbolCorpus,
        root: &Path,
        target_symbol_name: &str,
        coverage_mode: PreciseCoverageMode,
        precise_target: &crate::graph::PreciseSymbolRecord,
        exclude_symbol_id: &str,
    ) -> Vec<CallHierarchyMatch> {
        let precision = Self::precise_match_precision(coverage_mode).to_owned();
        let mut source_cache: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut matches = graph
            .precise_references_for_symbol(&target_corpus.repository_id, &precise_target.symbol)
            .into_iter()
            .filter_map(|occurrence| {
                let enclosing_symbol = Self::precise_enclosing_symbol_for_occurrence(
                    target_corpus,
                    root,
                    &occurrence,
                    Some(exclude_symbol_id),
                )?;
                let relation = Self::classify_precise_incoming_occurrence_relation(
                    root,
                    precise_target,
                    &occurrence,
                    &mut source_cache,
                );
                let (call_path, call_line, call_column, call_end_line, call_end_column) =
                    Self::precise_call_site_fields(root, &occurrence);
                Some(CallHierarchyMatch {
                    source_symbol: enclosing_symbol.name.clone(),
                    target_symbol: if precise_target.display_name.is_empty() {
                        target_symbol_name.to_owned()
                    } else {
                        precise_target.display_name.clone()
                    },
                    repository_id: target_corpus.repository_id.clone(),
                    path: Self::relative_display_path(root, &enclosing_symbol.path),
                    line: enclosing_symbol.line,
                    column: enclosing_symbol.span.start_column,
                    relation: relation.to_owned(),
                    precision: Some(precision.clone()),
                    call_path,
                    call_line,
                    call_column,
                    call_end_line,
                    call_end_column,
                })
            })
            .collect::<Vec<_>>();
        Self::sort_call_hierarchy_matches(&mut matches);
        matches.dedup_by(|left, right| {
            left.repository_id == right.repository_id
                && left.path == right.path
                && left.line == right.line
                && left.column == right.column
                && left.source_symbol == right.source_symbol
                && left.target_symbol == right.target_symbol
                && left.relation == right.relation
                && left.precision == right.precision
                && left.call_path == right.call_path
                && left.call_line == right.call_line
                && left.call_column == right.call_column
                && left.call_end_line == right.call_end_line
                && left.call_end_column == right.call_end_column
        });
        matches
    }

    fn classify_precise_incoming_occurrence_relation(
        root: &Path,
        precise_target: &crate::graph::PreciseSymbolRecord,
        occurrence: &crate::graph::PreciseOccurrenceRecord,
        source_cache: &mut BTreeMap<String, Option<String>>,
    ) -> &'static str {
        if !Self::is_precise_callable_kind(&precise_target.kind) {
            return "refers_to";
        }
        if Self::precise_occurrence_has_call_like_source(
            root,
            precise_target,
            occurrence,
            source_cache,
        ) {
            "calls"
        } else {
            "refers_to"
        }
    }

    fn precise_occurrence_has_call_like_source(
        root: &Path,
        precise_target: &crate::graph::PreciseSymbolRecord,
        occurrence: &crate::graph::PreciseOccurrenceRecord,
        source_cache: &mut BTreeMap<String, Option<String>>,
    ) -> bool {
        let source = source_cache
            .entry(occurrence.path.clone())
            .or_insert_with(|| {
                let occurrence_path = Path::new(&occurrence.path);
                let absolute_path = if occurrence_path.is_absolute() {
                    occurrence_path.to_path_buf()
                } else {
                    root.join(occurrence_path)
                };
                fs::read_to_string(absolute_path).ok()
            })
            .as_deref();
        let Some(source) = source else {
            return false;
        };
        let Some(line) = Self::source_line_for_precise_range(source, &occurrence.range) else {
            return false;
        };
        let target_name = Self::precise_target_call_name(precise_target);
        line.match_indices(target_name).any(|(index, _)| {
            let suffix_start = index.saturating_add(target_name.len()).min(line.len());
            line.get(suffix_start..)
                .map(Self::source_suffix_looks_like_rust_call)
                .unwrap_or(false)
        })
    }

    fn precise_target_call_name<'a>(
        precise_target: &'a crate::graph::PreciseSymbolRecord,
    ) -> &'a str {
        if !precise_target.display_name.is_empty() {
            return precise_target.display_name.as_str();
        }
        precise_target
            .symbol
            .rsplit(['#', '/', '.'])
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or(precise_target.symbol.as_str())
    }

    fn source_line_for_precise_range<'a>(
        source: &'a str,
        range: &crate::graph::PreciseRange,
    ) -> Option<&'a str> {
        source.lines().nth(range.start_line.saturating_sub(1))
    }

    fn source_suffix_looks_like_rust_call(mut suffix: &str) -> bool {
        suffix = suffix.trim_start_matches(|ch: char| ch == ' ' || ch == '\t');
        suffix = suffix.trim_start_matches(|ch: char| ch.is_ascii_alphanumeric() || ch == '_');
        suffix = suffix.trim_start_matches(|ch: char| ch == ' ' || ch == '\t');
        if suffix.starts_with('(') {
            return true;
        }
        if !suffix.starts_with("::") {
            return false;
        }

        suffix = suffix[2..].trim_start_matches(|ch: char| ch == ' ' || ch == '\t');
        if !suffix.starts_with('<') {
            return false;
        }

        let mut depth = 0usize;
        let mut end_index = None;
        for (index, ch) in suffix.char_indices() {
            match ch {
                '<' => depth = depth.saturating_add(1),
                '>' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end_index = Some(index + ch.len_utf8());
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end_index) = end_index else {
            return false;
        };
        suffix[end_index..]
            .trim_start_matches(|ch: char| ch == ' ' || ch == '\t')
            .starts_with('(')
    }

    fn precise_outgoing_matches_from_occurrences(
        graph: &SymbolGraph,
        target_corpus: &RepositorySymbolCorpus,
        root: &Path,
        source_symbol_name: &str,
        coverage_mode: PreciseCoverageMode,
        precise_target: &crate::graph::PreciseSymbolRecord,
        enclosing_symbol_id: &str,
    ) -> Vec<CallHierarchyMatch> {
        let precision = Self::precise_match_precision(coverage_mode).to_owned();
        let source_definition = match Self::precise_definition_occurrence_for_symbol(
            graph,
            &target_corpus.repository_id,
            &precise_target.symbol,
        ) {
            Some(definition) => definition,
            None => return Vec::new(),
        };
        let source_path = Self::canonicalize_navigation_path(root, &source_definition.path);
        let mut matches = graph
            .precise_occurrences_for_file(&target_corpus.repository_id, &source_path)
            .into_iter()
            .filter(|occurrence| !occurrence.is_definition())
            .filter(|occurrence| occurrence.symbol != precise_target.symbol)
            .filter_map(|occurrence| {
                let enclosing_symbol = Self::precise_enclosing_symbol_for_occurrence(
                    target_corpus,
                    root,
                    &occurrence,
                    None,
                )?;
                if enclosing_symbol.stable_id != enclosing_symbol_id {
                    return None;
                }

                let callee_symbol = graph
                    .precise_symbol(&target_corpus.repository_id, &occurrence.symbol)?
                    .clone();
                if !Self::is_precise_callable_kind(&callee_symbol.kind) {
                    return None;
                }
                let callee_definition = Self::precise_definition_occurrence_for_symbol(
                    graph,
                    &target_corpus.repository_id,
                    &occurrence.symbol,
                )?;
                let (call_path, call_line, call_column, call_end_line, call_end_column) =
                    Self::precise_call_site_fields(root, &occurrence);
                Some(CallHierarchyMatch {
                    source_symbol: if precise_target.display_name.is_empty() {
                        source_symbol_name.to_owned()
                    } else {
                        precise_target.display_name.clone()
                    },
                    target_symbol: if callee_symbol.display_name.is_empty() {
                        callee_symbol.symbol
                    } else {
                        callee_symbol.display_name
                    },
                    repository_id: target_corpus.repository_id.clone(),
                    path: Self::canonicalize_navigation_path(root, &callee_definition.path),
                    line: callee_definition.range.start_line,
                    column: callee_definition.range.start_column,
                    relation: "calls".to_owned(),
                    precision: Some(precision.clone()),
                    call_path,
                    call_line,
                    call_column,
                    call_end_line,
                    call_end_column,
                })
            })
            .collect::<Vec<_>>();
        Self::sort_call_hierarchy_matches(&mut matches);
        matches.dedup_by(|left, right| {
            left.repository_id == right.repository_id
                && left.path == right.path
                && left.line == right.line
                && left.column == right.column
                && left.source_symbol == right.source_symbol
                && left.target_symbol == right.target_symbol
                && left.relation == right.relation
                && left.precision == right.precision
                && left.call_path == right.call_path
                && left.call_line == right.call_line
                && left.call_column == right.call_column
                && left.call_end_line == right.call_end_line
                && left.call_end_column == right.call_end_column
        });
        matches
    }

    fn position_leq(
        left_line: usize,
        left_column: usize,
        right_line: usize,
        right_column: usize,
    ) -> bool {
        (left_line, left_column) <= (right_line, right_column)
    }

    fn source_span_contains_precise_range(
        span: &SourceSpan,
        range: &crate::graph::PreciseRange,
    ) -> bool {
        Self::position_leq(
            span.start_line,
            span.start_column,
            range.start_line,
            range.start_column,
        ) && Self::position_leq(
            range.end_line,
            range.end_column,
            span.end_line,
            span.end_column,
        )
    }

    fn precise_kind_numeric_value(kind: &str) -> Option<i32> {
        kind.strip_prefix("kind_")
            .unwrap_or(kind)
            .parse::<i32>()
            .ok()
    }

    fn display_symbol_kind(kind: &str) -> Option<String> {
        let normalized = kind.trim();
        if normalized.is_empty() {
            return None;
        }

        if let Some(value) = Self::precise_kind_numeric_value(normalized) {
            if let Some(kind) = ScipSymbolKind::from_i32(value) {
                return Some(Self::camel_to_snake_case(&format!("{kind:?}")));
            }
        }

        Some(Self::camel_to_snake_case(normalized))
    }

    fn camel_to_snake_case(raw: &str) -> String {
        let mut output = String::with_capacity(raw.len());
        let mut previous_was_separator = false;
        let mut previous_was_lower_or_digit = false;

        for character in raw.chars() {
            if matches!(character, '_' | '-' | ' ' | '\t') {
                if !output.ends_with('_') && !output.is_empty() {
                    output.push('_');
                }
                previous_was_separator = true;
                previous_was_lower_or_digit = false;
                continue;
            }

            if character.is_ascii_uppercase()
                && !output.is_empty()
                && !previous_was_separator
                && previous_was_lower_or_digit
            {
                output.push('_');
            }

            output.push(character.to_ascii_lowercase());
            previous_was_separator = false;
            previous_was_lower_or_digit =
                character.is_ascii_lowercase() || character.is_ascii_digit();
        }

        output
    }

    fn metadata_note_pair(metadata: Value) -> (Option<Value>, Option<String>) {
        let note = Some(metadata.to_string());
        (Some(metadata), note)
    }

    fn precise_call_site_fields(
        root: &Path,
        occurrence: &crate::graph::PreciseOccurrenceRecord,
    ) -> (
        Option<String>,
        Option<usize>,
        Option<usize>,
        Option<usize>,
        Option<usize>,
    ) {
        (
            Some(Self::canonicalize_navigation_path(root, &occurrence.path)),
            Some(occurrence.range.start_line),
            Some(occurrence.range.start_column),
            Some(occurrence.range.end_line),
            Some(occurrence.range.end_column),
        )
    }

    fn source_span_contains_symbol(parent: &SourceSpan, child: &SourceSpan) -> bool {
        parent.start_byte <= child.start_byte
            && child.end_byte <= parent.end_byte
            && (parent.start_byte < child.start_byte || child.end_byte < parent.end_byte)
    }

    fn build_document_symbol_tree(
        symbols: &[SymbolDefinition],
        repository_id: &str,
        display_path: &str,
    ) -> Vec<crate::mcp::types::DocumentSymbolItem> {
        #[derive(Clone)]
        struct PendingDocumentSymbolNode {
            item: crate::mcp::types::DocumentSymbolItem,
            span: SourceSpan,
            children: Vec<usize>,
        }

        fn materialize(
            nodes: &[PendingDocumentSymbolNode],
            index: usize,
        ) -> crate::mcp::types::DocumentSymbolItem {
            let mut item = nodes[index].item.clone();
            item.children = nodes[index]
                .children
                .iter()
                .map(|child_index| materialize(nodes, *child_index))
                .collect();
            item
        }

        let mut nodes: Vec<PendingDocumentSymbolNode> = Vec::with_capacity(symbols.len());
        let mut root_indices = Vec::new();
        let mut stack: Vec<usize> = Vec::new();

        for symbol in symbols {
            while let Some(parent_index) = stack.last().copied() {
                if Self::source_span_contains_symbol(&nodes[parent_index].span, &symbol.span) {
                    break;
                }
                stack.pop();
            }

            let container = stack
                .last()
                .map(|parent_index| nodes[*parent_index].item.symbol.clone());
            let node_index = nodes.len();
            nodes.push(PendingDocumentSymbolNode {
                item: crate::mcp::types::DocumentSymbolItem {
                    symbol: symbol.name.clone(),
                    kind: symbol.kind.as_str().to_owned(),
                    repository_id: repository_id.to_owned(),
                    path: display_path.to_owned(),
                    line: symbol.span.start_line,
                    column: symbol.span.start_column,
                    end_line: Some(symbol.span.end_line),
                    end_column: Some(symbol.span.end_column),
                    container,
                    children: Vec::new(),
                },
                span: symbol.span.clone(),
                children: Vec::new(),
            });

            if let Some(parent_index) = stack.last().copied() {
                nodes[parent_index].children.push(node_index);
            } else {
                root_indices.push(node_index);
            }
            stack.push(node_index);
        }

        root_indices
            .into_iter()
            .map(|index| materialize(&nodes, index))
            .collect()
    }

    fn is_precise_callable_kind(kind: &str) -> bool {
        let normalized = kind.trim().to_ascii_lowercase();
        matches!(
            normalized.as_str(),
            "function"
                | "method"
                | "constructor"
                | "abstract_method"
                | "method_alias"
                | "method_specification"
                | "protocol_method"
                | "pure_virtual_method"
                | "singleton_method"
                | "static_method"
                | "trait_method"
                | "type_class_method"
        ) || matches!(
            Self::precise_kind_numeric_value(&normalized),
            Some(9 | 17 | 26 | 66 | 67 | 68 | 69 | 70 | 74 | 76 | 80)
        )
    }

    fn is_heuristic_callable_kind(kind: &str) -> bool {
        matches!(
            kind.trim().to_ascii_lowercase().as_str(),
            "function" | "method"
        )
    }

    fn search_hybrid_warning(
        semantic_status: Option<&str>,
        semantic_reason: Option<&str>,
        semantic_hit_count: Option<usize>,
        semantic_match_count: Option<usize>,
    ) -> Option<String> {
        match semantic_status {
            Some("disabled") => Some(match semantic_reason {
                Some(reason) if !reason.trim().is_empty() => format!(
                    "semantic retrieval is disabled; results are ranked from lexical and graph signals only ({reason})"
                ),
                _ => "semantic retrieval is disabled; results are ranked from lexical and graph signals only".to_owned(),
            }),
            Some("unavailable") => Some(match semantic_reason {
                Some(reason) if !reason.trim().is_empty() => format!(
                    "semantic retrieval is unavailable; results are ranked from lexical and graph signals only ({reason})"
                ),
                _ => "semantic retrieval is unavailable; results are ranked from lexical and graph signals only".to_owned(),
            }),
            Some("degraded") => Some(match semantic_reason {
                Some(reason) if !reason.trim().is_empty() => format!(
                    "semantic retrieval is degraded; semantic contribution may be partial ({reason})"
                ),
                _ => "semantic retrieval is degraded; semantic contribution may be partial".to_owned(),
            }),
            Some("ok") if semantic_hit_count == Some(0) => Some(
                "semantic retrieval completed successfully but retained no query-relevant semantic hits; results are ranked from lexical and graph signals only"
                    .to_owned(),
            ),
            Some("ok")
                if semantic_hit_count.unwrap_or(0) > 0
                    && semantic_match_count == Some(0) =>
            {
                Some(
                    "semantic retrieval retained semantic hits, but none contributed to the returned top results; ranking is effectively lexical and graph for this result set"
                        .to_owned(),
                )
            }
            _ => None,
        }
    }

    fn parse_rust_impl_signature(symbol_name: &str) -> Option<(Option<&str>, &str)> {
        let body = symbol_name.strip_prefix("impl ")?;
        if let Some((implemented_trait, implementing_type)) = body.split_once(" for ") {
            let implemented_trait = implemented_trait.trim();
            let implementing_type = implementing_type.trim();
            if implemented_trait.is_empty() || implementing_type.is_empty() {
                return None;
            }
            return Some((Some(implemented_trait), implementing_type));
        }
        let implementing_type = body.trim();
        if implementing_type.is_empty() {
            return None;
        }
        Some((None, implementing_type))
    }

    fn navigation_target_selection_note(
        symbol_query: &str,
        target: &SymbolCandidate,
        candidate_count: usize,
        selected_rank_candidate_count: usize,
    ) -> serde_json::Value {
        json!({
            "query": symbol_query,
            "selected_symbol_id": target.symbol.stable_id,
            "selected_symbol": target.symbol.name,
            "selected_kind": target.symbol.kind,
            "selected_repository_id": target.repository_id,
            "selected_path": Self::relative_display_path(&target.root, &target.symbol.path),
            "selected_path_class": target.path_class,
            "selected_line": target.symbol.line,
            "selected_rank": target.rank,
            "candidate_count": candidate_count,
            "same_rank_candidate_count": selected_rank_candidate_count,
            "ambiguous_query": selected_rank_candidate_count > 1,
        })
    }

    fn precise_absence_reason(
        coverage_mode: PreciseCoverageMode,
        stats: &PreciseIngestStats,
        precise_match_count: usize,
    ) -> &'static str {
        if stats.artifacts_discovered == 0 {
            return "no_scip_artifacts_discovered";
        }

        match coverage_mode {
            PreciseCoverageMode::Partial if precise_match_count == 0 => {
                return "precise_partial_non_authoritative_absence";
            }
            PreciseCoverageMode::None if stats.artifacts_failed > 0 => {
                return "scip_artifact_ingest_failed";
            }
            PreciseCoverageMode::Full | PreciseCoverageMode::Partial
                if stats.artifacts_ingested > 0 && precise_match_count == 0 =>
            {
                return "target_not_present_in_precise_graph";
            }
            PreciseCoverageMode::None => {
                return "no_usable_precise_data";
            }
            _ => {}
        }

        "precise_unavailable"
    }

    fn precise_coverage_mode(stats: &PreciseIngestStats) -> PreciseCoverageMode {
        if stats.artifacts_ingested == 0 {
            return PreciseCoverageMode::None;
        }
        if stats.artifacts_failed > 0 {
            return PreciseCoverageMode::Partial;
        }
        PreciseCoverageMode::Full
    }

    fn precise_resolution_precision(coverage_mode: PreciseCoverageMode) -> &'static str {
        match coverage_mode {
            PreciseCoverageMode::Full => "precise",
            PreciseCoverageMode::Partial => "precise_partial",
            PreciseCoverageMode::None => "heuristic",
        }
    }

    fn precise_match_precision(coverage_mode: PreciseCoverageMode) -> &'static str {
        match coverage_mode {
            PreciseCoverageMode::Full => "precise",
            PreciseCoverageMode::Partial => "precise_partial",
            PreciseCoverageMode::None => "heuristic",
        }
    }

    fn precise_note_metadata(
        coverage_mode: PreciseCoverageMode,
        stats: &PreciseIngestStats,
    ) -> serde_json::Value {
        json!({
            "coverage": coverage_mode.as_str(),
            "candidate_directories": Self::bounded_text_values(&stats.candidate_directories),
            "discovered_artifacts": Self::bounded_text_values(&stats.discovered_artifacts),
            "artifacts_discovered": stats.artifacts_discovered,
            "artifacts_discovered_bytes": stats.artifacts_discovered_bytes,
            "artifacts_ingested": stats.artifacts_ingested,
            "artifacts_ingested_bytes": stats.artifacts_ingested_bytes,
            "artifacts_failed": stats.artifacts_failed,
            "artifacts_failed_bytes": stats.artifacts_failed_bytes,
            "failed_artifacts": Self::precise_failure_note_entries(stats),
        })
    }

    fn precise_note_with_count(
        coverage_mode: PreciseCoverageMode,
        stats: &PreciseIngestStats,
        count_key: &str,
        count: usize,
    ) -> serde_json::Value {
        let mut precise = Self::precise_note_metadata(coverage_mode, stats);
        precise[count_key] = json!(count);
        precise
    }

    fn push_precise_failure_sample(
        stats: &mut PreciseIngestStats,
        artifact_label: impl Into<String>,
        stage: &str,
        detail: impl AsRef<str>,
    ) {
        if stats.failed_artifacts.len() >= Self::PRECISE_FAILURE_SAMPLE_LIMIT {
            return;
        }

        let artifact_label = artifact_label.into();
        stats.failed_artifacts.push(PreciseArtifactFailureSample {
            artifact_label: Self::bounded_text(&artifact_label),
            stage: stage.to_owned(),
            detail: Self::bounded_text(detail.as_ref()),
        });
    }

    fn precise_failure_note_entries(stats: &PreciseIngestStats) -> Vec<Value> {
        stats
            .failed_artifacts
            .iter()
            .map(|sample| {
                json!({
                    "artifact_label": sample.artifact_label,
                    "stage": sample.stage,
                    "detail": sample.detail,
                })
            })
            .collect()
    }

    fn bounded_text_values(values: &[String]) -> Vec<String> {
        values
            .iter()
            .map(|value| Self::bounded_text(value))
            .collect::<Vec<_>>()
    }

    fn heuristic_implementation_matches_from_symbols(
        target_symbol: &SymbolDefinition,
        target_corpus: &RepositorySymbolCorpus,
        target_root: &Path,
    ) -> Vec<ImplementationMatch> {
        match heuristic_implementation_strategy(target_symbol.language) {
            Some(HeuristicImplementationStrategy::RustImplBlocks) => {
                Self::heuristic_rust_implementation_matches_from_symbols(
                    target_symbol,
                    target_corpus,
                    target_root,
                )
            }
            Some(HeuristicImplementationStrategy::PhpDeclarationRelations) => {
                Self::heuristic_php_implementation_matches_from_symbols(
                    target_symbol,
                    target_corpus,
                    target_root,
                )
            }
            None => Vec::new(),
        }
    }

    fn heuristic_rust_implementation_matches_from_symbols(
        target_symbol: &SymbolDefinition,
        target_corpus: &RepositorySymbolCorpus,
        target_root: &Path,
    ) -> Vec<ImplementationMatch> {
        let target_name = target_symbol.name.trim();
        if target_name.is_empty() {
            return Vec::new();
        }

        let mut matches = Vec::new();
        for symbol in &target_corpus.symbols {
            if symbol.kind.as_str() != "impl" {
                continue;
            }

            let impl_symbol_name = symbol.name.trim();
            if impl_symbol_name.is_empty() {
                continue;
            }

            let mut relation = Some("implementation".to_owned());
            let matched_display_name = if let Some((implemented_trait, implementing_type)) =
                Self::parse_rust_impl_signature(impl_symbol_name)
            {
                if let Some(implemented_trait) = implemented_trait {
                    if implemented_trait.eq_ignore_ascii_case(target_name) {
                        relation = Some("implements".to_owned());
                        implementing_type.to_owned()
                    } else if implementing_type.eq_ignore_ascii_case(target_name) {
                        impl_symbol_name.to_owned()
                    } else {
                        continue;
                    }
                } else if implementing_type.eq_ignore_ascii_case(target_name) {
                    impl_symbol_name.to_owned()
                } else {
                    continue;
                }
            } else {
                continue;
            };

            matches.push(ImplementationMatch {
                symbol: matched_display_name,
                kind: Self::display_symbol_kind(symbol.kind.as_str()),
                repository_id: target_corpus.repository_id.clone(),
                path: Self::relative_display_path(target_root, &symbol.path),
                line: symbol.line,
                column: 1,
                relation,
                precision: Some("heuristic".to_owned()),
                fallback_reason: Some("precise_absent".to_owned()),
            });
        }

        Self::dedup_sorted_implementation_matches(matches)
    }

    fn heuristic_php_implementation_matches_from_symbols(
        target_symbol: &SymbolDefinition,
        target_corpus: &RepositorySymbolCorpus,
        target_root: &Path,
    ) -> Vec<ImplementationMatch> {
        let candidate_files = target_corpus
            .source_paths
            .iter()
            .map(|path| (Self::relative_display_path(target_root, path), path.clone()))
            .collect::<Vec<_>>();
        let mut matches = Vec::new();
        for (source_symbol_index, relation) in php_heuristic_implementation_candidates_for_target(
            target_symbol,
            &candidate_files,
            &target_corpus.symbols,
            &target_corpus.symbols_by_relative_path,
            Some(&target_corpus.symbol_indices_by_name),
            Some(&target_corpus.symbol_indices_by_lower_name),
        ) {
            let source_symbol = &target_corpus.symbols[source_symbol_index];
            if source_symbol.stable_id == target_symbol.stable_id {
                continue;
            }

            matches.push(ImplementationMatch {
                symbol: source_symbol.name.clone(),
                kind: Self::display_symbol_kind(source_symbol.kind.as_str()),
                repository_id: target_corpus.repository_id.clone(),
                path: Self::relative_display_path(target_root, &source_symbol.path),
                line: source_symbol.line,
                column: 1,
                relation: Some(relation.as_str().to_owned()),
                precision: Some("heuristic".to_owned()),
                fallback_reason: Some("precise_absent".to_owned()),
            });
        }

        Self::dedup_sorted_implementation_matches(matches)
    }

    fn dedup_sorted_implementation_matches(
        mut matches: Vec<ImplementationMatch>,
    ) -> Vec<ImplementationMatch> {
        Self::sort_implementation_matches(&mut matches);
        matches.dedup_by(|left, right| {
            left.repository_id == right.repository_id
                && left.path == right.path
                && left.line == right.line
                && left.column == right.column
                && left.symbol == right.symbol
                && left.kind == right.kind
                && left.relation == right.relation
                && left.precision == right.precision
                && left.fallback_reason == right.fallback_reason
        });
        matches
    }

    fn sort_call_hierarchy_matches(matches: &mut [CallHierarchyMatch]) {
        matches.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.path.cmp(&right.path))
                .then(left.line.cmp(&right.line))
                .then(left.column.cmp(&right.column))
                .then(left.source_symbol.cmp(&right.source_symbol))
                .then(left.target_symbol.cmp(&right.target_symbol))
                .then(left.relation.cmp(&right.relation))
                .then(left.precision.cmp(&right.precision))
                .then(left.call_path.cmp(&right.call_path))
                .then(left.call_line.cmp(&right.call_line))
                .then(left.call_column.cmp(&right.call_column))
                .then(left.call_end_line.cmp(&right.call_end_line))
                .then(left.call_end_column.cmp(&right.call_end_column))
        });
    }

    fn parse_symbol_language(value: Option<&str>) -> Result<Option<SymbolLanguage>, ErrorData> {
        let Some(value) = value else {
            return Ok(None);
        };
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(Self::invalid_params("language must not be empty", None));
        }

        let language = parse_supported_language(&normalized, LanguageCapability::StructuralSearch)
            .ok_or_else(|| {
            Self::invalid_params(
                format!("unsupported language `{value}` for structural search"),
                Some(json!({
                    "language": value,
                    "supported_languages": LanguageCapability::StructuralSearch.supported_language_names(),
                })),
            )
        })?;
        Ok(Some(language))
    }

    fn is_heuristic_call_relation(relation: RelationKind) -> bool {
        matches!(relation, RelationKind::Calls)
    }

    fn scip_candidate_directories(root: &Path) -> [PathBuf; 1] {
        [root.join(".frigg/scip")]
    }

    fn system_time_to_unix_nanos(system_time: SystemTime) -> Option<u64> {
        system_time
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
    }

    fn root_signature(file_digests: &[FileMetadataDigest]) -> String {
        let mut hasher = DeterministicSignatureHasher::new();
        for digest in file_digests {
            hasher.write_str(&digest.path.to_string_lossy());
            hasher.write_u64(digest.size_bytes);
            hasher.write_optional_u64(digest.mtime_ns);
        }
        hasher.finish_hex()
    }

    fn scip_signature(artifact_digests: &[ScipArtifactDigest]) -> String {
        let mut hasher = DeterministicSignatureHasher::new();
        for artifact in artifact_digests {
            hasher.write_str(&artifact.path.to_string_lossy());
            hasher.write_str(artifact.format.as_str());
            hasher.write_u64(artifact.size_bytes);
            hasher.write_optional_u64(artifact.mtime_ns);
        }
        hasher.finish_hex()
    }

    fn collect_scip_artifact_digests(root: &Path) -> ScipArtifactDiscovery {
        let mut artifacts = Vec::new();
        let mut candidate_directories = Vec::new();
        let mut candidate_directory_digests = Vec::new();
        for directory in Self::scip_candidate_directories(root) {
            candidate_directories.push(directory.display().to_string());
            let directory_metadata = fs::metadata(&directory).ok();
            let directory_mtime_ns = directory_metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(Self::system_time_to_unix_nanos);
            candidate_directory_digests.push(ScipCandidateDirectoryDigest {
                path: directory.clone(),
                exists: directory_metadata.is_some(),
                mtime_ns: directory_mtime_ns,
            });
            let read_dir = match fs::read_dir(&directory) {
                Ok(read_dir) => read_dir,
                Err(_) => continue,
            };

            for entry in read_dir {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };
                let path = entry.path();
                let Some(format) = ScipArtifactFormat::from_path(&path) else {
                    continue;
                };
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(_) => continue,
                };
                if !metadata.is_file() {
                    continue;
                }
                let mtime_ns = metadata
                    .modified()
                    .ok()
                    .and_then(Self::system_time_to_unix_nanos);
                artifacts.push(ScipArtifactDigest {
                    path,
                    format,
                    size_bytes: metadata.len(),
                    mtime_ns,
                });
            }
        }

        artifacts.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.size_bytes.cmp(&right.size_bytes))
                .then(left.mtime_ns.cmp(&right.mtime_ns))
        });
        artifacts.dedup_by(|left, right| left.path == right.path);
        ScipArtifactDiscovery {
            candidate_directories,
            candidate_directory_digests,
            artifact_digests: artifacts,
        }
    }

    fn ingest_precise_artifacts_for_repository(
        graph: &mut SymbolGraph,
        repository_id: &str,
        discovery: &ScipArtifactDiscovery,
        budgets: FindReferencesResourceBudgets,
    ) -> Result<PreciseIngestStats, ErrorData> {
        let artifact_digests = &discovery.artifact_digests;
        let discovered_bytes = artifact_digests
            .iter()
            .fold(0u64, |acc, digest| acc.saturating_add(digest.size_bytes));
        let mut stats = PreciseIngestStats {
            candidate_directories: discovery.candidate_directories.clone(),
            discovered_artifacts: artifact_digests
                .iter()
                .take(Self::PRECISE_DISCOVERY_SAMPLE_LIMIT)
                .map(|digest| digest.path.display().to_string())
                .collect(),
            artifacts_discovered: artifact_digests.len(),
            artifacts_discovered_bytes: discovered_bytes,
            ..PreciseIngestStats::default()
        };
        let max_artifacts = Self::usize_to_u64(budgets.scip_max_artifacts);
        if stats.artifacts_discovered > budgets.scip_max_artifacts {
            return Err(Self::find_references_resource_budget_error(
                "scip",
                "scip_artifact_count",
                "find_references SCIP artifact count exceeds configured budget",
                json!({
                    "repository_id": repository_id,
                    "actual": Self::usize_to_u64(stats.artifacts_discovered),
                    "limit": max_artifacts,
                }),
            ));
        }

        let max_artifact_bytes = Self::usize_to_u64(budgets.scip_max_artifact_bytes);
        let max_total_bytes = Self::usize_to_u64(budgets.scip_max_total_bytes);
        if discovered_bytes > max_total_bytes {
            warn!(
                repository_id,
                discovered_bytes,
                max_total_bytes,
                "scip discovery bytes exceed configured budget; precise ingest may degrade to heuristic fallback"
            );
        }

        let started_at = Instant::now();
        let max_elapsed = Duration::from_millis(budgets.scip_max_elapsed_ms);
        let mut processed_artifacts = 0usize;
        let mut processed_bytes = 0u64;

        for artifact_digest in artifact_digests {
            if started_at.elapsed() > max_elapsed {
                let elapsed_ms =
                    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                warn!(
                    repository_id,
                    actual_elapsed_ms = elapsed_ms,
                    limit_elapsed_ms = budgets.scip_max_elapsed_ms,
                    processed_artifacts,
                    bytes_processed = processed_bytes,
                    "scip processing exceeded time budget; degrading precise path to heuristic fallback"
                );
                Self::push_precise_failure_sample(
                    &mut stats,
                    "<scip-processing-budget>".to_owned(),
                    "ingest_budget_elapsed_ms",
                    format!(
                        "scip processing elapsed_ms={} exceeded limit={} after processing {} artifacts and {} bytes",
                        elapsed_ms,
                        budgets.scip_max_elapsed_ms,
                        processed_artifacts,
                        processed_bytes
                    ),
                );
                break;
            }

            if artifact_digest.size_bytes > max_artifact_bytes {
                warn!(
                    repository_id,
                    path = %artifact_digest.path.display(),
                    actual_bytes = artifact_digest.size_bytes,
                    limit_bytes = max_artifact_bytes,
                    "skipping scip artifact that exceeds per-file byte budget"
                );
                stats.artifacts_failed += 1;
                stats.artifacts_failed_bytes = stats
                    .artifacts_failed_bytes
                    .saturating_add(artifact_digest.size_bytes);
                Self::push_precise_failure_sample(
                    &mut stats,
                    artifact_digest.path.display().to_string(),
                    "artifact_budget_bytes",
                    format!(
                        "artifact bytes {} exceed configured per-file limit {}",
                        artifact_digest.size_bytes, max_artifact_bytes
                    ),
                );
                continue;
            }
            let projected_processed_bytes =
                processed_bytes.saturating_add(artifact_digest.size_bytes);
            if projected_processed_bytes > max_total_bytes {
                warn!(
                    repository_id,
                    path = %artifact_digest.path.display(),
                    projected_processed_bytes,
                    limit_bytes = max_total_bytes,
                    "skipping scip artifact because cumulative byte budget would be exceeded"
                );
                stats.artifacts_failed += 1;
                stats.artifacts_failed_bytes = stats
                    .artifacts_failed_bytes
                    .saturating_add(artifact_digest.size_bytes);
                Self::push_precise_failure_sample(
                    &mut stats,
                    artifact_digest.path.display().to_string(),
                    "artifact_budget_total_bytes",
                    format!(
                        "projected cumulative bytes {} exceed configured total limit {}",
                        projected_processed_bytes, max_total_bytes
                    ),
                );
                continue;
            }
            processed_bytes = projected_processed_bytes;

            let payload = match fs::read(&artifact_digest.path) {
                Ok(payload) => payload,
                Err(err) => {
                    warn!(
                        repository_id,
                        path = %artifact_digest.path.display(),
                        error = %err,
                        "failed to read scip artifact payload while resolving references"
                    );
                    stats.artifacts_failed += 1;
                    stats.artifacts_failed_bytes = stats
                        .artifacts_failed_bytes
                        .saturating_add(artifact_digest.size_bytes);
                    Self::push_precise_failure_sample(
                        &mut stats,
                        artifact_digest.path.display().to_string(),
                        "read_payload",
                        err.to_string(),
                    );
                    continue;
                }
            };
            let payload_bytes = Self::usize_to_u64(payload.len());
            if payload_bytes > max_artifact_bytes {
                warn!(
                    repository_id,
                    path = %artifact_digest.path.display(),
                    actual_bytes = payload_bytes,
                    limit_bytes = max_artifact_bytes,
                    "skipping scip artifact payload that exceeds per-file byte budget after read"
                );
                stats.artifacts_failed += 1;
                stats.artifacts_failed_bytes =
                    stats.artifacts_failed_bytes.saturating_add(payload_bytes);
                Self::push_precise_failure_sample(
                    &mut stats,
                    artifact_digest.path.display().to_string(),
                    "payload_budget_bytes",
                    format!(
                        "payload bytes {} exceed configured per-file limit {}",
                        payload_bytes, max_artifact_bytes
                    ),
                );
                continue;
            }

            let artifact_label = artifact_digest.path.to_string_lossy().into_owned();
            let ingest_result = match artifact_digest.format {
                ScipArtifactFormat::Json => graph.overlay_scip_json_with_budgets(
                    repository_id,
                    &artifact_label,
                    &payload,
                    ScipResourceBudgets {
                        max_payload_bytes: budgets.scip_max_artifact_bytes,
                        max_documents: budgets.scip_max_documents_per_artifact,
                        max_elapsed_ms: budgets.scip_max_elapsed_ms,
                    },
                ),
                ScipArtifactFormat::Protobuf => graph.overlay_scip_protobuf_with_budgets(
                    repository_id,
                    &artifact_label,
                    &payload,
                    ScipResourceBudgets {
                        max_payload_bytes: budgets.scip_max_artifact_bytes,
                        max_documents: budgets.scip_max_documents_per_artifact,
                        max_elapsed_ms: budgets.scip_max_elapsed_ms,
                    },
                ),
            };
            match ingest_result {
                Ok(_) => {
                    stats.artifacts_ingested += 1;
                    stats.artifacts_ingested_bytes =
                        stats.artifacts_ingested_bytes.saturating_add(payload_bytes);
                }
                Err(err) => {
                    if let ScipIngestError::ResourceBudgetExceeded { diagnostic } = &err {
                        warn!(
                            repository_id,
                            path = %artifact_digest.path.display(),
                            budget_code = diagnostic.code.as_str(),
                            actual = diagnostic.actual,
                            limit = diagnostic.limit,
                            detail = %diagnostic.message,
                            "scip ingest exceeded resource budget; degrading precise path to heuristic fallback"
                        );
                        stats.artifacts_failed += 1;
                        stats.artifacts_failed_bytes =
                            stats.artifacts_failed_bytes.saturating_add(payload_bytes);
                        Self::push_precise_failure_sample(
                            &mut stats,
                            artifact_digest.path.display().to_string(),
                            &format!("ingest_budget_{}", diagnostic.code.as_str()),
                            format!(
                                "ingest budget {} exceeded (actual={}, limit={}): {}",
                                diagnostic.code.as_str(),
                                diagnostic.actual,
                                diagnostic.limit,
                                diagnostic.message
                            ),
                        );
                        continue;
                    }
                    warn!(
                        repository_id,
                        path = %artifact_digest.path.display(),
                        error = %err,
                        "failed to ingest scip artifact while resolving references"
                    );
                    stats.artifacts_failed += 1;
                    stats.artifacts_failed_bytes =
                        stats.artifacts_failed_bytes.saturating_add(payload_bytes);
                    Self::push_precise_failure_sample(
                        &mut stats,
                        artifact_digest.path.display().to_string(),
                        "ingest_payload",
                        err.to_string(),
                    );
                }
            }
            processed_artifacts = processed_artifacts.saturating_add(1);
        }

        Ok(stats)
    }

    fn collect_repository_symbol_corpus(
        &self,
        repository_id: String,
        root: PathBuf,
    ) -> Result<Arc<RepositorySymbolCorpus>, ErrorData> {
        let mut diagnostics = RepositoryDiagnosticsSummary::default();
        let mut manifest_output = None;
        let mut source_paths = None;
        let (file_digests, manifest_token) =
            match Self::load_latest_manifest_snapshot(&root, &repository_id) {
                Some(snapshot) => {
                    let snapshot_digests = snapshot
                        .entries
                        .into_iter()
                        .map(|entry| FileMetadataDigest {
                            path: PathBuf::from(entry.path),
                            size_bytes: entry.size_bytes,
                            mtime_ns: entry.mtime_ns,
                        })
                        .collect::<Vec<_>>();
                    match validate_manifest_digests_for_root(&root, &snapshot_digests) {
                        Some(validated_digests) => {
                            let snapshot_source_paths =
                                Self::manifest_source_paths_for_digests(&validated_digests);
                            source_paths = Some(snapshot_source_paths);
                            (
                                validated_digests,
                                format!("snapshot:{}", snapshot.snapshot_id),
                            )
                        }
                        None => {
                            let live_output = ManifestBuilder::default()
                                .build_metadata_with_diagnostics(&root)
                                .map_err(Self::map_frigg_error)?;
                            let live_signature = Self::root_signature(&live_output.entries);
                            manifest_output = Some(live_output);
                            (
                                manifest_output
                                    .as_ref()
                                    .expect("live manifest output just assigned")
                                    .entries
                                    .clone(),
                                format!("live:{live_signature}"),
                            )
                        }
                    }
                }
                None => {
                    let live_output = ManifestBuilder::default()
                        .build_metadata_with_diagnostics(&root)
                        .map_err(Self::map_frigg_error)?;
                    let live_signature = Self::root_signature(&live_output.entries);
                    manifest_output = Some(live_output);
                    (
                        manifest_output
                            .as_ref()
                            .expect("live manifest output just assigned")
                            .entries
                            .clone(),
                        format!("live:{live_signature}"),
                    )
                }
            };
        if let Some(manifest_output) = &manifest_output {
            for manifest_diagnostic in &manifest_output.diagnostics {
                match manifest_diagnostic.kind {
                    ManifestDiagnosticKind::Walk => diagnostics.manifest_walk_count += 1,
                    ManifestDiagnosticKind::Read => diagnostics.manifest_read_count += 1,
                }
            }
        }
        let root_signature = Self::root_signature(&file_digests);
        let cache_key = SymbolCorpusCacheKey {
            repository_id: repository_id.clone(),
            manifest_token: manifest_token.clone(),
        };

        if let Some(cached) = self
            .symbol_corpus_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            return Ok(cached);
        }

        let mut source_paths = source_paths.unwrap_or_else(|| {
            file_digests
                .iter()
                .map(|digest| digest.path.clone())
                .filter(|path| {
                    supported_language_for_path(path, LanguageCapability::SymbolCorpus).is_some()
                })
                .collect::<Vec<_>>()
        });
        source_paths.sort();

        let SymbolExtractionOutput {
            symbols,
            diagnostics: symbol_diagnostics,
        } = extract_symbols_for_paths(&source_paths);
        diagnostics.symbol_extraction_count = symbol_diagnostics.len();
        let symbols_by_relative_path = Self::symbols_by_relative_path(&root, &symbols);
        let symbol_index_by_stable_id = Self::symbol_index_by_stable_id(&symbols);
        let symbol_indices_by_name = Self::symbol_indices_by_name(&symbols);
        let symbol_indices_by_lower_name = Self::symbol_indices_by_lower_name(&symbols);
        let mut php_evidence_by_relative_path = BTreeMap::new();
        let mut blade_evidence_by_relative_path = BTreeMap::new();
        let mut canonical_symbol_name_by_stable_id = BTreeMap::new();

        for path in &source_paths {
            let relative_path = Self::relative_display_path(&root, path);
            let file_symbols = symbols_by_relative_path
                .get(&relative_path)
                .into_iter()
                .flatten()
                .map(|index| symbols[*index].clone())
                .collect::<Vec<_>>();
            if file_symbols.is_empty() {
                continue;
            }
            let Ok(source) = fs::read_to_string(path) else {
                continue;
            };
            match supported_language_for_path(path, LanguageCapability::SymbolCorpus) {
                Some(SymbolLanguage::Php) => {
                    let Ok(evidence) =
                        extract_php_source_evidence_from_source(path, &source, &file_symbols)
                    else {
                        continue;
                    };
                    canonical_symbol_name_by_stable_id
                        .extend(evidence.canonical_names_by_stable_id.clone());
                    php_evidence_by_relative_path.insert(relative_path, evidence);
                }
                Some(SymbolLanguage::Blade) => {
                    let mut evidence =
                        extract_blade_source_evidence_from_source(path, &source, &file_symbols);
                    mark_local_flux_overlays(&mut evidence, &symbols, &symbol_indices_by_name);
                    blade_evidence_by_relative_path.insert(relative_path, evidence);
                }
                _ => {}
            }
        }
        let symbol_indices_by_canonical_name = Self::symbol_indices_by_canonical_name(
            &symbol_index_by_stable_id,
            &canonical_symbol_name_by_stable_id,
        );
        let symbol_indices_by_lower_canonical_name = Self::symbol_indices_by_lower_canonical_name(
            &symbol_index_by_stable_id,
            &canonical_symbol_name_by_stable_id,
        );

        let corpus = Arc::new(RepositorySymbolCorpus {
            repository_id: repository_id.clone(),
            root,
            root_signature: root_signature.clone(),
            source_paths,
            symbols,
            symbols_by_relative_path,
            symbol_index_by_stable_id,
            symbol_indices_by_name,
            symbol_indices_by_lower_name,
            canonical_symbol_name_by_stable_id,
            symbol_indices_by_canonical_name,
            symbol_indices_by_lower_canonical_name,
            php_evidence_by_relative_path,
            blade_evidence_by_relative_path,
            diagnostics,
        });

        let mut cache = self
            .symbol_corpus_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|key, _| {
            key.repository_id != repository_id || key.manifest_token == manifest_token
        });
        cache.insert(cache_key, corpus.clone());

        Ok(corpus)
    }

    fn load_latest_manifest_snapshot(
        root: &Path,
        repository_id: &str,
    ) -> Option<crate::storage::RepositoryManifestSnapshot> {
        let db_path = resolve_provenance_db_path(root).ok()?;
        if !db_path.exists() {
            return None;
        }
        let storage = Storage::new(db_path);
        storage
            .load_latest_manifest_for_repository(repository_id)
            .ok()?
    }

    fn current_root_signature_for_repository(root: &Path, repository_id: &str) -> Option<String> {
        match Self::load_latest_manifest_snapshot(root, repository_id) {
            Some(snapshot) => {
                let snapshot_digests = snapshot
                    .entries
                    .into_iter()
                    .map(|entry| FileMetadataDigest {
                        path: PathBuf::from(entry.path),
                        size_bytes: entry.size_bytes,
                        mtime_ns: entry.mtime_ns,
                    })
                    .collect::<Vec<_>>();
                if let Some(validated_digests) = validate_manifest_digests_for_root(root, &snapshot_digests) {
                    return Some(Self::root_signature(&validated_digests));
                }
            }
            None => {}
        }

        ManifestBuilder::default()
            .build_metadata_with_diagnostics(root)
            .ok()
            .map(|output| Self::root_signature(&output.entries))
    }

    fn manifest_source_paths_for_digests(file_digests: &[FileMetadataDigest]) -> Vec<PathBuf> {
        let mut source_paths = Vec::new();
        for digest in file_digests {
            if supported_language_for_path(&digest.path, LanguageCapability::SymbolCorpus).is_some()
            {
                source_paths.push(digest.path.clone());
            }
        }
        source_paths
    }

    fn symbols_by_relative_path(
        root: &Path,
        symbols: &[SymbolDefinition],
    ) -> BTreeMap<String, Vec<usize>> {
        let mut symbols_by_relative_path = BTreeMap::new();
        for (index, symbol) in symbols.iter().enumerate() {
            symbols_by_relative_path
                .entry(Self::relative_display_path(root, &symbol.path))
                .or_insert_with(Vec::new)
                .push(index);
        }
        for indices in symbols_by_relative_path.values_mut() {
            indices.sort_by(|left, right| {
                symbols[*left]
                    .line
                    .cmp(&symbols[*right].line)
                    .then(
                        symbols[*left]
                            .span
                            .start_column
                            .cmp(&symbols[*right].span.start_column),
                    )
                    .then(symbols[*left].stable_id.cmp(&symbols[*right].stable_id))
            });
        }
        symbols_by_relative_path
    }

    fn symbol_index_by_stable_id(symbols: &[SymbolDefinition]) -> BTreeMap<String, usize> {
        symbols
            .iter()
            .enumerate()
            .map(|(index, symbol)| (symbol.stable_id.clone(), index))
            .collect()
    }

    fn symbol_indices_by_name(symbols: &[SymbolDefinition]) -> BTreeMap<String, Vec<usize>> {
        let mut symbol_indices_by_name = BTreeMap::new();
        for (index, symbol) in symbols.iter().enumerate() {
            symbol_indices_by_name
                .entry(symbol.name.clone())
                .or_insert_with(Vec::new)
                .push(index);
        }
        symbol_indices_by_name
    }

    fn symbol_indices_by_lower_name(symbols: &[SymbolDefinition]) -> BTreeMap<String, Vec<usize>> {
        let mut symbol_indices_by_lower_name = BTreeMap::new();
        for (index, symbol) in symbols.iter().enumerate() {
            symbol_indices_by_lower_name
                .entry(symbol.name.to_ascii_lowercase())
                .or_insert_with(Vec::new)
                .push(index);
        }
        symbol_indices_by_lower_name
    }

    fn symbol_indices_by_canonical_name(
        symbol_index_by_stable_id: &BTreeMap<String, usize>,
        canonical_symbol_name_by_stable_id: &BTreeMap<String, String>,
    ) -> BTreeMap<String, Vec<usize>> {
        let mut symbol_indices_by_canonical_name = BTreeMap::new();
        for (stable_id, canonical_name) in canonical_symbol_name_by_stable_id {
            let Some(symbol_index) = symbol_index_by_stable_id.get(stable_id).copied() else {
                continue;
            };
            symbol_indices_by_canonical_name
                .entry(canonical_name.clone())
                .or_insert_with(Vec::new)
                .push(symbol_index);
        }
        symbol_indices_by_canonical_name
    }

    fn symbol_indices_by_lower_canonical_name(
        symbol_index_by_stable_id: &BTreeMap<String, usize>,
        canonical_symbol_name_by_stable_id: &BTreeMap<String, String>,
    ) -> BTreeMap<String, Vec<usize>> {
        let mut symbol_indices_by_lower_canonical_name = BTreeMap::new();
        for (stable_id, canonical_name) in canonical_symbol_name_by_stable_id {
            let Some(symbol_index) = symbol_index_by_stable_id.get(stable_id).copied() else {
                continue;
            };
            symbol_indices_by_lower_canonical_name
                .entry(canonical_name.to_ascii_lowercase())
                .or_insert_with(Vec::new)
                .push(symbol_index);
        }
        symbol_indices_by_lower_canonical_name
    }

    fn try_reuse_cached_precise_graph(
        &self,
        corpus: &RepositorySymbolCorpus,
    ) -> Option<CachedPreciseGraph> {
        let cached = self
            .latest_precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&corpus.repository_id)
            .cloned()?;
        if cached.corpus_signature != corpus.root_signature {
            return None;
        }
        if !Self::cached_scip_discovery_is_current(&corpus.root, &cached.discovery) {
            return None;
        }
        Some((*cached).clone())
    }

    fn try_reuse_latest_precise_graph_for_repository(
        &self,
        repository_id: &str,
        root: &Path,
    ) -> Option<CachedPreciseGraph> {
        let current_root_signature =
            Self::current_root_signature_for_repository(root, repository_id)?;
        let cached = self
            .latest_precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(repository_id)
            .cloned()?;
        if cached.corpus_signature != current_root_signature {
            return None;
        }
        if !Self::cached_scip_discovery_is_current(root, &cached.discovery) {
            return None;
        }
        Some((*cached).clone())
    }

    fn cached_scip_discovery_is_current(root: &Path, discovery: &ScipArtifactDiscovery) -> bool {
        let expected_directories = Self::scip_candidate_directories(root);
        if discovery.candidate_directory_digests.len() != expected_directories.len() {
            return false;
        }

        for (expected_path, cached_digest) in expected_directories
            .iter()
            .zip(discovery.candidate_directory_digests.iter())
        {
            if cached_digest.path != *expected_path {
                return false;
            }
            let metadata = fs::metadata(expected_path).ok();
            let exists = metadata.is_some();
            let mtime_ns = metadata
                .as_ref()
                .and_then(|value| value.modified().ok())
                .and_then(Self::system_time_to_unix_nanos);
            if cached_digest.exists != exists || cached_digest.mtime_ns != mtime_ns {
                return false;
            }
        }

        discovery.artifact_digests.iter().all(|artifact| {
            let metadata = match fs::metadata(&artifact.path) {
                Ok(metadata) => metadata,
                Err(_) => return false,
            };
            metadata.is_file()
                && metadata.len() == artifact.size_bytes
                && metadata
                    .modified()
                    .ok()
                    .and_then(Self::system_time_to_unix_nanos)
                    == artifact.mtime_ns
        })
    }

    fn precise_graph_for_corpus(
        &self,
        corpus: &RepositorySymbolCorpus,
        budgets: FindReferencesResourceBudgets,
    ) -> Result<CachedPreciseGraph, ErrorData> {
        if let Some(cached) = self.try_reuse_cached_precise_graph(corpus) {
            return Ok(cached);
        }

        let discovery = Self::collect_scip_artifact_digests(&corpus.root);
        let scip_signature = Self::scip_signature(&discovery.artifact_digests);
        let cache_key = PreciseGraphCacheKey {
            repository_id: corpus.repository_id.clone(),
            scip_signature: scip_signature.clone(),
            corpus_signature: corpus.root_signature.clone(),
        };

        if let Some(cached) = self
            .precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            self.latest_precise_graph_cache
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(corpus.repository_id.clone(), cached.clone());
            return Ok((*cached).clone());
        }

        let mut graph = SymbolGraph::default();
        register_symbol_definitions(&mut graph, &corpus.repository_id, &corpus.symbols);
        Self::register_php_declaration_relations(&mut graph, corpus);
        Self::register_php_target_evidence_relations(&mut graph, corpus);
        Self::register_blade_relation_evidence(&mut graph, corpus);
        let ingest_stats = Self::ingest_precise_artifacts_for_repository(
            &mut graph,
            &corpus.repository_id,
            &discovery,
            budgets,
        )?;
        let coverage_mode = Self::precise_coverage_mode(&ingest_stats);
        if coverage_mode == PreciseCoverageMode::Partial {
            warn!(
                repository_id = corpus.repository_id,
                artifacts_ingested = ingest_stats.artifacts_ingested,
                artifacts_failed = ingest_stats.artifacts_failed,
                "retaining partial precise graph because some SCIP artifacts ingested successfully"
            );
        }
        if coverage_mode == PreciseCoverageMode::None && ingest_stats.artifacts_failed > 0 {
            warn!(
                repository_id = corpus.repository_id,
                artifacts_ingested = ingest_stats.artifacts_ingested,
                artifacts_failed = ingest_stats.artifacts_failed,
                "precise graph has no usable artifact data after SCIP ingest failures"
            );
        }
        let cached_graph = CachedPreciseGraph {
            graph: Arc::new(graph),
            ingest_stats,
            corpus_signature: corpus.root_signature.clone(),
            discovery: discovery.clone(),
            coverage_mode,
        };

        let mut cache = self
            .precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|key, _| {
            key.repository_id != corpus.repository_id
                || (key.scip_signature == scip_signature
                    && key.corpus_signature == corpus.root_signature)
        });
        let cached_graph = Arc::new(cached_graph);
        cache.insert(cache_key, cached_graph.clone());
        self.latest_precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(corpus.repository_id.clone(), cached_graph.clone());

        Ok((*cached_graph).clone())
    }

    fn register_php_declaration_relations(
        graph: &mut SymbolGraph,
        corpus: &RepositorySymbolCorpus,
    ) {
        for path in &corpus.source_paths {
            let relative_path = Self::relative_display_path(&corpus.root, path);
            let edges = match php_declaration_relation_edges_for_file(
                &relative_path,
                path,
                &corpus.symbols,
                &corpus.symbols_by_relative_path,
                Some(&corpus.symbol_indices_by_name),
                Some(&corpus.symbol_indices_by_lower_name),
            ) {
                Ok(edges) => edges,
                Err(err) => {
                    warn!(
                        repository_id = corpus.repository_id,
                        path = %path.display(),
                        error = %err,
                        "failed to build php declaration relations while building heuristic graph"
                    );
                    continue;
                }
            };

            for (source_symbol_index, target_symbol_index, relation) in edges {
                let source_symbol = &corpus.symbols[source_symbol_index];
                let target_symbol = &corpus.symbols[target_symbol_index];
                if source_symbol.stable_id == target_symbol.stable_id {
                    continue;
                }

                let _ = graph.add_relation(
                    &source_symbol.stable_id,
                    &target_symbol.stable_id,
                    relation,
                );
            }
        }
    }

    fn register_php_target_evidence_relations(
        graph: &mut SymbolGraph,
        corpus: &RepositorySymbolCorpus,
    ) {
        for evidence in corpus.php_evidence_by_relative_path.values() {
            for (source_symbol_index, target_symbol_index, relation) in
                resolve_php_target_evidence_edges(
                    &corpus.symbols,
                    &corpus.symbol_index_by_stable_id,
                    &corpus.symbol_indices_by_canonical_name,
                    &corpus.symbol_indices_by_lower_canonical_name,
                    evidence,
                )
            {
                let source_symbol = &corpus.symbols[source_symbol_index];
                let target_symbol = &corpus.symbols[target_symbol_index];
                if source_symbol.stable_id == target_symbol.stable_id {
                    continue;
                }
                let _ = graph.add_relation(
                    &source_symbol.stable_id,
                    &target_symbol.stable_id,
                    relation,
                );
            }
        }
    }

    fn register_blade_relation_evidence(graph: &mut SymbolGraph, corpus: &RepositorySymbolCorpus) {
        for evidence in corpus.blade_evidence_by_relative_path.values() {
            for (source_symbol_index, target_symbol_index, relation) in
                resolve_blade_relation_evidence_edges(
                    &corpus.symbols,
                    &corpus.symbol_index_by_stable_id,
                    &corpus.symbol_indices_by_name,
                    &corpus.symbol_indices_by_lower_name,
                    evidence,
                )
            {
                let source_symbol = &corpus.symbols[source_symbol_index];
                let target_symbol = &corpus.symbols[target_symbol_index];
                if source_symbol.stable_id == target_symbol.stable_id {
                    continue;
                }
                let _ = graph.add_relation(
                    &source_symbol.stable_id,
                    &target_symbol.stable_id,
                    relation,
                );
            }
        }
    }

    fn collect_repository_symbol_corpora(
        &self,
        repository_id: Option<&str>,
    ) -> Result<Vec<Arc<RepositorySymbolCorpus>>, ErrorData> {
        let mut corpora = self
            .roots_for_repository(repository_id)?
            .into_iter()
            .map(|(repository_id, root)| self.collect_repository_symbol_corpus(repository_id, root))
            .collect::<Result<Vec<_>, ErrorData>>()?;

        corpora.sort_by(|left, right| left.repository_id.cmp(&right.repository_id));
        Ok(corpora)
    }

    fn bounded_text(value: &str) -> String {
        if value.chars().count() <= Self::PROVENANCE_MAX_TEXT_CHARS {
            return value.to_owned();
        }
        let mut bounded = value
            .chars()
            .take(Self::PROVENANCE_MAX_TEXT_CHARS)
            .collect::<String>();
        bounded.push_str("...");
        bounded
    }

    fn default_provenance_target(&self) -> Option<(String, PathBuf)> {
        self.current_workspace()
            .into_iter()
            .chain(self.attached_workspaces())
            .map(|workspace| (workspace.repository_id, workspace.root))
            .min_by(|left, right| left.0.cmp(&right.0))
    }

    fn provenance_target_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Option<(String, PathBuf)> {
        match repository_id {
            Some(repository_id) => self
                .attached_workspaces()
                .into_iter()
                .find(|workspace| workspace.repository_id == repository_id)
                .map(|workspace| (workspace.repository_id, workspace.root)),
            None => self.default_provenance_target(),
        }
    }

    fn provenance_error_code(error: &ErrorData) -> String {
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code"))
            .and_then(|value| value.as_str())
            .unwrap_or("missing_error_code")
            .to_owned()
    }

    fn provenance_outcome<T>(result: &Result<Json<T>, ErrorData>) -> Value {
        match result {
            Ok(_) => json!({
                "status": "ok",
            }),
            Err(error) => json!({
                "status": "error",
                "error_code": Self::provenance_error_code(error),
                "mcp_error_code": error.code,
            }),
        }
    }

    fn provenance_storage_for_target(
        &self,
        tool_name: &str,
        target_repository_id: &str,
        db_path: &Path,
    ) -> Result<Arc<Storage>, ErrorData> {
        let cache_key = ProvenanceStorageCacheKey {
            repository_id: target_repository_id.to_owned(),
            db_path: db_path.to_path_buf(),
        };
        if let Some(storage) = self
            .provenance_storage_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            return Ok(storage);
        }

        let storage = Arc::new(Storage::new(db_path));
        if let Err(err) = storage.initialize() {
            return Err(Self::provenance_persistence_error(
                ProvenancePersistenceStage::InitializeStorage,
                tool_name,
                Some(target_repository_id),
                Some(db_path),
                err,
            ));
        }

        let mut cache = self
            .provenance_storage_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(storage) = cache.get(&cache_key).cloned() {
            return Ok(storage);
        }
        cache.insert(cache_key, storage.clone());
        Ok(storage)
    }

    fn record_provenance_with_outcome(
        &self,
        tool_name: &str,
        repository_hint: Option<&str>,
        params: Value,
        source_refs: Value,
        outcome: Value,
    ) -> Result<(), ErrorData> {
        let Some((target_repository_id, target_root)) =
            self.provenance_target_for_repository(repository_hint)
        else {
            return Ok(());
        };

        let db_path = match ensure_provenance_db_parent_dir(&target_root) {
            Ok(path) => path,
            Err(err) => {
                return Err(Self::provenance_persistence_error(
                    ProvenancePersistenceStage::ResolveStoragePath,
                    tool_name,
                    Some(&target_repository_id),
                    None,
                    err,
                ));
            }
        };

        let storage =
            self.provenance_storage_for_target(tool_name, &target_repository_id, &db_path)?;

        let payload = json!({
            "tool_name": tool_name,
            "params": params,
            "source_refs": source_refs,
            "outcome": outcome,
            "target_repository_id": target_repository_id,
        });
        let trace_id = Storage::new_provenance_trace_id(tool_name);
        if let Err(err) = storage.append_provenance_event(&trace_id, tool_name, &payload) {
            return Err(Self::provenance_persistence_error(
                ProvenancePersistenceStage::AppendEvent,
                tool_name,
                Some(&target_repository_id),
                Some(&db_path),
                err,
            ));
        }

        Ok(())
    }

    async fn record_provenance_blocking<T>(
        &self,
        tool_name: &'static str,
        repository_hint: Option<&str>,
        params: Value,
        source_refs: Value,
        result: &Result<Json<T>, ErrorData>,
    ) -> Result<(), ErrorData> {
        let server = self.clone();
        let repository_hint = repository_hint.map(str::to_owned);
        let outcome = Self::provenance_outcome(result);
        Self::run_blocking_task("record_provenance", move || {
            server.record_provenance_with_outcome(
                tool_name,
                repository_hint.as_deref(),
                params,
                source_refs,
                outcome,
            )
        })
        .await?
    }

    fn finalize_with_provenance<T>(
        &self,
        tool_name: &str,
        result: Result<Json<T>, ErrorData>,
        provenance_result: Result<(), ErrorData>,
    ) -> Result<Json<T>, ErrorData> {
        match provenance_result {
            Ok(_) => result,
            Err(provenance_error) if self.provenance_best_effort => {
                warn!(
                    tool_name,
                    error = %provenance_error.message,
                    "provenance persistence failed in best-effort mode"
                );
                result
            }
            Err(provenance_error) => match result {
                Ok(_) => Err(provenance_error),
                Err(original_error) => {
                    warn!(
                        tool_name,
                        original_error_code = ?original_error.code,
                        provenance_error_code = ?provenance_error.code,
                        "provenance persistence failed but original request already returned typed error"
                    );
                    Err(original_error)
                }
            },
        }
    }

    pub fn new(config: FriggConfig) -> Self {
        Self::new_with_provenance_best_effort(config, Self::provenance_best_effort_from_env())
    }

    pub fn new_with_runtime_options(
        config: FriggConfig,
        provenance_best_effort: bool,
        enable_extended_tools: bool,
    ) -> Self {
        let workspace_registry = WorkspaceRegistry::from_startup_repositories(
            config.repositories().into_iter().map(|repository| {
                (
                    repository.repository_id.0,
                    repository.display_name,
                    repository.root_path,
                )
            }),
        );
        Self {
            config: Arc::new(config),
            tool_router: Self::filtered_tool_router(enable_extended_tools),
            workspace_registry: Arc::new(RwLock::new(workspace_registry)),
            session_default_repository_id: Arc::new(RwLock::new(None)),
            symbol_corpus_cache: Arc::new(RwLock::new(BTreeMap::new())),
            precise_graph_cache: Arc::new(RwLock::new(BTreeMap::new())),
            latest_precise_graph_cache: Arc::new(RwLock::new(BTreeMap::new())),
            provenance_storage_cache: Arc::new(RwLock::new(BTreeMap::new())),
            provenance_best_effort,
        }
    }

    pub fn new_with_provenance_best_effort(
        config: FriggConfig,
        provenance_best_effort: bool,
    ) -> Self {
        let enable_extended_tools =
            active_runtime_tool_surface_profile() == ToolSurfaceProfile::Extended;
        Self::new_with_runtime_options(config, provenance_best_effort, enable_extended_tools)
    }

    pub fn runtime_registered_tool_names(&self) -> Vec<String> {
        self.tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect::<Vec<_>>()
    }

    pub fn runtime_tool_surface_parity(
        &self,
        profile: ToolSurfaceProfile,
    ) -> ToolSurfaceParityDiff {
        let runtime_names = self.runtime_registered_tool_names();
        diff_runtime_against_profile_manifest(profile, &runtime_names)
    }

    fn clone_for_new_session(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            tool_router: self.tool_router.clone(),
            workspace_registry: Arc::clone(&self.workspace_registry),
            session_default_repository_id: Arc::new(RwLock::new(None)),
            symbol_corpus_cache: Arc::clone(&self.symbol_corpus_cache),
            precise_graph_cache: Arc::clone(&self.precise_graph_cache),
            latest_precise_graph_cache: Arc::clone(&self.latest_precise_graph_cache),
            provenance_storage_cache: Arc::clone(&self.provenance_storage_cache),
            provenance_best_effort: self.provenance_best_effort,
        }
    }

    pub async fn serve_stdio(self) -> Result<(), rmcp::RmcpError> {
        let service = self.serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
        Ok(())
    }

    pub fn streamable_http_service(self, config: StreamableHttpServerConfig) -> FriggMcpService {
        StreamableHttpService::new(
            move || Ok(self.clone_for_new_session()),
            Arc::new(LocalSessionManager::default()),
            config,
        )
    }

    pub fn auto_attach_stdio_default_workspace_from_current_dir(&self) -> std::io::Result<()> {
        if !self.attached_workspaces().is_empty() {
            return Ok(());
        }

        let current_dir = std::env::current_dir()?;
        self.attach_workspace_internal(&current_dir, true, WorkspaceResolveMode::GitRoot)
            .map(|_| ())
            .map_err(|error| std::io::Error::other(error.message))
    }

    fn attached_workspaces(&self) -> Vec<AttachedWorkspace> {
        self.workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .attached_workspaces()
    }

    fn current_repository_id(&self) -> Option<String> {
        self.session_default_repository_id
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set_current_repository_id(&self, repository_id: Option<String>) {
        let mut current = self
            .session_default_repository_id
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = repository_id;
    }

    fn current_workspace(&self) -> Option<AttachedWorkspace> {
        let repository_id = self.current_repository_id()?;
        self.workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_by_repository_id(&repository_id)
    }

    fn no_attached_workspaces_error(action: &str) -> ErrorData {
        Self::resource_not_found(
            "no repositories are attached for this session",
            Some(json!({
                "attached_repositories": [],
                "action": action,
                "hint": "call workspace_attach first or provide --workspace-root at startup",
            })),
        )
    }

    fn effective_repository_id(&self, repository_id: Option<&str>) -> Option<String> {
        repository_id
            .map(str::to_owned)
            .or_else(|| self.current_repository_id())
    }

    fn attached_workspaces_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Result<Vec<AttachedWorkspace>, ErrorData> {
        let registry = self
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(repository_id) = self.effective_repository_id(repository_id) {
            if let Some(workspace) = registry.workspace_by_repository_id(&repository_id) {
                return Ok(vec![workspace]);
            }
            return Err(Self::resource_not_found(
                "repository_id not found",
                Some(json!({ "repository_id": repository_id })),
            ));
        }

        let workspaces = registry.attached_workspaces();
        if workspaces.is_empty() {
            return Err(Self::no_attached_workspaces_error("workspace_attach"));
        }

        Ok(workspaces)
    }

    fn roots_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Result<Vec<(String, PathBuf)>, ErrorData> {
        Ok(self
            .attached_workspaces_for_repository(repository_id)?
            .into_iter()
            .map(|workspace| (workspace.repository_id, workspace.root))
            .collect())
    }

    fn effective_attach_directory(path: &Path) -> Result<PathBuf, ErrorData> {
        if path.exists() {
            let metadata = fs::metadata(path).map_err(|err| {
                Self::invalid_params(
                    format!("failed to inspect attach path {}: {err}", path.display()),
                    Some(json!({ "path": path.display().to_string() })),
                )
            })?;
            let directory = if metadata.is_dir() {
                path.to_path_buf()
            } else {
                path.parent().map(Path::to_path_buf).ok_or_else(|| {
                    Self::invalid_params(
                        "workspace_attach path has no parent directory",
                        Some(json!({ "path": path.display().to_string() })),
                    )
                })?
            };
            return directory.canonicalize().map_err(|err| {
                Self::invalid_params(
                    format!(
                        "failed to canonicalize attach path {}: {err}",
                        directory.display()
                    ),
                    Some(json!({ "path": path.display().to_string() })),
                )
            });
        }

        Self::canonicalize_existing_ancestor(path)?.ok_or_else(|| {
            Self::invalid_params(
                "workspace_attach path does not exist and has no existing ancestor",
                Some(json!({ "path": path.display().to_string() })),
            )
        })
    }

    fn find_git_root(start: &Path) -> Option<PathBuf> {
        start.ancestors().find_map(|ancestor| {
            ancestor
                .join(".git")
                .exists()
                .then(|| ancestor.to_path_buf())
        })
    }

    fn workspace_storage_summary(workspace: &AttachedWorkspace) -> WorkspaceStorageSummary {
        if !workspace.db_path.is_file() {
            return WorkspaceStorageSummary {
                db_path: workspace.db_path.display().to_string(),
                exists: false,
                initialized: false,
                index_state: WorkspaceStorageIndexState::MissingDb,
                error: None,
            };
        }

        let storage = Storage::new(&workspace.db_path);
        match storage.schema_version() {
            Ok(0) => WorkspaceStorageSummary {
                db_path: workspace.db_path.display().to_string(),
                exists: true,
                initialized: false,
                index_state: WorkspaceStorageIndexState::Uninitialized,
                error: None,
            },
            Ok(_) => match storage.verify() {
                Ok(_) => {
                    match storage.load_latest_manifest_for_repository(&workspace.repository_id) {
                        Ok(Some(_)) => WorkspaceStorageSummary {
                            db_path: workspace.db_path.display().to_string(),
                            exists: true,
                            initialized: true,
                            index_state: WorkspaceStorageIndexState::Ready,
                            error: None,
                        },
                        Ok(None) => WorkspaceStorageSummary {
                            db_path: workspace.db_path.display().to_string(),
                            exists: true,
                            initialized: true,
                            index_state: WorkspaceStorageIndexState::Uninitialized,
                            error: None,
                        },
                        Err(err) => WorkspaceStorageSummary {
                            db_path: workspace.db_path.display().to_string(),
                            exists: true,
                            initialized: true,
                            index_state: WorkspaceStorageIndexState::Error,
                            error: Some(err.to_string()),
                        },
                    }
                }
                Err(err) => WorkspaceStorageSummary {
                    db_path: workspace.db_path.display().to_string(),
                    exists: true,
                    initialized: true,
                    index_state: WorkspaceStorageIndexState::Error,
                    error: Some(err.to_string()),
                },
            },
            Err(err) => WorkspaceStorageSummary {
                db_path: workspace.db_path.display().to_string(),
                exists: true,
                initialized: false,
                index_state: WorkspaceStorageIndexState::Error,
                error: Some(err.to_string()),
            },
        }
    }

    fn repository_summary(&self, workspace: &AttachedWorkspace) -> RepositorySummary {
        let storage = Self::workspace_storage_summary(workspace);
        let health = self.workspace_index_health_summary(workspace, &storage);
        RepositorySummary {
            repository_id: workspace.repository_id.clone(),
            display_name: workspace.display_name.clone(),
            root_path: workspace.root.display().to_string(),
            storage: Some(storage),
            health: Some(health),
        }
    }

    fn workspace_index_health_summary(
        &self,
        workspace: &AttachedWorkspace,
        storage: &WorkspaceStorageSummary,
    ) -> WorkspaceIndexHealthSummary {
        WorkspaceIndexHealthSummary {
            lexical: self.workspace_lexical_index_summary(workspace, storage),
            semantic: self.workspace_semantic_index_summary(workspace, storage),
            scip: self.workspace_scip_index_summary(workspace),
        }
    }

    fn workspace_lexical_index_summary(
        &self,
        workspace: &AttachedWorkspace,
        storage: &WorkspaceStorageSummary,
    ) -> WorkspaceIndexComponentSummary {
        if let Some(summary) = Self::storage_error_health_summary(storage) {
            return summary;
        }

        let Some(snapshot) =
            Self::load_latest_manifest_snapshot(&workspace.root, &workspace.repository_id)
        else {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Missing,
                reason: Some("missing_manifest_snapshot".to_owned()),
                snapshot_id: None,
                compatible_snapshot_id: None,
                provider: None,
                model: None,
                artifact_count: None,
            };
        };
        let snapshot_id = snapshot.snapshot_id.clone();
        let snapshot_digests = snapshot
            .entries
            .iter()
            .map(|entry| FileMetadataDigest {
                path: PathBuf::from(&entry.path),
                size_bytes: entry.size_bytes,
                mtime_ns: entry.mtime_ns,
            })
            .collect::<Vec<_>>();
        let state =
            if validate_manifest_digests_for_root(&workspace.root, &snapshot_digests).is_some() {
                WorkspaceIndexComponentState::Ready
            } else {
                WorkspaceIndexComponentState::Stale
            };
        let reason = match state {
            WorkspaceIndexComponentState::Ready => None,
            WorkspaceIndexComponentState::Stale => Some("stale_manifest_snapshot".to_owned()),
            _ => None,
        };
        WorkspaceIndexComponentSummary {
            state,
            reason,
            snapshot_id: Some(snapshot_id),
            compatible_snapshot_id: None,
            provider: None,
            model: None,
            artifact_count: Some(snapshot.entries.len()),
        }
    }

    fn workspace_semantic_index_summary(
        &self,
        workspace: &AttachedWorkspace,
        storage: &WorkspaceStorageSummary,
    ) -> WorkspaceIndexComponentSummary {
        if !self.config.semantic_runtime.enabled {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Disabled,
                reason: Some("semantic_runtime_disabled".to_owned()),
                snapshot_id: None,
                compatible_snapshot_id: None,
                provider: None,
                model: None,
                artifact_count: None,
            };
        }

        let provider = self
            .config
            .semantic_runtime
            .provider
            .map(|value| value.as_str().to_owned());
        let model = self
            .config
            .semantic_runtime
            .normalized_model()
            .map(ToOwned::to_owned);
        if provider.is_none() || model.is_none() {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Error,
                reason: Some("semantic_runtime_invalid_config".to_owned()),
                snapshot_id: None,
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: None,
            };
        }
        if let Some(summary) = Self::storage_error_health_summary(storage) {
            return WorkspaceIndexComponentSummary {
                provider,
                model,
                ..summary
            };
        }

        let Some(snapshot) =
            Self::load_latest_manifest_snapshot(&workspace.root, &workspace.repository_id)
        else {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Missing,
                reason: Some("missing_manifest_snapshot".to_owned()),
                snapshot_id: None,
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: None,
            };
        };
        let snapshot_id = snapshot.snapshot_id.clone();
        let snapshot_digests = snapshot
            .entries
            .iter()
            .map(|entry| FileMetadataDigest {
                path: PathBuf::from(&entry.path),
                size_bytes: entry.size_bytes,
                mtime_ns: entry.mtime_ns,
            })
            .collect::<Vec<_>>();
        if validate_manifest_digests_for_root(&workspace.root, &snapshot_digests).is_none() {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Stale,
                reason: Some("stale_manifest_snapshot".to_owned()),
                snapshot_id: Some(snapshot_id),
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: None,
            };
        }
        let has_semantic_eligible_entries = snapshot
            .entries
            .iter()
            .any(|entry| semantic_chunk_language_for_path(Path::new(&entry.path)).is_some());
        if !has_semantic_eligible_entries {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Ready,
                reason: Some("manifest_valid_no_semantic_eligible_entries".to_owned()),
                snapshot_id: Some(snapshot_id),
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: Some(0),
            };
        }

        let storage_reader = Storage::new(&workspace.db_path);
        let provider_ref = provider
            .as_deref()
            .expect("semantic provider should exist after config validation");
        let model_ref = model
            .as_deref()
            .expect("semantic model should exist after config validation");
        match storage_reader.has_semantic_embeddings_for_repository_snapshot_model(
            &workspace.repository_id,
            &snapshot_id,
            provider_ref,
            model_ref,
        ) {
            Ok(true) => WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Ready,
                reason: None,
                snapshot_id: Some(snapshot_id.clone()),
                compatible_snapshot_id: None,
                provider: provider.clone(),
                model: model.clone(),
                artifact_count: storage_reader
                    .count_semantic_embeddings_for_repository_snapshot_model(
                        &workspace.repository_id,
                        &snapshot_id,
                        provider_ref,
                        model_ref,
                    )
                    .ok(),
            },
            Ok(false) => {
                let compatible_snapshot_id = storage_reader
                    .load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
                        &workspace.repository_id,
                        provider_ref,
                        model_ref,
                    )
                    .ok()
                    .flatten();
                WorkspaceIndexComponentSummary {
                    state: if compatible_snapshot_id.is_some() {
                        WorkspaceIndexComponentState::Stale
                    } else {
                        WorkspaceIndexComponentState::Missing
                    },
                    reason: Some("semantic_snapshot_missing_for_active_model".to_owned()),
                    snapshot_id: Some(snapshot_id),
                    compatible_snapshot_id: compatible_snapshot_id.clone(),
                    provider: provider.clone(),
                    model: model.clone(),
                    artifact_count: compatible_snapshot_id.as_ref().and_then(
                        |fallback_snapshot_id| {
                            storage_reader
                                .count_semantic_embeddings_for_repository_snapshot_model(
                                    &workspace.repository_id,
                                    fallback_snapshot_id,
                                    provider_ref,
                                    model_ref,
                                )
                                .ok()
                        },
                    ),
                }
            }
            Err(err) => WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Error,
                reason: Some(err.to_string()),
                snapshot_id: Some(snapshot_id),
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: None,
            },
        }
    }

    fn workspace_scip_index_summary(
        &self,
        workspace: &AttachedWorkspace,
    ) -> WorkspaceIndexComponentSummary {
        let discovery = Self::collect_scip_artifact_digests(&workspace.root);
        let artifact_count = discovery.artifact_digests.len();
        WorkspaceIndexComponentSummary {
            state: if artifact_count == 0 {
                WorkspaceIndexComponentState::Missing
            } else {
                WorkspaceIndexComponentState::Ready
            },
            reason: if artifact_count == 0 {
                Some("no_scip_artifacts_discovered".to_owned())
            } else {
                None
            },
            snapshot_id: None,
            compatible_snapshot_id: None,
            provider: None,
            model: None,
            artifact_count: Some(artifact_count),
        }
    }

    fn workspace_semantic_refresh_plan(
        &self,
        workspace: &AttachedWorkspace,
    ) -> Option<WorkspaceSemanticRefreshPlan> {
        if !self.config.semantic_runtime.enabled {
            return None;
        }

        let provider = self.config.semantic_runtime.provider?;
        let model = self.config.semantic_runtime.normalized_model()?;
        let snapshot =
            Self::load_latest_manifest_snapshot(&workspace.root, &workspace.repository_id)?;
        let snapshot_digests = snapshot
            .entries
            .iter()
            .map(|entry| FileMetadataDigest {
                path: PathBuf::from(&entry.path),
                size_bytes: entry.size_bytes,
                mtime_ns: entry.mtime_ns,
            })
            .collect::<Vec<_>>();
        let lexical_ready =
            validate_manifest_digests_for_root(&workspace.root, &snapshot_digests).is_some();
        let storage = Storage::new(&workspace.db_path);
        let latest_has_semantic = storage
            .has_semantic_embeddings_for_repository_snapshot_model(
                &workspace.repository_id,
                &snapshot.snapshot_id,
                provider.as_str(),
                model,
            )
            .ok()?;
        let compatible_snapshot_id = storage
            .load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
                &workspace.repository_id,
                provider.as_str(),
                model,
            )
            .ok()
            .flatten()?;

        if lexical_ready && latest_has_semantic {
            return None;
        }

        Some(WorkspaceSemanticRefreshPlan {
            latest_snapshot_id: snapshot.snapshot_id,
            compatible_snapshot_id,
            reason: if !lexical_ready {
                "stale_manifest_snapshot"
            } else {
                "semantic_snapshot_missing_for_active_model"
            },
        })
    }

    fn refresh_workspace_semantic_snapshot_with_plan(
        &self,
        workspace: &AttachedWorkspace,
        plan: &WorkspaceSemanticRefreshPlan,
    ) {
        let credentials = SemanticRuntimeCredentials::from_process_env();
        if let Err(err) = self.config.semantic_runtime.validate_startup(&credentials) {
            warn!(
                repository_id = workspace.repository_id,
                snapshot_id = %plan.latest_snapshot_id,
                compatible_snapshot_id = %plan.compatible_snapshot_id,
                reason = plan.reason,
                error = %err,
                "skipping semantic snapshot refresh because runtime startup validation failed"
            );
            return;
        }

        if let Err(err) = reindex_repository_with_runtime_config(
            &workspace.repository_id,
            &workspace.root,
            &workspace.db_path,
            ReindexMode::ChangedOnly,
            &self.config.semantic_runtime,
            &credentials,
        ) {
            warn!(
                repository_id = workspace.repository_id,
                snapshot_id = %plan.latest_snapshot_id,
                compatible_snapshot_id = %plan.compatible_snapshot_id,
                reason = plan.reason,
                error = %err,
                "workspace semantic refresh failed during attach"
            );
        }
    }

    fn maybe_refresh_workspace_semantic_snapshot(&self, workspace: &AttachedWorkspace) {
        let Some(plan) = self.workspace_semantic_refresh_plan(workspace) else {
            return;
        };
        if plan.reason != "semantic_snapshot_missing_for_active_model" {
            return;
        }
        self.refresh_workspace_semantic_snapshot_with_plan(workspace, &plan);
    }

    fn maybe_spawn_workspace_runtime_prewarm(&self, workspace: &AttachedWorkspace) {
        let semantic_plan = self.workspace_semantic_refresh_plan(workspace);
        let should_refresh_semantic = semantic_plan
            .as_ref()
            .is_some_and(|plan| plan.reason == "stale_manifest_snapshot");
        let should_prewarm_precise = !Self::collect_scip_artifact_digests(&workspace.root)
            .artifact_digests
            .is_empty();
        if !should_refresh_semantic && !should_prewarm_precise {
            return;
        }

        if should_refresh_semantic {
            let server = self.clone();
            let workspace = workspace.clone();
            let semantic_plan = semantic_plan.clone();
            let _ = std::thread::Builder::new()
                .name(format!(
                    "frigg-semantic-refresh-{}",
                    workspace.repository_id
                ))
                .spawn(move || {
                    if let Some(plan) = semantic_plan.as_ref() {
                        server.refresh_workspace_semantic_snapshot_with_plan(&workspace, plan);
                    }
                });
        }

        if should_prewarm_precise {
            let server = self.clone();
            let workspace = workspace.clone();
            let _ = std::thread::Builder::new()
                .name(format!("frigg-precise-prewarm-{}", workspace.repository_id))
                .spawn(move || {
                    server.prewarm_precise_graph_for_workspace(&workspace);
                });
        }
    }

    fn prewarm_precise_graph_for_workspace(&self, workspace: &AttachedWorkspace) {
        let discovery = Self::collect_scip_artifact_digests(&workspace.root);
        if discovery.artifact_digests.is_empty() {
            return;
        }
        if self
            .try_reuse_latest_precise_graph_for_repository(&workspace.repository_id, &workspace.root)
            .is_some()
        {
            return;
        }

        let corpus = match self.collect_repository_symbol_corpus(
            workspace.repository_id.clone(),
            workspace.root.clone(),
        ) {
            Ok(corpus) => corpus,
            Err(err) => {
                warn!(
                    repository_id = workspace.repository_id,
                    error = %err,
                    "failed to prewarm repository symbol corpus during workspace attach"
                );
                return;
            }
        };

        if let Err(err) =
            self.precise_graph_for_corpus(corpus.as_ref(), self.find_references_resource_budgets())
        {
            warn!(
                repository_id = workspace.repository_id,
                error = %err,
                "failed to prewarm precise graph during workspace attach"
            );
        }
    }

    fn storage_error_health_summary(
        storage: &WorkspaceStorageSummary,
    ) -> Option<WorkspaceIndexComponentSummary> {
        let (state, reason) = match storage.index_state {
            WorkspaceStorageIndexState::MissingDb => (
                WorkspaceIndexComponentState::Missing,
                Some("missing_db".to_owned()),
            ),
            WorkspaceStorageIndexState::Uninitialized => (
                WorkspaceIndexComponentState::Missing,
                Some(if storage.initialized {
                    "missing_manifest_snapshot".to_owned()
                } else {
                    "uninitialized_db".to_owned()
                }),
            ),
            WorkspaceStorageIndexState::Ready => return None,
            WorkspaceStorageIndexState::Error => (
                WorkspaceIndexComponentState::Error,
                storage
                    .error
                    .clone()
                    .or_else(|| Some("storage_error".to_owned())),
            ),
        };
        Some(WorkspaceIndexComponentSummary {
            state,
            reason,
            snapshot_id: None,
            compatible_snapshot_id: None,
            provider: None,
            model: None,
            artifact_count: None,
        })
    }

    fn attach_workspace_internal(
        &self,
        path: &Path,
        set_default: bool,
        resolve_mode: WorkspaceResolveMode,
    ) -> Result<WorkspaceAttachResponse, ErrorData> {
        if path.as_os_str().is_empty() {
            return Err(Self::invalid_params(
                "workspace_attach.path must not be empty",
                None,
            ));
        }

        let resolved_from = Self::effective_attach_directory(path)?;
        let (root, resolution) = match resolve_mode {
            WorkspaceResolveMode::GitRoot => match Self::find_git_root(&resolved_from) {
                Some(git_root) => (git_root, WorkspaceResolveMode::GitRoot),
                None => (resolved_from.clone(), WorkspaceResolveMode::Direct),
            },
            WorkspaceResolveMode::Direct => (resolved_from.clone(), WorkspaceResolveMode::Direct),
        };

        let workspace = {
            let mut registry = self
                .workspace_registry
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            registry.get_or_insert(root)
        };

        if set_default {
            self.set_current_repository_id(Some(workspace.repository_id.clone()));
        }

        self.maybe_refresh_workspace_semantic_snapshot(&workspace);

        let mut repository = self.repository_summary(&workspace);
        let storage = repository
            .storage
            .clone()
            .unwrap_or_else(|| Self::workspace_storage_summary(&workspace));
        repository.storage = None;
        self.maybe_spawn_workspace_runtime_prewarm(&workspace);

        Ok(WorkspaceAttachResponse {
            repository,
            resolved_from: resolved_from.display().to_string(),
            resolution,
            session_default: self.current_repository_id().as_deref()
                == Some(workspace.repository_id.as_str()),
            storage,
        })
    }

    fn scoped_search_config(
        &self,
        scoped_workspaces: &[AttachedWorkspace],
    ) -> (FriggConfig, BTreeMap<String, String>) {
        let scoped_config = FriggConfig {
            workspace_roots: scoped_workspaces
                .iter()
                .map(|workspace| workspace.root.clone())
                .collect(),
            ..(*self.config).clone()
        };
        let repository_id_map = scoped_config
            .repositories()
            .into_iter()
            .zip(scoped_workspaces.iter())
            .map(|(temporary, actual)| (temporary.repository_id.0, actual.repository_id.clone()))
            .collect::<BTreeMap<_, _>>();
        (scoped_config, repository_id_map)
    }

    fn canonicalize_existing_ancestor(path: &Path) -> Result<Option<PathBuf>, ErrorData> {
        for ancestor in path.ancestors() {
            match ancestor.canonicalize() {
                Ok(canonical) => return Ok(Some(canonical)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(Self::internal(
                        format!(
                            "failed to canonicalize ancestor {}: {err}",
                            ancestor.display()
                        ),
                        None,
                    ));
                }
            }
        }

        Ok(None)
    }

    fn candidate_within_root(candidate: &Path, root_canonical: &Path) -> Result<bool, ErrorData> {
        let Some(ancestor) = Self::canonicalize_existing_ancestor(candidate)? else {
            return Ok(false);
        };

        Ok(ancestor.starts_with(root_canonical))
    }

    fn resolve_file_path(
        &self,
        params: &ReadFileParams,
    ) -> Result<(String, PathBuf, String), ErrorData> {
        let requested = PathBuf::from(&params.path);
        let roots = self
            .roots_for_repository(params.repository_id.as_deref())?
            .into_iter()
            .map(|(repository_id, root)| {
                let root_canonical = root.canonicalize().map_err(|err| {
                    Self::internal(
                        format!("failed to canonicalize root {}: {err}", root.display()),
                        None,
                    )
                })?;
                Ok((repository_id, root_canonical))
            })
            .collect::<Result<Vec<_>, ErrorData>>()?;

        let mut saw_workspace_candidate = false;

        for (repository_id, root_canonical) in roots {
            let candidate = if requested.is_absolute() {
                requested.clone()
            } else {
                root_canonical.join(&requested)
            };

            match candidate.canonicalize() {
                Ok(candidate_canonical) => {
                    if !candidate_canonical.starts_with(&root_canonical) {
                        continue;
                    }
                    saw_workspace_candidate = true;

                    let metadata = fs::metadata(&candidate_canonical).map_err(|err| {
                        Self::internal(
                            format!(
                                "failed to stat file {}: {err}",
                                candidate_canonical.display()
                            ),
                            None,
                        )
                    })?;
                    if metadata.is_file() {
                        let display_path =
                            Self::relative_display_path(&root_canonical, &candidate_canonical);
                        return Ok((repository_id, candidate_canonical, display_path));
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    if Self::candidate_within_root(&candidate, &root_canonical)? {
                        saw_workspace_candidate = true;
                    }
                }
                Err(err) => {
                    return Err(Self::internal(
                        format!("failed to canonicalize file {}: {err}", candidate.display()),
                        None,
                    ));
                }
            }
        }

        if saw_workspace_candidate {
            return Err(Self::resource_not_found(
                "file not found",
                Some(serde_json::json!({ "path": params.path })),
            ));
        }

        Err(Self::access_denied(
            "path is outside workspace roots",
            Some(serde_json::json!({ "path": params.path })),
        ))
    }
}

#[tool_router(router = tool_router)]
impl FriggMcpServer {
    #[tool(
        name = "list_repositories",
        description = "List attached repositories for the current Frigg process.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn list_repositories(
        &self,
        params: Parameters<ListRepositoriesParams>,
    ) -> Result<Json<ListRepositoriesResponse>, ErrorData> {
        let _params = params.0;
        let repositories = self
            .attached_workspaces()
            .into_iter()
            .map(|workspace| self.repository_summary(&workspace))
            .collect::<Vec<_>>();

        let response = ListRepositoriesResponse { repositories };
        let source_refs = json!({
            "repository_ids": response
                .repositories
                .iter()
                .map(|repo| repo.repository_id.clone())
                .collect::<Vec<_>>(),
        });
        let result = Ok(Json(response));
        let provenance_result = self
            .record_provenance_blocking("list_repositories", None, json!({}), source_refs, &result)
            .await;
        self.finalize_with_provenance("list_repositories", result, provenance_result)
    }

    #[tool(
        name = "workspace_attach",
        description = "Attach a workspace and optionally set it as the session default repository.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn workspace_attach(
        &self,
        params: Parameters<WorkspaceAttachParams>,
    ) -> Result<Json<WorkspaceAttachResponse>, ErrorData> {
        let params = params.0;
        let set_default = params.set_default.unwrap_or(true);
        let resolve_mode = params.resolve_mode.unwrap_or(WorkspaceResolveMode::GitRoot);
        let response =
            self.attach_workspace_internal(Path::new(&params.path), set_default, resolve_mode)?;
        let source_refs = json!({
            "repository_id": response.repository.repository_id.clone(),
            "root_path": response.repository.root_path.clone(),
            "resolved_from": response.resolved_from.clone(),
            "resolution": response.resolution,
            "session_default": response.session_default,
            "storage": {
                "db_path": response.storage.db_path.clone(),
                "exists": response.storage.exists,
                "initialized": response.storage.initialized,
                "index_state": response.storage.index_state,
            },
        });
        let result = Ok(Json(response));
        let provenance_result = self
            .record_provenance_blocking(
                "workspace_attach",
                None,
                json!({
                    "path": Self::bounded_text(&params.path),
                    "set_default": params.set_default,
                    "resolve_mode": params.resolve_mode,
                }),
                source_refs,
                &result,
            )
            .await;
        self.finalize_with_provenance("workspace_attach", result, provenance_result)
    }

    #[tool(
        name = "workspace_current",
        description = "Return the session default repository, if any.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn workspace_current(
        &self,
        params: Parameters<WorkspaceCurrentParams>,
    ) -> Result<Json<WorkspaceCurrentResponse>, ErrorData> {
        let _params = params.0;
        let current_workspace = self.current_workspace();
        let response = WorkspaceCurrentResponse {
            repository: current_workspace
                .as_ref()
                .map(|workspace| self.repository_summary(workspace)),
            session_default: current_workspace.is_some(),
        };
        let source_refs = json!({
            "repository_id": response
                .repository
                .as_ref()
                .map(|repository| repository.repository_id.clone()),
        });
        let result = Ok(Json(response));
        let provenance_result = self
            .record_provenance_blocking("workspace_current", None, json!({}), source_refs, &result)
            .await;
        self.finalize_with_provenance("workspace_current", result, provenance_result)
    }

    #[tool(
        name = "read_file",
        description = "Read a workspace file by canonical path or in-root absolute path; prefer local shell reads for simple direct inspection.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn read_file(
        &self,
        params: Parameters<ReadFileParams>,
    ) -> Result<Json<ReadFileResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("read_file", move || {
            let mut resolved_repository_id: Option<String> = None;
            let mut resolved_path: Option<String> = None;
            let mut resolved_absolute_path: Option<String> = None;
            let mut effective_max_bytes: Option<usize> = None;
            let mut effective_line_start: Option<usize> = None;
            let mut effective_line_end: Option<usize> = None;
            let result = (|| -> Result<Json<ReadFileResponse>, ErrorData> {
                let requested_max_bytes = params_for_blocking
                    .max_bytes
                    .unwrap_or(server.config.max_file_bytes);
                if requested_max_bytes == 0 {
                    return Err(Self::invalid_params(
                        "max_bytes must be greater than zero",
                        None,
                    ));
                }

                let max_bytes = requested_max_bytes.min(server.config.max_file_bytes);
                effective_max_bytes = Some(max_bytes);
                let has_line_range = params_for_blocking.line_start.is_some()
                    || params_for_blocking.line_end.is_some();
                if params_for_blocking.line_start == Some(0) {
                    return Err(Self::invalid_params(
                        "line_start must be greater than zero when provided",
                        None,
                    ));
                }
                if params_for_blocking.line_end == Some(0) {
                    return Err(Self::invalid_params(
                        "line_end must be greater than zero when provided",
                        None,
                    ));
                }
                if let (Some(line_start), Some(line_end)) =
                    (params_for_blocking.line_start, params_for_blocking.line_end)
                {
                    if line_end < line_start {
                        return Err(Self::invalid_params(
                            "line_end must be greater than or equal to line_start",
                            Some(serde_json::json!({
                                "line_start": line_start,
                                "line_end": line_end,
                            })),
                        ));
                    }
                }

                let (repository_id, path, display_path) =
                    server.resolve_file_path(&params_for_blocking)?;
                resolved_repository_id = Some(repository_id.clone());
                resolved_path = Some(display_path.clone());
                resolved_absolute_path = Some(path.display().to_string());

                let metadata = fs::metadata(&path).map_err(|err| {
                    Self::internal(
                        format!("failed to stat file {}: {err}", path.display()),
                        None,
                    )
                })?;
                let pre_read_bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
                if !has_line_range && pre_read_bytes > max_bytes {
                    let suggested_max_bytes = pre_read_bytes.min(server.config.max_file_bytes);
                    return Err(Self::invalid_params(
                        format!("file exceeds max_bytes={max_bytes}"),
                        Some(serde_json::json!({
                            "path": display_path.clone(),
                            "bytes": pre_read_bytes,
                            "max_bytes": max_bytes,
                            "requested_max_bytes": requested_max_bytes,
                            "config_max_file_bytes": server.config.max_file_bytes,
                            "suggested_max_bytes": suggested_max_bytes,
                        })),
                    ));
                }

                if !has_line_range {
                    let bytes = fs::read(&path).map_err(|err| {
                        Self::internal(
                            format!("failed to read file {}: {err}", path.display()),
                            None,
                        )
                    })?;

                    if bytes.len() > max_bytes {
                        let suggested_max_bytes = bytes.len().min(server.config.max_file_bytes);
                        return Err(Self::invalid_params(
                            format!("file exceeds max_bytes={max_bytes}"),
                            Some(serde_json::json!({
                                "path": display_path.clone(),
                                "bytes": bytes.len(),
                                "max_bytes": max_bytes,
                                "requested_max_bytes": requested_max_bytes,
                                "config_max_file_bytes": server.config.max_file_bytes,
                                "suggested_max_bytes": suggested_max_bytes,
                            })),
                        ));
                    }

                    let content = String::from_utf8_lossy(&bytes).to_string();
                    return Ok(Json(ReadFileResponse {
                        repository_id,
                        path: display_path,
                        bytes: bytes.len(),
                        content,
                    }));
                }

                let line_start = params_for_blocking.line_start.unwrap_or(1);
                let requested_line_end = params_for_blocking.line_end;
                let effective_end_hint = requested_line_end;
                effective_line_start = Some(line_start);
                effective_line_end = Some(effective_end_hint.unwrap_or(1));

                let line_slice =
                    read_line_slice_lossy(&path, line_start, requested_line_end, max_bytes)
                        .map_err(|err| Self::map_lossy_line_slice_error(&path, err))?;
                let sliced_content = line_slice.content;
                let sliced_bytes = line_slice.bytes;
                let total_lines = line_slice.total_lines;
                let effective_end = requested_line_end
                    .unwrap_or(total_lines.max(1))
                    .min(total_lines.max(1));
                effective_line_end = Some(effective_end);

                if sliced_bytes > max_bytes {
                    let suggested_max_bytes = sliced_bytes.min(server.config.max_file_bytes);
                    return Err(Self::invalid_params(
                        format!("selected line range exceeds max_bytes={max_bytes}"),
                        Some(serde_json::json!({
                            "path": display_path.clone(),
                            "bytes": sliced_bytes,
                            "max_bytes": max_bytes,
                            "requested_max_bytes": requested_max_bytes,
                            "config_max_file_bytes": server.config.max_file_bytes,
                            "suggested_max_bytes": suggested_max_bytes,
                            "line_start": line_start,
                            "line_end": effective_end,
                            "total_lines": total_lines,
                        })),
                    ));
                }

                Ok(Json(ReadFileResponse {
                    repository_id,
                    path: display_path,
                    bytes: sliced_bytes,
                    content: sliced_content,
                }))
            })();

            ReadFileExecution {
                result,
                resolved_repository_id,
                resolved_path,
                resolved_absolute_path,
                effective_max_bytes,
                effective_line_start,
                effective_line_end,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
                "read_file",
                repository_hint.as_deref(),
                json!({
                    "repository_id": repository_hint,
                    "path": Self::bounded_text(&params.path),
                    "max_bytes": params.max_bytes,
                    "line_start": params.line_start,
                    "line_end": params.line_end,
                    "effective_max_bytes": execution.effective_max_bytes,
                    "effective_line_start": execution.effective_line_start,
                    "effective_line_end": execution.effective_line_end,
                }),
                json!({
                    "resolved_repository_id": execution.resolved_repository_id,
                    "resolved_path": execution
                        .resolved_path
                        .map(|path| Self::bounded_text(&path)),
                    "resolved_absolute_path": execution
                        .resolved_absolute_path
                        .map(|path| Self::bounded_text(&path)),
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("read_file", result, provenance_result)
    }

    #[tool(
        name = "explore",
        description = "Explore one resolved artifact with probe, zoom, or refine follow-up operations.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn explore(
        &self,
        params: Parameters<ExploreParams>,
    ) -> Result<Json<ExploreResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("explore", move || {
            let mut resolved_repository_id: Option<String> = None;
            let mut resolved_path: Option<String> = None;
            let mut resolved_absolute_path: Option<String> = None;
            let mut effective_context_lines: Option<usize> = None;
            let mut effective_max_matches: Option<usize> = None;
            let mut scan_scope = None;
            let mut total_matches = 0usize;
            let mut truncated = false;

            let result = (|| -> Result<Json<ExploreResponse>, ErrorData> {
                let requested_context_lines = params_for_blocking
                    .context_lines
                    .unwrap_or(DEFAULT_CONTEXT_LINES);
                let context_lines = requested_context_lines.min(MAX_CONTEXT_LINES);
                effective_context_lines = Some(context_lines);

                let requested_max_matches = params_for_blocking
                    .max_matches
                    .unwrap_or(DEFAULT_MAX_MATCHES);
                if requested_max_matches == 0 {
                    return Err(Self::invalid_params(
                        "max_matches must be greater than zero",
                        None,
                    ));
                }
                let max_matches =
                    requested_max_matches.min(server.config.max_search_results.max(1));
                effective_max_matches = Some(max_matches);

                let operation = params_for_blocking.operation;
                let query = params_for_blocking
                    .query
                    .as_ref()
                    .map(|value| value.trim().to_owned());
                let anchor = params_for_blocking.anchor.clone();
                let resume_from = params_for_blocking.resume_from.clone();

                let (matcher, response_query, response_pattern_type, scope, include_scope_content) =
                    match operation {
                        ExploreOperation::Probe => {
                            if anchor.is_some() {
                                return Err(Self::invalid_params(
                                    "anchor is not allowed for probe",
                                    None,
                                ));
                            }
                            let Some(query) = query.clone().filter(|value| !value.is_empty())
                            else {
                                return Err(Self::invalid_params("query must not be empty", None));
                            };
                            if let Some(cursor) = resume_from.as_ref() {
                                validate_cursor(cursor).map_err(|message| {
                                    Self::invalid_params(
                                        message,
                                        Some(json!({ "resume_from": cursor })),
                                    )
                                })?;
                            }

                            let pattern_type = params_for_blocking
                                .pattern_type
                                .clone()
                                .unwrap_or(SearchPatternType::Literal);
                            let matcher = match pattern_type.clone() {
                                SearchPatternType::Literal => {
                                    ExploreMatcher::Literal(query.clone())
                                }
                                SearchPatternType::Regex => {
                                    let regex = compile_safe_regex(&query).map_err(|err| {
                                        Self::invalid_params(
                                            format!("invalid query regex: {err}"),
                                            Some(json!({
                                                "query": query,
                                                "regex_error_code": err.code(),
                                            })),
                                        )
                                    })?;
                                    if regex.is_match("") {
                                        return Err(Self::invalid_params(
                                            "query regex must not match empty strings",
                                            Some(json!({ "query": query })),
                                        ));
                                    }
                                    ExploreMatcher::Regex(regex)
                                }
                            };

                            (
                                Some(matcher),
                                Some(query),
                                Some(pattern_type),
                                ExploreScopeRequest {
                                    start_line: resume_from
                                        .as_ref()
                                        .map(|cursor| cursor.line)
                                        .unwrap_or(1),
                                    end_line: None,
                                },
                                false,
                            )
                        }
                        ExploreOperation::Zoom => {
                            if params_for_blocking.query.is_some() {
                                return Err(Self::invalid_params(
                                    "query is not allowed for zoom",
                                    None,
                                ));
                            }
                            if params_for_blocking.pattern_type.is_some() {
                                return Err(Self::invalid_params(
                                    "pattern_type is not allowed for zoom",
                                    None,
                                ));
                            }
                            if resume_from.is_some() {
                                return Err(Self::invalid_params(
                                    "resume_from is not allowed for zoom",
                                    None,
                                ));
                            }
                            let Some(anchor) = anchor.as_ref() else {
                                return Err(Self::invalid_params(
                                    "anchor is required for zoom",
                                    None,
                                ));
                            };
                            validate_anchor(anchor).map_err(|message| {
                                Self::invalid_params(message, Some(json!({ "anchor": anchor })))
                            })?;
                            let scope_window = line_window_around_anchor(anchor, context_lines);
                            (
                                None,
                                None,
                                None,
                                ExploreScopeRequest {
                                    start_line: scope_window.start_line,
                                    end_line: Some(scope_window.end_line),
                                },
                                true,
                            )
                        }
                        ExploreOperation::Refine => {
                            let Some(anchor) = anchor.as_ref() else {
                                return Err(Self::invalid_params(
                                    "anchor is required for refine",
                                    None,
                                ));
                            };
                            validate_anchor(anchor).map_err(|message| {
                                Self::invalid_params(message, Some(json!({ "anchor": anchor })))
                            })?;
                            let Some(query) = query.clone().filter(|value| !value.is_empty())
                            else {
                                return Err(Self::invalid_params("query must not be empty", None));
                            };
                            let scope_window = line_window_around_anchor(anchor, context_lines);
                            if let Some(cursor) = resume_from.as_ref() {
                                validate_cursor(cursor).map_err(|message| {
                                    Self::invalid_params(
                                        message,
                                        Some(json!({ "resume_from": cursor })),
                                    )
                                })?;
                                if cursor.line < scope_window.start_line
                                    || cursor.line > scope_window.end_line
                                {
                                    return Err(Self::invalid_params(
                                        "resume_from must stay within the refine scan scope",
                                        Some(json!({
                                        "resume_from": cursor,
                                            "scan_scope": scope_window.clone(),
                                        })),
                                    ));
                                }
                            }

                            let pattern_type = params_for_blocking
                                .pattern_type
                                .clone()
                                .unwrap_or(SearchPatternType::Literal);
                            let matcher = match pattern_type.clone() {
                                SearchPatternType::Literal => {
                                    ExploreMatcher::Literal(query.clone())
                                }
                                SearchPatternType::Regex => {
                                    let regex = compile_safe_regex(&query).map_err(|err| {
                                        Self::invalid_params(
                                            format!("invalid query regex: {err}"),
                                            Some(json!({
                                                "query": query,
                                                "regex_error_code": err.code(),
                                            })),
                                        )
                                    })?;
                                    if regex.is_match("") {
                                        return Err(Self::invalid_params(
                                            "query regex must not match empty strings",
                                            Some(json!({ "query": query })),
                                        ));
                                    }
                                    ExploreMatcher::Regex(regex)
                                }
                            };

                            (
                                Some(matcher),
                                Some(query),
                                Some(pattern_type),
                                ExploreScopeRequest {
                                    start_line: scope_window.start_line,
                                    end_line: Some(scope_window.end_line),
                                },
                                true,
                            )
                        }
                    };

                let read_params = ReadFileParams {
                    path: params_for_blocking.path.clone(),
                    repository_id: params_for_blocking.repository_id.clone(),
                    max_bytes: None,
                    line_start: None,
                    line_end: None,
                };
                let (repository_id, path, display_path) = server.resolve_file_path(&read_params)?;
                resolved_repository_id = Some(repository_id.clone());
                resolved_path = Some(display_path.clone());
                resolved_absolute_path = Some(path.display().to_string());

                let mut lossy_utf8 = false;
                let scan = scan_file_scope_lossy(
                    &path,
                    scope,
                    matcher.as_ref(),
                    max_matches,
                    resume_from.as_ref(),
                    include_scope_content,
                    include_scope_content.then_some(server.config.max_file_bytes),
                )
                .map_err(|err| {
                    Self::internal(
                        format!("failed to read file {}: {err}", path.display()),
                        None,
                    )
                })?;
                lossy_utf8 |= scan.lossy_utf8;

                if let Some(anchor) = anchor.as_ref() {
                    if scan.total_lines == 0 || anchor.end_line > scan.total_lines {
                        return Err(Self::invalid_params(
                            "anchor is outside file bounds",
                            Some(json!({
                                "anchor": anchor,
                                "total_lines": scan.total_lines,
                            })),
                        ));
                    }
                }
                if let Some(cursor) = resume_from.as_ref() {
                    if scan.total_lines == 0 || cursor.line > scan.total_lines {
                        return Err(Self::invalid_params(
                            "resume_from is outside file bounds",
                            Some(json!({
                                "resume_from": cursor,
                                "total_lines": scan.total_lines,
                            })),
                        ));
                    }
                }

                let window = if include_scope_content {
                    if !scan.scope_within_budget {
                        return Err(Self::line_slice_budget_error(
                            &display_path,
                            scan.scope_bytes.unwrap_or(0),
                            server.config.max_file_bytes,
                            scope.start_line,
                            scan.effective_scope.end_line,
                            scan.total_lines,
                        ));
                    }

                    Some(ExploreWindow {
                        start_line: scan.effective_scope.start_line,
                        end_line: scan.effective_scope.end_line,
                        bytes: scan.scope_bytes.unwrap_or(0),
                        content: scan.scope_content.clone().unwrap_or_default(),
                    })
                } else {
                    None
                };

                let mut matches = Vec::with_capacity(scan.matches.len());
                for (index, matched) in scan.matches.iter().enumerate() {
                    let match_window = line_window_around_anchor(&matched.anchor, context_lines);
                    let match_window_slice = read_line_slice_lossy(
                        &path,
                        match_window.start_line,
                        Some(match_window.end_line),
                        server.config.max_file_bytes,
                    )
                    .map_err(|err| Self::map_lossy_line_slice_error(&path, err))?;
                    if match_window_slice.bytes > server.config.max_file_bytes {
                        return Err(Self::line_slice_budget_error(
                            &display_path,
                            match_window_slice.bytes,
                            server.config.max_file_bytes,
                            match_window.start_line,
                            match_window
                                .end_line
                                .min(match_window_slice.total_lines.max(match_window.start_line)),
                            match_window_slice.total_lines,
                        ));
                    }
                    lossy_utf8 |= match_window_slice.lossy_utf8;
                    let match_window_end = if match_window_slice.total_lines == 0 {
                        0
                    } else {
                        match_window.end_line.min(match_window_slice.total_lines)
                    };

                    matches.push(ExploreMatch {
                        match_id: format!("match-{index:04}"),
                        start_line: matched.start_line,
                        start_column: matched.start_column,
                        end_line: matched.end_line,
                        end_column: matched.end_column,
                        excerpt: matched.excerpt.clone(),
                        window: ExploreWindow {
                            start_line: match_window.start_line,
                            end_line: match_window_end,
                            bytes: match_window_slice.bytes,
                            content: match_window_slice.content,
                        },
                        anchor: matched.anchor.clone(),
                    });
                }

                scan_scope = Some(scan.effective_scope.clone());
                total_matches = scan.total_matches;
                truncated = scan.truncated;

                Ok(Json(ExploreResponse {
                    repository_id,
                    path: display_path,
                    operation,
                    query: response_query,
                    pattern_type: response_pattern_type,
                    total_lines: scan.total_lines,
                    scan_scope: scan.effective_scope,
                    window,
                    total_matches: scan.total_matches,
                    matches,
                    truncated: scan.truncated,
                    resume_from: scan.resume_from,
                    metadata: ExploreMetadata {
                        lossy_utf8,
                        effective_context_lines: context_lines,
                        effective_max_matches: max_matches,
                    },
                }))
            })();

            ExploreExecution {
                result,
                resolved_repository_id,
                resolved_path,
                resolved_absolute_path,
                effective_context_lines,
                effective_max_matches,
                scan_scope,
                total_matches,
                truncated,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
                "explore",
                repository_hint.as_deref(),
                json!({
                    "repository_id": repository_hint,
                    "path": Self::bounded_text(&params.path),
                    "operation": params.operation,
                    "query": params.query.as_ref().map(|value| Self::bounded_text(value)),
                    "pattern_type": params.pattern_type,
                    "context_lines": params.context_lines,
                    "max_matches": params.max_matches,
                    "resume_from": params.resume_from,
                    "effective_context_lines": execution.effective_context_lines,
                    "effective_max_matches": execution.effective_max_matches,
                }),
                json!({
                    "resolved_repository_id": execution.resolved_repository_id,
                    "resolved_path": execution
                        .resolved_path
                        .map(|path| Self::bounded_text(&path)),
                    "resolved_absolute_path": execution
                        .resolved_absolute_path
                        .map(|path| Self::bounded_text(&path)),
                    "scan_scope": execution.scan_scope,
                    "total_matches": execution.total_matches,
                    "truncated": execution.truncated,
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("explore", result, provenance_result)
    }

    #[tool(
        name = "search_text",
        description = "Search literal or regex text with repository-aware paths; prefer local rg/grep for simple scans and use path_regex to narrow noisy scopes.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn search_text(
        &self,
        params: Parameters<SearchTextParams>,
    ) -> Result<Json<SearchTextResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("search_text", move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut effective_limit: Option<usize> = None;
            let mut effective_pattern_type: Option<SearchPatternType> = None;
            let mut diagnostics_count = 0usize;
            let mut walk_diagnostics_count = 0usize;
            let mut read_diagnostics_count = 0usize;
            let result = (|| -> Result<Json<SearchTextResponse>, ErrorData> {
                let query = params_for_blocking.query.trim().to_owned();
                if query.is_empty() {
                    return Err(Self::invalid_params("query must not be empty", None));
                }

                let path_regex = match params_for_blocking.path_regex.clone() {
                    Some(raw) => Some(compile_safe_regex(&raw).map_err(|err| {
                        Self::invalid_params(
                            format!("invalid path_regex: {err}"),
                            Some(serde_json::json!({
                                "path_regex": raw,
                                "regex_error_code": err.code(),
                            })),
                        )
                    })?),
                    None => None,
                };

                let pattern_type = params_for_blocking
                    .pattern_type
                    .clone()
                    .unwrap_or(SearchPatternType::Literal);
                effective_pattern_type = Some(pattern_type.clone());

                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                let scoped_workspaces = server.attached_workspaces_for_repository(
                    params_for_blocking.repository_id.as_deref(),
                )?;
                scoped_repository_ids = scoped_workspaces
                    .iter()
                    .map(|workspace| workspace.repository_id.clone())
                    .collect::<Vec<_>>();
                let (scoped_config, repository_id_map) =
                    server.scoped_search_config(&scoped_workspaces);

                let searcher = TextSearcher::new(scoped_config);
                let search_output = match pattern_type {
                    SearchPatternType::Literal => searcher.search_literal_with_filters_diagnostics(
                        SearchTextQuery {
                            query,
                            path_regex,
                            limit,
                        },
                        SearchFilters::default(),
                    ),
                    SearchPatternType::Regex => searcher.search_regex_with_filters_diagnostics(
                        SearchTextQuery {
                            query,
                            path_regex,
                            limit,
                        },
                        SearchFilters::default(),
                    ),
                }
                .map_err(Self::map_frigg_error)?;
                diagnostics_count = search_output.diagnostics.total_count();
                walk_diagnostics_count = search_output
                    .diagnostics
                    .count_by_kind(SearchDiagnosticKind::Walk);
                read_diagnostics_count = search_output
                    .diagnostics
                    .count_by_kind(SearchDiagnosticKind::Read);
                let mut matches = search_output.matches;
                let total_matches = search_output.total_matches;
                for found in &mut matches {
                    if let Some(actual_repository_id) = repository_id_map.get(&found.repository_id)
                    {
                        found.repository_id = actual_repository_id.clone();
                    }
                }

                Ok(Json(SearchTextResponse {
                    total_matches,
                    matches,
                }))
            })();

            let total_matches = result
                .as_ref()
                .map(|response| response.0.total_matches)
                .unwrap_or(0);

            SearchTextExecution {
                result,
                scoped_repository_ids,
                total_matches,
                effective_limit,
                effective_pattern_type,
                diagnostics_count,
                walk_diagnostics_count,
                read_diagnostics_count,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
                "search_text",
                repository_hint.as_deref(),
                json!({
                    "repository_id": repository_hint,
                    "query": Self::bounded_text(&params.query),
                    "pattern_type": execution.effective_pattern_type,
                    "path_regex": params.path_regex.map(|raw| Self::bounded_text(&raw)),
                    "limit": params.limit,
                    "effective_limit": execution.effective_limit,
                }),
                json!({
                    "scoped_repository_ids": execution.scoped_repository_ids,
                    "total_matches": execution.total_matches,
                    "diagnostics_count": execution.diagnostics_count,
                    "diagnostics": {
                        "walk": execution.walk_diagnostics_count,
                        "read": execution.read_diagnostics_count,
                        "total": execution.diagnostics_count,
                    },
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("search_text", result, provenance_result)
    }

    #[tool(
        name = "search_hybrid",
        description = "Broad repository-aware doc/runtime search when shell grep is too weak; pivot to search_symbol or scoped search_text for concrete anchors.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn search_hybrid(
        &self,
        params: Parameters<SearchHybridParams>,
    ) -> Result<Json<SearchHybridResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("search_hybrid", move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut effective_limit: Option<usize> = None;
            let mut effective_weights: Option<SearchHybridChannelWeightsParams> = None;
            let mut diagnostics_count = 0usize;
            let mut walk_diagnostics_count = 0usize;
            let mut read_diagnostics_count = 0usize;
            let mut semantic_requested: Option<bool> = None;
            let mut semantic_enabled: Option<bool> = None;
            let mut semantic_status: Option<String> = None;
            let mut semantic_reason: Option<String> = None;
            let mut semantic_candidate_count: Option<usize> = None;
            let mut semantic_hit_count: Option<usize> = None;
            let mut semantic_match_count: Option<usize> = None;
            let mut warning: Option<String> = None;
            let result = (|| -> Result<Json<SearchHybridResponse>, ErrorData> {
                let query = params_for_blocking.query.trim().to_owned();
                if query.is_empty() {
                    return Err(Self::invalid_params("query must not be empty", None));
                }

                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                let scoped_workspaces = server.attached_workspaces_for_repository(
                    params_for_blocking.repository_id.as_deref(),
                )?;
                scoped_repository_ids = scoped_workspaces
                    .iter()
                    .map(|workspace| workspace.repository_id.clone())
                    .collect::<Vec<_>>();
                let (scoped_config, repository_id_map) =
                    server.scoped_search_config(&scoped_workspaces);

                let weights = {
                    let mut weights = HybridChannelWeights::default();
                    if let Some(overrides) = params_for_blocking.weights.clone() {
                        if let Some(lexical) = overrides.lexical {
                            weights.lexical = lexical;
                        }
                        if let Some(graph) = overrides.graph {
                            weights.graph = graph;
                        }
                        if let Some(semantic) = overrides.semantic {
                            weights.semantic = semantic;
                        }
                    }
                    effective_weights = Some(SearchHybridChannelWeightsParams {
                        lexical: Some(weights.lexical),
                        graph: Some(weights.graph),
                        semantic: Some(weights.semantic),
                    });
                    weights
                };

                let searcher = TextSearcher::new(scoped_config);
                let search_output = searcher
                    .search_hybrid_with_filters(
                        SearchHybridQuery {
                            query,
                            limit,
                            weights,
                            semantic: params_for_blocking.semantic,
                        },
                        SearchFilters {
                            repository_id: None,
                            language: params_for_blocking.language.clone(),
                        },
                    )
                    .map_err(Self::map_frigg_error)?;

                diagnostics_count = search_output.diagnostics.total_count();
                walk_diagnostics_count = search_output
                    .diagnostics
                    .count_by_kind(SearchDiagnosticKind::Walk);
                read_diagnostics_count = search_output
                    .diagnostics
                    .count_by_kind(SearchDiagnosticKind::Read);
                semantic_requested = Some(search_output.note.semantic_requested);
                semantic_enabled = Some(search_output.note.semantic_enabled);
                semantic_status = Some(search_output.note.semantic_status.as_str().to_owned());
                semantic_reason = search_output.note.semantic_reason.clone();
                semantic_candidate_count = Some(search_output.note.semantic_candidate_count);
                semantic_hit_count = Some(search_output.note.semantic_hit_count);
                semantic_match_count = Some(search_output.note.semantic_match_count);
                warning = Self::search_hybrid_warning(
                    semantic_status.as_deref(),
                    semantic_reason.as_deref(),
                    semantic_hit_count,
                    semantic_match_count,
                );

                let mut matches = search_output
                    .matches
                    .into_iter()
                    .map(|evidence| SearchHybridMatch {
                        repository_id: evidence.document.repository_id,
                        path: evidence.document.path,
                        line: evidence.document.line,
                        column: evidence.document.column,
                        excerpt: evidence.excerpt,
                        blended_score: evidence.blended_score,
                        lexical_score: evidence.lexical_score,
                        graph_score: evidence.graph_score,
                        semantic_score: evidence.semantic_score,
                        lexical_sources: evidence.lexical_sources,
                        graph_sources: evidence.graph_sources,
                        semantic_sources: evidence.semantic_sources,
                    })
                    .collect::<Vec<_>>();
                for found in &mut matches {
                    if let Some(actual_repository_id) = repository_id_map.get(&found.repository_id)
                    {
                        found.repository_id = actual_repository_id.clone();
                    }
                }

                let metadata = Some(json!({
                    "semantic_requested": semantic_requested,
                    "semantic_enabled": semantic_enabled,
                    "semantic_status": semantic_status.clone(),
                    "semantic_reason": semantic_reason.clone(),
                    "semantic_candidate_count": semantic_candidate_count,
                    "semantic_hit_count": semantic_hit_count,
                    "semantic_match_count": semantic_match_count,
                    "warning": warning.clone(),
                    "diagnostics_count": diagnostics_count,
                    "diagnostics": {
                        "walk": walk_diagnostics_count,
                        "read": read_diagnostics_count,
                        "total": diagnostics_count,
                    },
                }));

                Ok(Json(SearchHybridResponse {
                    matches,
                    semantic_requested: None,
                    semantic_enabled: None,
                    semantic_status: None,
                    semantic_reason: None,
                    semantic_hit_count: None,
                    semantic_match_count: None,
                    warning: None,
                    metadata,
                    note: None,
                }))
            })();

            SearchHybridExecution {
                result,
                scoped_repository_ids,
                effective_limit,
                effective_weights,
                diagnostics_count,
                walk_diagnostics_count,
                read_diagnostics_count,
                semantic_requested,
                semantic_enabled,
                semantic_status,
                semantic_reason,
                semantic_candidate_count,
                semantic_hit_count,
                semantic_match_count,
                warning,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
                "search_hybrid",
                repository_hint.as_deref(),
                json!({
                    "repository_id": repository_hint,
                    "query": Self::bounded_text(&params.query),
                    "language": params.language.map(|language| Self::bounded_text(&language)),
                    "limit": params.limit,
                    "effective_limit": execution.effective_limit,
                    "semantic": params.semantic,
                    "weights": execution.effective_weights,
                }),
                json!({
                    "scoped_repository_ids": execution.scoped_repository_ids,
                    "diagnostics_count": execution.diagnostics_count,
                    "diagnostics": {
                        "walk": execution.walk_diagnostics_count,
                        "read": execution.read_diagnostics_count,
                        "total": execution.diagnostics_count,
                    },
                    "semantic_requested": execution.semantic_requested,
                    "semantic_enabled": execution.semantic_enabled,
                    "semantic_status": execution.semantic_status,
                    "semantic_reason": execution.semantic_reason.map(|reason| Self::bounded_text(&reason)),
                    "semantic_candidate_count": execution.semantic_candidate_count,
                    "semantic_hit_count": execution.semantic_hit_count,
                    "semantic_match_count": execution.semantic_match_count,
                    "warning": execution.warning.map(|warning| Self::bounded_text(&warning)),
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("search_hybrid", result, provenance_result)
    }

    #[tool(
        name = "search_symbol",
        description = "Find API, type, and function symbols when the runtime anchor is known and repository-aware symbol lookup is needed.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn search_symbol(
        &self,
        params: Parameters<SearchSymbolParams>,
    ) -> Result<Json<SearchSymbolResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("search_symbol", move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut diagnostics_count = 0usize;
            let mut manifest_walk_diagnostics_count = 0usize;
            let mut manifest_read_diagnostics_count = 0usize;
            let mut symbol_extraction_diagnostics_count = 0usize;
            let mut effective_limit: Option<usize> = None;
            let result = (|| -> Result<Json<SearchSymbolResponse>, ErrorData> {
                let query = params_for_blocking.query.trim().to_owned();
                if query.is_empty() {
                    return Err(Self::invalid_params("query must not be empty", None));
                }

                let path_regex = match params_for_blocking.path_regex.clone() {
                    Some(raw) => Some(compile_safe_regex(&raw).map_err(|err| {
                        Self::invalid_params(
                            format!("invalid path_regex: {err}"),
                            Some(serde_json::json!({
                                "path_regex": raw,
                                "regex_error_code": err.code(),
                            })),
                        )
                    })?),
                    None => None,
                };
                let path_class_filter = params_for_blocking.path_class;
                let query_lower = query.to_ascii_lowercase();
                let query_looks_canonical =
                    query.contains('\\') || query.contains("::") || query.contains('$');
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                if params_for_blocking.symbol.is_none() {
                    if let (Some(path), Some(line)) = (
                        params_for_blocking.path.as_deref(),
                        params_for_blocking.line,
                    ) {
                        if let Some((
                            response,
                            repository_id,
                            precise_symbol,
                            precision,
                        )) = server.try_cached_precise_definition_fast_path(
                            params_for_blocking.repository_id.as_deref(),
                            path,
                            line,
                            params_for_blocking.column,
                            limit,
                        )? {
                            scoped_repository_ids = vec![repository_id];
                            selected_precise_symbol = Some(precise_symbol);
                            resolution_source = Some("location_precise_cache".to_owned());
                            resolution_precision = Some(precision);
                            match_count = response.0.matches.len();
                            return Ok(response);
                        }
                    }
                }

                let corpora = server.collect_repository_symbol_corpora(
                    params_for_blocking.repository_id.as_deref(),
                )?;
                scoped_repository_ids = corpora
                    .iter()
                    .map(|corpus| corpus.repository_id.clone())
                    .collect::<Vec<_>>();
                manifest_walk_diagnostics_count = corpora
                    .iter()
                    .map(|corpus| corpus.diagnostics.manifest_walk_count)
                    .sum::<usize>();
                manifest_read_diagnostics_count = corpora
                    .iter()
                    .map(|corpus| corpus.diagnostics.manifest_read_count)
                    .sum::<usize>();
                symbol_extraction_diagnostics_count = corpora
                    .iter()
                    .map(|corpus| corpus.diagnostics.symbol_extraction_count)
                    .sum::<usize>();
                diagnostics_count = manifest_walk_diagnostics_count
                    + manifest_read_diagnostics_count
                    + symbol_extraction_diagnostics_count;

                let mut ranked_matches: Vec<RankedSymbolMatch> = Vec::new();
                for corpus in &corpora {
                    if query_looks_canonical {
                        if let Some(symbol_indices) =
                            corpus.symbol_indices_by_canonical_name.get(&query)
                        {
                            for symbol_index in symbol_indices {
                                if let Some(candidate) = Self::build_ranked_symbol_match(
                                    corpus,
                                    *symbol_index,
                                    0,
                                    path_class_filter,
                                    path_regex.as_ref(),
                                ) {
                                    ranked_matches.push(candidate);
                                }
                            }
                        }
                        if let Some(symbol_indices) = corpus
                            .symbol_indices_by_lower_canonical_name
                            .get(&query_lower)
                        {
                            for symbol_index in symbol_indices {
                                if corpus
                                    .canonical_symbol_name_by_stable_id
                                    .get(corpus.symbols[*symbol_index].stable_id.as_str())
                                    .is_some_and(|canonical| canonical != &query)
                                {
                                    if let Some(candidate) = Self::build_ranked_symbol_match(
                                        corpus,
                                        *symbol_index,
                                        1,
                                        path_class_filter,
                                        path_regex.as_ref(),
                                    ) {
                                        ranked_matches.push(candidate);
                                    }
                                }
                            }
                        }
                        for (canonical_name, symbol_indices) in corpus
                            .symbol_indices_by_lower_canonical_name
                            .range(query_lower.clone()..)
                        {
                            if !canonical_name.starts_with(&query_lower) {
                                break;
                            }
                            if canonical_name == &query_lower {
                                continue;
                            }
                            for symbol_index in symbol_indices {
                                if let Some(candidate) = Self::build_ranked_symbol_match(
                                    corpus,
                                    *symbol_index,
                                    2,
                                    path_class_filter,
                                    path_regex.as_ref(),
                                ) {
                                    ranked_matches.push(candidate);
                                }
                            }
                        }
                    }

                    let name_rank_offset = if query_looks_canonical { 3 } else { 0 };
                    if let Some(symbol_indices) = corpus.symbol_indices_by_name.get(&query) {
                        for symbol_index in symbol_indices {
                            if let Some(candidate) = Self::build_ranked_symbol_match(
                                corpus,
                                *symbol_index,
                                name_rank_offset,
                                path_class_filter,
                                path_regex.as_ref(),
                            ) {
                                ranked_matches.push(candidate);
                            }
                        }
                    }
                    if let Some(symbol_indices) =
                        corpus.symbol_indices_by_lower_name.get(&query_lower)
                    {
                        for symbol_index in symbol_indices {
                            if corpus.symbols[*symbol_index].name != query {
                                if let Some(candidate) = Self::build_ranked_symbol_match(
                                    corpus,
                                    *symbol_index,
                                    name_rank_offset + 1,
                                    path_class_filter,
                                    path_regex.as_ref(),
                                ) {
                                    ranked_matches.push(candidate);
                                }
                            }
                        }
                    }
                    for (normalized_name, symbol_indices) in corpus
                        .symbol_indices_by_lower_name
                        .range(query_lower.clone()..)
                    {
                        if !normalized_name.starts_with(&query_lower) {
                            break;
                        }
                        if normalized_name == &query_lower {
                            continue;
                        }
                        for symbol_index in symbol_indices {
                            if let Some(candidate) = Self::build_ranked_symbol_match(
                                corpus,
                                *symbol_index,
                                name_rank_offset + 2,
                                path_class_filter,
                                path_regex.as_ref(),
                            ) {
                                ranked_matches.push(candidate);
                            }
                        }
                    }
                }
                if ranked_matches.len() < limit {
                    let infix_limit = limit.saturating_sub(ranked_matches.len());
                    let mut infix_matches = Vec::new();
                    for corpus in &corpora {
                        for (symbol_index, symbol) in corpus.symbols.iter().enumerate() {
                            if Self::symbol_name_match_rank(&symbol.name, &query, &query_lower)
                                != Some(3)
                            {
                                continue;
                            }
                            if let Some(candidate) = Self::build_ranked_symbol_match(
                                corpus,
                                symbol_index,
                                if query_looks_canonical { 6 } else { 3 },
                                path_class_filter,
                                path_regex.as_ref(),
                            ) {
                                Self::retain_bounded_ranked_symbol_match(
                                    &mut infix_matches,
                                    infix_limit,
                                    candidate,
                                );
                            }
                        }
                    }
                    ranked_matches.extend(infix_matches);
                }

                Self::sort_ranked_symbol_matches(&mut ranked_matches);
                Self::dedup_ranked_symbol_matches(&mut ranked_matches);
                let matches = ranked_matches
                    .into_iter()
                    .take(limit)
                    .map(|ranked| ranked.matched)
                    .collect::<Vec<_>>();

                let metadata = json!({
                    "source": "tree_sitter",
                    "diagnostics_count": diagnostics_count,
                    "diagnostics": {
                        "manifest_walk": manifest_walk_diagnostics_count,
                        "manifest_read": manifest_read_diagnostics_count,
                        "symbol_extraction": symbol_extraction_diagnostics_count,
                        "total": diagnostics_count,
                    },
                    "heuristic": false,
                    "path_class": path_class_filter.map(|value| value.as_str()),
                    "path_regex": params_for_blocking.path_regex.clone(),
                    "path_class_sort": "runtime_first",
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(SearchSymbolResponse {
                    matches,
                    metadata,
                    note,
                }))
            })();

            SearchSymbolExecution {
                result,
                scoped_repository_ids,
                diagnostics_count,
                manifest_walk_diagnostics_count,
                manifest_read_diagnostics_count,
                symbol_extraction_diagnostics_count,
                effective_limit,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
                "search_symbol",
                repository_hint.as_deref(),
                json!({
                    "repository_id": repository_hint,
                    "query": Self::bounded_text(&params.query),
                    "path_class": params.path_class.map(|value| value.as_str().to_owned()),
                    "path_regex": params.path_regex.map(|value| Self::bounded_text(&value)),
                    "limit": params.limit,
                    "effective_limit": execution.effective_limit,
                }),
                json!({
                    "scoped_repository_ids": execution.scoped_repository_ids,
                    "diagnostics_count": execution.diagnostics_count,
                    "diagnostics": {
                        "manifest_walk": execution.manifest_walk_diagnostics_count,
                        "manifest_read": execution.manifest_read_diagnostics_count,
                        "symbol_extraction": execution.symbol_extraction_diagnostics_count,
                        "total": execution.diagnostics_count,
                    },
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("search_symbol", result, provenance_result)
    }

    #[tool(
        name = "find_references",
        description = "Find references for a symbol or source position, preferring precise SCIP data.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn find_references(
        &self,
        params: Parameters<FindReferencesParams>,
    ) -> Result<Json<FindReferencesResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let resource_budgets = self.find_references_resource_budgets();
        let resource_budget_metadata = Self::find_references_budget_metadata(resource_budgets);
        let params_for_blocking = params.clone();
        let server = self.clone();
        let resource_budget_metadata_for_blocking = resource_budget_metadata.clone();
        let execution = Self::run_blocking_task("find_references", move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut total_matches = 0usize;
            let mut selected_symbol_id: Option<String> = None;
            let mut selected_precise_symbol: Option<String> = None;
            let mut resolution_precision: Option<String> = None;
            let mut resolution_source: Option<String> = None;
            let mut diagnostics_count = 0usize;
            let mut manifest_walk_diagnostics_count = 0usize;
            let mut manifest_read_diagnostics_count = 0usize;
            let mut symbol_extraction_diagnostics_count = 0usize;
            let mut source_read_diagnostics_count = 0usize;
            let mut precise_artifacts_discovered = 0usize;
            let mut precise_artifacts_discovered_bytes = 0u64;
            let mut precise_artifacts_ingested = 0usize;
            let mut precise_artifacts_ingested_bytes = 0u64;
            let mut precise_artifacts_failed = 0usize;
            let mut precise_artifacts_failed_bytes = 0u64;
            let mut precise_reference_count = 0usize;
            let mut source_files_discovered = 0usize;
            let mut source_files_loaded = 0usize;
            let mut source_bytes_loaded = 0u64;
            let mut effective_limit: Option<usize> = None;
            let mut target_selection_candidate_count = 0usize;
            let mut target_selection_same_rank_count = 0usize;

            let result = (|| -> Result<Json<FindReferencesResponse>, ErrorData> {
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                let corpora = server
                    .collect_repository_symbol_corpora(params_for_blocking.repository_id.as_deref())?;
                scoped_repository_ids = corpora
                    .iter()
                    .map(|corpus| corpus.repository_id.clone())
                    .collect::<Vec<_>>();
                manifest_walk_diagnostics_count = corpora
                    .iter()
                    .map(|corpus| corpus.diagnostics.manifest_walk_count)
                    .sum::<usize>();
                manifest_read_diagnostics_count = corpora
                    .iter()
                    .map(|corpus| corpus.diagnostics.manifest_read_count)
                    .sum::<usize>();
                symbol_extraction_diagnostics_count = corpora
                    .iter()
                    .map(|corpus| corpus.diagnostics.symbol_extraction_count)
                    .sum::<usize>();
                diagnostics_count = manifest_walk_diagnostics_count
                    + manifest_read_diagnostics_count
                    + symbol_extraction_diagnostics_count;

                let resolve_by_location = params_for_blocking.path.is_some()
                    || params_for_blocking.line.is_some()
                    || params_for_blocking.column.is_some();
                let resolved_target = if resolve_by_location {
                    Self::resolve_navigation_target(
                        &corpora,
                        None,
                        params_for_blocking.path.as_deref(),
                        params_for_blocking.line,
                        params_for_blocking.column,
                        params_for_blocking.repository_id.as_deref(),
                    )?
                } else {
                    Self::resolve_navigation_target(
                        &corpora,
                        params_for_blocking.symbol.as_deref(),
                        None,
                        None,
                        None,
                        params_for_blocking.repository_id.as_deref(),
                    )?
                };
                resolution_source = Some(resolved_target.resolution_source.to_owned());
                let symbol_query = resolved_target.symbol_query;
                let target_resolution = Self::resolve_navigation_symbol_target(
                    &corpora,
                    &symbol_query,
                    params_for_blocking.repository_id.as_deref(),
                )?;
                target_selection_candidate_count = target_resolution.candidate_count;
                target_selection_same_rank_count = target_resolution.selected_rank_candidate_count;
                let target = target_resolution.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());

                let target_corpus = target_resolution.corpus;
                source_files_discovered = target_corpus.source_paths.len();

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                let target_precise_stats = cached_precise_graph.ingest_stats;
                precise_artifacts_discovered = target_precise_stats.artifacts_discovered;
                precise_artifacts_discovered_bytes = target_precise_stats.artifacts_discovered_bytes;
                precise_artifacts_ingested = target_precise_stats.artifacts_ingested;
                precise_artifacts_ingested_bytes = target_precise_stats.artifacts_ingested_bytes;
                precise_artifacts_failed = target_precise_stats.artifacts_failed;
                precise_artifacts_failed_bytes = target_precise_stats.artifacts_failed_bytes;

                let precise_target = Self::select_precise_symbol_for_resolved_target(
                    graph.as_ref(),
                    &target_corpus.repository_id,
                    &target.root,
                    &symbol_query,
                    &target.symbol,
                );
                if let Some(precise_target) = &precise_target {
                    selected_precise_symbol = Some(precise_target.symbol.clone());
                }

                let precise_references = precise_target
                    .as_ref()
                    .map(|precise_target| {
                        graph.precise_references_for_symbol(
                            &target_corpus.repository_id,
                            &precise_target.symbol,
                        )
                    })
                    .unwrap_or_default();
                precise_reference_count = precise_references.len();

                if !precise_references.is_empty() {
                    let matches = precise_references
                        .into_iter()
                        .take(limit)
                        .map(|reference| {
                            let reference_path = PathBuf::from(&reference.path);
                            let absolute_path = if reference_path.is_absolute() {
                                reference_path
                            } else {
                                target.root.join(reference_path)
                            };

                            ReferenceMatch {
                                repository_id: target_corpus.repository_id.clone(),
                                symbol: precise_target
                                    .as_ref()
                                    .map(|selected| selected.display_name.clone())
                                    .filter(|display_name| !display_name.is_empty())
                                    .unwrap_or_else(|| target.symbol.name.clone()),
                                path: Self::relative_display_path(&target.root, &absolute_path),
                                line: reference.range.start_line,
                                column: reference.range.start_column,
                            }
                        })
                        .collect::<Vec<_>>();
                    total_matches = precise_reference_count;

                    let precision = Self::precise_resolution_precision(precise_coverage);
                    resolution_precision = Some(precision.to_owned());
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "target_symbol_id": target.symbol.stable_id,
                        "target_precise_symbol": precise_target
                            .as_ref()
                            .map(|selected| selected.symbol.clone()),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "diagnostics_count": diagnostics_count,
                        "diagnostics": {
                            "manifest_walk": manifest_walk_diagnostics_count,
                            "manifest_read": manifest_read_diagnostics_count,
                            "symbol_extraction": symbol_extraction_diagnostics_count,
                            "source_read": source_read_diagnostics_count,
                            "total": diagnostics_count,
                        },
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &target_precise_stats,
                            "reference_count",
                            precise_reference_count,
                        ),
                        "resource_budgets": resource_budget_metadata_for_blocking.clone(),
                        "resource_usage": {
                            "scip": {
                                "artifacts_discovered": target_precise_stats.artifacts_discovered,
                                "artifacts_discovered_bytes": target_precise_stats.artifacts_discovered_bytes,
                                "artifacts_ingested": target_precise_stats.artifacts_ingested,
                                "artifacts_ingested_bytes": target_precise_stats.artifacts_ingested_bytes,
                                "artifacts_failed": target_precise_stats.artifacts_failed,
                                "artifacts_failed_bytes": target_precise_stats.artifacts_failed_bytes,
                            },
                            "source": {
                                "files_discovered": source_files_discovered,
                                "files_loaded": source_files_loaded,
                                "bytes_loaded": source_bytes_loaded,
                            },
                        },
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);

                    return Ok(Json(FindReferencesResponse {
                        total_matches,
                        matches,
                        metadata,
                        note,
                    }));
                }

                let mut resolver = HeuristicReferenceResolver::new(
                    &target_corpus.repository_id,
                    &target.symbol.stable_id,
                    &target_corpus.symbols,
                    graph.as_ref(),
                )
                .ok_or_else(|| {
                    Self::internal(
                        "failed to initialize heuristic resolver for selected symbol",
                        Some(json!({
                            "repository_id": target_corpus.repository_id,
                            "symbol_id": target.symbol.stable_id,
                        })),
                    )
                })?;

                let source_started_at = Instant::now();
                let source_max_elapsed =
                    Duration::from_millis(resource_budgets.source_max_elapsed_ms);
                let source_max_file_bytes =
                    Self::usize_to_u64(resource_budgets.source_max_file_bytes);
                let source_max_total_bytes =
                    Self::usize_to_u64(resource_budgets.source_max_total_bytes);

                for (index, path) in target_corpus.source_paths.iter().enumerate() {
                    if source_started_at.elapsed() > source_max_elapsed {
                        let elapsed_ms =
                            u64::try_from(source_started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                        return Err(Self::find_references_resource_budget_error(
                            "source",
                            "source_elapsed_ms",
                            "find_references source processing exceeded time budget",
                            json!({
                                "repository_id": target_corpus.repository_id,
                                "actual": elapsed_ms,
                                "limit": resource_budgets.source_max_elapsed_ms,
                                "files_loaded": Self::usize_to_u64(source_files_loaded),
                                "bytes_loaded": source_bytes_loaded,
                            }),
                        ));
                    }

                    if index >= resource_budgets.source_max_files {
                        return Err(Self::find_references_resource_budget_error(
                            "source",
                            "source_file_count",
                            "find_references source file count exceeds configured budget",
                            json!({
                                "repository_id": target_corpus.repository_id,
                                "actual": Self::usize_to_u64(index.saturating_add(1)),
                                "limit": Self::usize_to_u64(resource_budgets.source_max_files),
                            }),
                        ));
                    }

                    let metadata = match fs::metadata(path) {
                        Ok(metadata) => Some(metadata),
                        Err(err) => {
                            source_read_diagnostics_count += 1;
                            warn!(
                                repository_id = target_corpus.repository_id,
                                path = %path.display(),
                                error = %err,
                                "skipping source file while resolving heuristic references"
                            );
                            None
                        }
                    };

                    if let Some(metadata) = metadata {
                        let pre_read_bytes = metadata.len();
                        if pre_read_bytes > source_max_file_bytes {
                            return Err(Self::find_references_resource_budget_error(
                                "source",
                                "source_file_bytes",
                                "find_references source file exceeds per-file byte budget",
                                json!({
                                    "repository_id": target_corpus.repository_id,
                                    "path": path.display().to_string(),
                                    "actual": pre_read_bytes,
                                    "limit": source_max_file_bytes,
                                }),
                            ));
                        }
                        let projected_total = source_bytes_loaded.saturating_add(pre_read_bytes);
                        if projected_total > source_max_total_bytes {
                            return Err(Self::find_references_resource_budget_error(
                                "source",
                                "source_total_bytes",
                                "find_references source bytes exceed configured budget",
                                json!({
                                    "repository_id": target_corpus.repository_id,
                                    "path": path.display().to_string(),
                                    "actual": projected_total,
                                    "limit": source_max_total_bytes,
                                }),
                            ));
                        }
                    }

                    match fs::read_to_string(path) {
                        Ok(source) => {
                            let source_bytes = Self::usize_to_u64(source.len());
                            if source_bytes > source_max_file_bytes {
                                return Err(Self::find_references_resource_budget_error(
                                    "source",
                                    "source_file_bytes",
                                    "find_references source file exceeds per-file byte budget",
                                    json!({
                                        "repository_id": target_corpus.repository_id,
                                        "path": path.display().to_string(),
                                        "actual": source_bytes,
                                        "limit": source_max_file_bytes,
                                    }),
                                ));
                            }
                            let projected_total = source_bytes_loaded.saturating_add(source_bytes);
                            if projected_total > source_max_total_bytes {
                                return Err(Self::find_references_resource_budget_error(
                                    "source",
                                    "source_total_bytes",
                                    "find_references source bytes exceed configured budget",
                                    json!({
                                        "repository_id": target_corpus.repository_id,
                                        "path": path.display().to_string(),
                                        "actual": projected_total,
                                        "limit": source_max_total_bytes,
                                    }),
                                ));
                            }

                            resolver.ingest_source(path, &source);
                            source_files_loaded = source_files_loaded.saturating_add(1);
                            source_bytes_loaded = projected_total;
                        }
                        Err(err) => {
                            source_read_diagnostics_count += 1;
                            warn!(
                                repository_id = target_corpus.repository_id,
                                path = %path.display(),
                                error = %err,
                                "skipping source file while resolving heuristic references"
                            );
                        }
                    }
                }

                let all_references = resolver.finish();
                total_matches = all_references.len();
                let references = all_references.into_iter().take(limit).collect::<Vec<_>>();

                let mut high_confidence = 0usize;
                let mut medium_confidence = 0usize;
                let mut low_confidence = 0usize;
                let mut graph_evidence = 0usize;
                let mut lexical_evidence = 0usize;

                let matches = references
                    .iter()
                    .map(|reference| {
                        match reference.confidence {
                            HeuristicReferenceConfidence::High => high_confidence += 1,
                            HeuristicReferenceConfidence::Medium => medium_confidence += 1,
                            HeuristicReferenceConfidence::Low => low_confidence += 1,
                        }
                        match &reference.evidence {
                            HeuristicReferenceEvidence::GraphRelation { .. } => graph_evidence += 1,
                            HeuristicReferenceEvidence::LexicalToken => lexical_evidence += 1,
                        }

                        ReferenceMatch {
                            repository_id: reference.repository_id.clone(),
                            symbol: reference.symbol_name.clone(),
                            path: Self::relative_display_path(&target.root, &reference.path),
                            line: reference.line,
                            column: reference.column,
                        }
                    })
                    .collect::<Vec<_>>();

                diagnostics_count += source_read_diagnostics_count;
                let metadata = json!({
                    "precision": "heuristic",
                    "heuristic": true,
                    "fallback_reason": "precise_absent",
                    "precise_absence_reason": Self::precise_absence_reason(
                        precise_coverage,
                        &target_precise_stats,
                        precise_reference_count,
                    ),
                    "target_symbol_id": target.symbol.stable_id,
                    "resolution_source": resolution_source.clone(),
                    "target_selection": Self::navigation_target_selection_note(
                        &symbol_query,
                        &target,
                        target_selection_candidate_count,
                        target_selection_same_rank_count,
                    ),
                    "confidence": {
                        "high": high_confidence,
                        "medium": medium_confidence,
                        "low": low_confidence,
                    },
                    "evidence": {
                        "graph_relation": graph_evidence,
                        "lexical_token": lexical_evidence,
                    },
                    "diagnostics_count": diagnostics_count,
                    "diagnostics": {
                        "manifest_walk": manifest_walk_diagnostics_count,
                        "manifest_read": manifest_read_diagnostics_count,
                        "symbol_extraction": symbol_extraction_diagnostics_count,
                        "source_read": source_read_diagnostics_count,
                        "total": diagnostics_count,
                    },
                    "precise": Self::precise_note_with_count(
                        precise_coverage,
                        &target_precise_stats,
                        "reference_count",
                        precise_reference_count,
                    ),
                    "resource_budgets": resource_budget_metadata_for_blocking.clone(),
                    "resource_usage": {
                        "scip": {
                            "artifacts_discovered": target_precise_stats.artifacts_discovered,
                            "artifacts_discovered_bytes": target_precise_stats.artifacts_discovered_bytes,
                            "artifacts_ingested": target_precise_stats.artifacts_ingested,
                            "artifacts_ingested_bytes": target_precise_stats.artifacts_ingested_bytes,
                            "artifacts_failed": target_precise_stats.artifacts_failed,
                            "artifacts_failed_bytes": target_precise_stats.artifacts_failed_bytes,
                        },
                        "source": {
                            "files_discovered": source_files_discovered,
                            "files_loaded": source_files_loaded,
                            "bytes_loaded": source_bytes_loaded,
                        },
                    },
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                resolution_precision = Some("heuristic".to_owned());

                Ok(Json(FindReferencesResponse {
                    total_matches,
                    matches,
                    metadata,
                    note,
                }))
            })();

            FindReferencesExecution {
                result,
                scoped_repository_ids,
                total_matches,
                selected_symbol_id,
                selected_precise_symbol,
                resolution_precision,
                resolution_source,
                diagnostics_count,
                manifest_walk_diagnostics_count,
                manifest_read_diagnostics_count,
                symbol_extraction_diagnostics_count,
                source_read_diagnostics_count,
                precise_artifacts_discovered,
                precise_artifacts_discovered_bytes,
                precise_artifacts_ingested,
                precise_artifacts_ingested_bytes,
                precise_artifacts_failed,
                precise_artifacts_failed_bytes,
                precise_reference_count,
                source_files_discovered,
                source_files_loaded,
                source_bytes_loaded,
                effective_limit,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
            "find_references",
            repository_hint.as_deref(),
            json!({
                "repository_id": repository_hint,
                "symbol": params.symbol.map(|symbol| Self::bounded_text(&symbol)),
                "path": params.path.map(|path| Self::bounded_text(&path)),
                "line": params.line,
                "column": params.column,
                "limit": params.limit,
                "effective_limit": execution.effective_limit,
            }),
            json!({
                "scoped_repository_ids": execution.scoped_repository_ids,
                "total_matches": execution.total_matches,
                "selected_symbol_id": execution.selected_symbol_id,
                "selected_precise_symbol": execution.selected_precise_symbol,
                "resolution_precision": execution.resolution_precision,
                "resolution_source": execution.resolution_source,
                "diagnostics_count": execution.diagnostics_count,
                "diagnostics": {
                    "manifest_walk": execution.manifest_walk_diagnostics_count,
                    "manifest_read": execution.manifest_read_diagnostics_count,
                    "symbol_extraction": execution.symbol_extraction_diagnostics_count,
                    "source_read": execution.source_read_diagnostics_count,
                    "total": execution.diagnostics_count,
                },
                "precise_artifacts_discovered": execution.precise_artifacts_discovered,
                "precise_artifacts_discovered_bytes": execution.precise_artifacts_discovered_bytes,
                "precise_artifacts_ingested": execution.precise_artifacts_ingested,
                "precise_artifacts_ingested_bytes": execution.precise_artifacts_ingested_bytes,
                "precise_artifacts_failed": execution.precise_artifacts_failed,
                "precise_artifacts_failed_bytes": execution.precise_artifacts_failed_bytes,
                "precise_reference_count": execution.precise_reference_count,
                "resource_budgets": resource_budget_metadata,
                "source_files_discovered": execution.source_files_discovered,
                "source_files_loaded": execution.source_files_loaded,
                "source_bytes_loaded": execution.source_bytes_loaded,
            }),
            &result,
        )
            .await;
        self.finalize_with_provenance("find_references", result, provenance_result)
    }

    #[tool(
        name = "go_to_definition",
        description = "Go to definitions for a symbol or source position.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn go_to_definition(
        &self,
        params: Parameters<GoToDefinitionParams>,
    ) -> Result<Json<GoToDefinitionResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let resource_budgets = self.find_references_resource_budgets();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("go_to_definition", move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut selected_symbol_id: Option<String> = None;
            let mut selected_precise_symbol: Option<String> = None;
            let mut resolution_precision: Option<String> = None;
            let mut resolution_source: Option<String> = None;
            let mut target_selection_candidate_count = 0usize;
            let mut target_selection_same_rank_count = 0usize;
            let mut effective_limit: Option<usize> = None;
            let mut precise_artifacts_ingested = 0usize;
            let mut precise_artifacts_failed = 0usize;
            let mut match_count = 0usize;

            let result = (|| -> Result<Json<GoToDefinitionResponse>, ErrorData> {
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                let corpora = server.collect_repository_symbol_corpora(
                    params_for_blocking.repository_id.as_deref(),
                )?;
                scoped_repository_ids = corpora
                    .iter()
                    .map(|corpus| corpus.repository_id.clone())
                    .collect::<Vec<_>>();

                let resolved_target = Self::resolve_navigation_target(
                    &corpora,
                    params_for_blocking.symbol.as_deref(),
                    params_for_blocking.path.as_deref(),
                    params_for_blocking.line,
                    params_for_blocking.column,
                    params_for_blocking.repository_id.as_deref(),
                )?;
                resolution_source = Some(resolved_target.resolution_source.to_owned());
                let symbol_query = resolved_target.symbol_query;
                target_selection_candidate_count = resolved_target.target.candidate_count;
                target_selection_same_rank_count =
                    resolved_target.target.selected_rank_candidate_count;
                let target = resolved_target.target.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());
                let target_corpus = resolved_target.target.corpus;

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_target = Self::select_precise_symbol_for_resolved_target(
                    graph.as_ref(),
                    &target_corpus.repository_id,
                    &target.root,
                    &symbol_query,
                    &target.symbol,
                );
                if let Some(precise_target) = &precise_target {
                    selected_precise_symbol = Some(precise_target.symbol.clone());
                }

                let mut precise_matches = precise_target
                    .as_ref()
                    .map(|precise_target| {
                        graph
                            .precise_occurrences_for_symbol(
                                &target_corpus.repository_id,
                                &precise_target.symbol,
                            )
                            .into_iter()
                            .filter(|occurrence| occurrence.is_definition())
                            .map(|occurrence| NavigationLocation {
                                symbol: if precise_target.display_name.is_empty() {
                                    target.symbol.name.clone()
                                } else {
                                    precise_target.display_name.clone()
                                },
                                repository_id: target_corpus.repository_id.clone(),
                                path: Self::canonicalize_navigation_path(
                                    &target.root,
                                    &occurrence.path,
                                ),
                                line: occurrence.range.start_line,
                                column: occurrence.range.start_column,
                                kind: Self::display_symbol_kind(&precise_target.kind),
                                precision: Some(
                                    Self::precise_match_precision(precise_coverage).to_owned(),
                                ),
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Self::sort_navigation_locations(&mut precise_matches);
                if precise_matches.len() > limit {
                    precise_matches.truncate(limit);
                }

                if !precise_matches.is_empty() {
                    let precision = Self::precise_resolution_precision(precise_coverage);
                    resolution_precision = Some(precision.to_owned());
                    match_count = precise_matches.len();
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "target_symbol_id": target.symbol.stable_id.clone(),
                        "target_precise_symbol": selected_precise_symbol.clone(),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "definition_count",
                            precise_matches.len(),
                        )
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    return Ok(Json(GoToDefinitionResponse {
                        matches: precise_matches,
                        metadata,
                        note,
                    }));
                }

                let mut matches = vec![NavigationLocation {
                    symbol: target.symbol.name.clone(),
                    repository_id: target_corpus.repository_id.clone(),
                    path: Self::relative_display_path(&target.root, &target.symbol.path),
                    line: target.symbol.line,
                    column: 1,
                    kind: Self::display_symbol_kind(target.symbol.kind.as_str()),
                    precision: Some("heuristic".to_owned()),
                }];
                Self::sort_navigation_locations(&mut matches);
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                resolution_precision = Some("heuristic".to_owned());
                match_count = matches.len();
                let metadata = json!({
                    "precision": "heuristic",
                    "heuristic": true,
                    "fallback_reason": "precise_absent",
                    "precise_absence_reason": Self::precise_absence_reason(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        0,
                    ),
                    "target_symbol_id": target.symbol.stable_id.clone(),
                    "resolution_source": resolution_source.clone(),
                    "target_selection": Self::navigation_target_selection_note(
                        &symbol_query,
                        &target,
                        target_selection_candidate_count,
                        target_selection_same_rank_count,
                    ),
                    "precise": Self::precise_note_with_count(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        "definition_count",
                        0,
                    )
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(GoToDefinitionResponse {
                    matches,
                    metadata,
                    note,
                }))
            })();

            NavigationToolExecution {
                result,
                scoped_repository_ids,
                selected_symbol_id,
                selected_precise_symbol,
                resolution_precision,
                resolution_source,
                effective_limit,
                precise_artifacts_ingested,
                precise_artifacts_failed,
                match_count,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
                "go_to_definition",
                repository_hint.as_deref(),
                json!({
                    "symbol": params.symbol.map(|symbol| Self::bounded_text(&symbol)),
                    "repository_id": repository_hint,
                    "path": params.path.map(|path| Self::bounded_text(&path)),
                    "line": params.line,
                    "column": params.column,
                    "limit": params.limit,
                    "effective_limit": execution.effective_limit,
                }),
                json!({
                    "scoped_repository_ids": execution.scoped_repository_ids,
                    "total_matches": execution.match_count,
                    "selected_symbol_id": execution.selected_symbol_id,
                    "selected_precise_symbol": execution.selected_precise_symbol,
                    "resolution_precision": execution.resolution_precision,
                    "resolution_source": execution.resolution_source,
                    "precise_artifacts_ingested": execution.precise_artifacts_ingested,
                    "precise_artifacts_failed": execution.precise_artifacts_failed,
                    "match_count": execution.match_count,
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("go_to_definition", result, provenance_result)
    }

    #[tool(
        name = "find_declarations",
        description = "Find declaration anchors for a symbol or source position.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn find_declarations(
        &self,
        params: Parameters<FindDeclarationsParams>,
    ) -> Result<Json<FindDeclarationsResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let resource_budgets = self.find_references_resource_budgets();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("find_declarations", move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut selected_symbol_id: Option<String> = None;
            let mut selected_precise_symbol: Option<String> = None;
            let mut resolution_precision: Option<String> = None;
            let mut resolution_source: Option<String> = None;
            let mut target_selection_candidate_count = 0usize;
            let mut target_selection_same_rank_count = 0usize;
            let mut effective_limit: Option<usize> = None;
            let mut precise_artifacts_ingested = 0usize;
            let mut precise_artifacts_failed = 0usize;
            let mut match_count = 0usize;

            let result = (|| -> Result<Json<FindDeclarationsResponse>, ErrorData> {
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                let corpora = server.collect_repository_symbol_corpora(
                    params_for_blocking.repository_id.as_deref(),
                )?;
                scoped_repository_ids = corpora
                    .iter()
                    .map(|corpus| corpus.repository_id.clone())
                    .collect::<Vec<_>>();

                let resolved_target = Self::resolve_navigation_target(
                    &corpora,
                    params_for_blocking.symbol.as_deref(),
                    params_for_blocking.path.as_deref(),
                    params_for_blocking.line,
                    params_for_blocking.column,
                    params_for_blocking.repository_id.as_deref(),
                )?;
                resolution_source = Some(resolved_target.resolution_source.to_owned());
                let symbol_query = resolved_target.symbol_query;
                target_selection_candidate_count = resolved_target.target.candidate_count;
                target_selection_same_rank_count =
                    resolved_target.target.selected_rank_candidate_count;
                let target = resolved_target.target.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());
                let target_corpus = resolved_target.target.corpus;

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_target = Self::select_precise_symbol_for_resolved_target(
                    graph.as_ref(),
                    &target_corpus.repository_id,
                    &target.root,
                    &symbol_query,
                    &target.symbol,
                );
                if let Some(precise_target) = &precise_target {
                    selected_precise_symbol = Some(precise_target.symbol.clone());
                }

                let mut precise_matches = precise_target
                    .as_ref()
                    .map(|precise_target| {
                        graph
                            .precise_occurrences_for_symbol(
                                &target_corpus.repository_id,
                                &precise_target.symbol,
                            )
                            .into_iter()
                            .filter(|occurrence| occurrence.is_definition())
                            .map(|occurrence| NavigationLocation {
                                symbol: if precise_target.display_name.is_empty() {
                                    target.symbol.name.clone()
                                } else {
                                    precise_target.display_name.clone()
                                },
                                repository_id: target_corpus.repository_id.clone(),
                                path: Self::canonicalize_navigation_path(
                                    &target.root,
                                    &occurrence.path,
                                ),
                                line: occurrence.range.start_line,
                                column: occurrence.range.start_column,
                                kind: Self::display_symbol_kind(&precise_target.kind),
                                precision: Some(
                                    Self::precise_match_precision(precise_coverage).to_owned(),
                                ),
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Self::sort_navigation_locations(&mut precise_matches);
                if precise_matches.len() > limit {
                    precise_matches.truncate(limit);
                }

                if !precise_matches.is_empty() {
                    let precision = Self::precise_resolution_precision(precise_coverage);
                    resolution_precision = Some(precision.to_owned());
                    match_count = precise_matches.len();
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "declaration_mode": "definition_anchor_v1",
                        "target_symbol_id": target.symbol.stable_id.clone(),
                        "target_precise_symbol": selected_precise_symbol.clone(),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "declaration_count",
                            precise_matches.len(),
                        )
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    return Ok(Json(FindDeclarationsResponse {
                        matches: precise_matches,
                        metadata,
                        note,
                    }));
                }

                let mut matches = vec![NavigationLocation {
                    symbol: target.symbol.name.clone(),
                    repository_id: target_corpus.repository_id.clone(),
                    path: Self::relative_display_path(&target.root, &target.symbol.path),
                    line: target.symbol.line,
                    column: 1,
                    kind: Self::display_symbol_kind(target.symbol.kind.as_str()),
                    precision: Some("heuristic".to_owned()),
                }];
                Self::sort_navigation_locations(&mut matches);
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                resolution_precision = Some("heuristic".to_owned());
                match_count = matches.len();
                let metadata = json!({
                    "precision": "heuristic",
                    "heuristic": true,
                    "fallback_reason": "precise_absent",
                    "precise_absence_reason": Self::precise_absence_reason(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        0,
                    ),
                    "declaration_mode": "definition_anchor_v1",
                    "target_symbol_id": target.symbol.stable_id.clone(),
                    "resolution_source": resolution_source.clone(),
                    "target_selection": Self::navigation_target_selection_note(
                        &symbol_query,
                        &target,
                        target_selection_candidate_count,
                        target_selection_same_rank_count,
                    ),
                    "precise": Self::precise_note_with_count(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        "declaration_count",
                        0,
                    )
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(FindDeclarationsResponse {
                    matches,
                    metadata,
                    note,
                }))
            })();

            NavigationToolExecution {
                result,
                scoped_repository_ids,
                selected_symbol_id,
                selected_precise_symbol,
                resolution_precision,
                resolution_source,
                effective_limit,
                precise_artifacts_ingested,
                precise_artifacts_failed,
                match_count,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
                "find_declarations",
                repository_hint.as_deref(),
                json!({
                    "symbol": params.symbol.map(|symbol| Self::bounded_text(&symbol)),
                    "repository_id": repository_hint,
                    "path": params.path.map(|path| Self::bounded_text(&path)),
                    "line": params.line,
                    "column": params.column,
                    "limit": params.limit,
                    "effective_limit": execution.effective_limit,
                }),
                json!({
                    "scoped_repository_ids": execution.scoped_repository_ids,
                    "selected_symbol_id": execution.selected_symbol_id,
                    "selected_precise_symbol": execution.selected_precise_symbol,
                    "resolution_precision": execution.resolution_precision,
                    "resolution_source": execution.resolution_source,
                    "precise_artifacts_ingested": execution.precise_artifacts_ingested,
                    "precise_artifacts_failed": execution.precise_artifacts_failed,
                    "match_count": execution.match_count,
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("find_declarations", result, provenance_result)
    }

    #[tool(
        name = "find_implementations",
        description = "Find implementations for a symbol or source position.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn find_implementations(
        &self,
        params: Parameters<FindImplementationsParams>,
    ) -> Result<Json<FindImplementationsResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let resource_budgets = self.find_references_resource_budgets();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("find_implementations", move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut selected_symbol_id: Option<String> = None;
            let mut selected_precise_symbol: Option<String> = None;
            let mut resolution_precision: Option<String> = None;
            let mut resolution_source: Option<String> = None;
            let mut target_selection_candidate_count = 0usize;
            let mut target_selection_same_rank_count = 0usize;
            let mut effective_limit: Option<usize> = None;
            let mut precise_artifacts_ingested = 0usize;
            let mut precise_artifacts_failed = 0usize;
            let mut match_count = 0usize;

            let result = (|| -> Result<Json<FindImplementationsResponse>, ErrorData> {
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                let corpora = server.collect_repository_symbol_corpora(
                    params_for_blocking.repository_id.as_deref(),
                )?;
                scoped_repository_ids = corpora
                    .iter()
                    .map(|corpus| corpus.repository_id.clone())
                    .collect::<Vec<_>>();

                let resolved_target = Self::resolve_navigation_target(
                    &corpora,
                    params_for_blocking.symbol.as_deref(),
                    params_for_blocking.path.as_deref(),
                    params_for_blocking.line,
                    params_for_blocking.column,
                    params_for_blocking.repository_id.as_deref(),
                )?;
                resolution_source = Some(resolved_target.resolution_source.to_owned());
                let symbol_query = resolved_target.symbol_query;
                target_selection_candidate_count = resolved_target.target.candidate_count;
                target_selection_same_rank_count =
                    resolved_target.target.selected_rank_candidate_count;
                let target = resolved_target.target.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());
                let target_corpus = resolved_target.target.corpus;

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_targets = Self::matching_precise_symbols_for_resolved_target(
                    graph.as_ref(),
                    &target_corpus.repository_id,
                    &target.root,
                    &symbol_query,
                    &target.symbol,
                );
                let mut precise_matches = Vec::new();
                for precise_target in &precise_targets {
                    let matches = Self::precise_implementation_matches_for_symbol(
                        graph.as_ref(),
                        &target_corpus.repository_id,
                        &target.root,
                        precise_coverage,
                        precise_target,
                    );
                    if !matches.is_empty() {
                        selected_precise_symbol = Some(precise_target.symbol.clone());
                        precise_matches = matches;
                        break;
                    }
                }
                if precise_matches.is_empty() {
                    for precise_target in &precise_targets {
                        let matches = Self::precise_implementation_matches_from_occurrences(
                            graph.as_ref(),
                            target_corpus.as_ref(),
                            &target.root,
                            &target.symbol.name,
                            precise_coverage,
                            precise_target,
                        );
                        if !matches.is_empty() {
                            selected_precise_symbol = Some(precise_target.symbol.clone());
                            precise_matches = matches;
                            break;
                        }
                    }
                }
                if precise_matches.len() > limit {
                    precise_matches.truncate(limit);
                }

                if !precise_matches.is_empty() {
                    let precision = Self::precise_resolution_precision(precise_coverage);
                    resolution_precision = Some(precision.to_owned());
                    match_count = precise_matches.len();
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "target_symbol_id": target.symbol.stable_id.clone(),
                        "target_precise_symbol": selected_precise_symbol.clone(),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "implementation_count",
                            precise_matches.len(),
                        )
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    return Ok(Json(FindImplementationsResponse {
                        matches: precise_matches,
                        metadata,
                        note,
                    }));
                }

                let mut matches = graph
                    .incoming_adjacency(&target.symbol.stable_id)
                    .into_iter()
                    .filter(|adjacent| {
                        matches!(
                            adjacent.relation,
                            RelationKind::Implements | RelationKind::Extends
                        )
                    })
                    .map(|adjacent| ImplementationMatch {
                        symbol: adjacent.symbol.display_name,
                        kind: Self::display_symbol_kind(&adjacent.symbol.kind),
                        repository_id: target_corpus.repository_id.clone(),
                        path: Self::canonicalize_navigation_path(
                            &target.root,
                            &adjacent.symbol.path,
                        ),
                        line: adjacent.symbol.line,
                        column: 1,
                        relation: Some(adjacent.relation.as_str().to_owned()),
                        precision: Some("heuristic".to_owned()),
                        fallback_reason: Some("precise_absent".to_owned()),
                    })
                    .collect::<Vec<_>>();
                matches.extend(Self::heuristic_implementation_matches_from_symbols(
                    &target.symbol,
                    target_corpus.as_ref(),
                    &target.root,
                ));
                Self::sort_implementation_matches(&mut matches);
                matches.dedup_by(|left, right| {
                    left.repository_id == right.repository_id
                        && left.path == right.path
                        && left.line == right.line
                        && left.column == right.column
                        && left.symbol == right.symbol
                        && left.kind == right.kind
                        && left.relation == right.relation
                        && left.precision == right.precision
                        && left.fallback_reason == right.fallback_reason
                });
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                resolution_precision = Some("heuristic".to_owned());
                match_count = matches.len();
                let metadata = json!({
                    "precision": "heuristic",
                    "heuristic": true,
                    "fallback_reason": "precise_absent",
                    "precise_absence_reason": Self::precise_absence_reason(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        0,
                    ),
                    "target_symbol_id": target.symbol.stable_id.clone(),
                    "resolution_source": resolution_source.clone(),
                    "target_selection": Self::navigation_target_selection_note(
                        &symbol_query,
                        &target,
                        target_selection_candidate_count,
                        target_selection_same_rank_count,
                    ),
                    "precise": Self::precise_note_with_count(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        "implementation_count",
                        matches.len(),
                    )
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(FindImplementationsResponse {
                    matches,
                    metadata,
                    note,
                }))
            })();

            NavigationToolExecution {
                result,
                scoped_repository_ids,
                selected_symbol_id,
                selected_precise_symbol,
                resolution_precision,
                resolution_source,
                effective_limit,
                precise_artifacts_ingested,
                precise_artifacts_failed,
                match_count,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
                "find_implementations",
                repository_hint.as_deref(),
                json!({
                    "symbol": params.symbol.map(|symbol| Self::bounded_text(&symbol)),
                    "repository_id": repository_hint,
                    "path": params.path.map(|path| Self::bounded_text(&path)),
                    "line": params.line,
                    "column": params.column,
                    "limit": params.limit,
                    "effective_limit": execution.effective_limit,
                }),
                json!({
                    "scoped_repository_ids": execution.scoped_repository_ids,
                    "selected_symbol_id": execution.selected_symbol_id,
                    "selected_precise_symbol": execution.selected_precise_symbol,
                    "resolution_precision": execution.resolution_precision,
                    "resolution_source": execution.resolution_source,
                    "precise_artifacts_ingested": execution.precise_artifacts_ingested,
                    "precise_artifacts_failed": execution.precise_artifacts_failed,
                    "match_count": execution.match_count,
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("find_implementations", result, provenance_result)
    }

    #[tool(
        name = "incoming_calls",
        description = "Find incoming callers for a symbol or source position.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn incoming_calls(
        &self,
        params: Parameters<IncomingCallsParams>,
    ) -> Result<Json<IncomingCallsResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let resource_budgets = self.find_references_resource_budgets();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("incoming_calls", move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut selected_symbol_id: Option<String> = None;
            let mut selected_precise_symbol: Option<String> = None;
            let mut resolution_precision: Option<String> = None;
            let mut resolution_source: Option<String> = None;
            let mut target_selection_candidate_count = 0usize;
            let mut target_selection_same_rank_count = 0usize;
            let mut effective_limit: Option<usize> = None;
            let mut precise_artifacts_ingested = 0usize;
            let mut precise_artifacts_failed = 0usize;
            let mut match_count = 0usize;

            let result = (|| -> Result<Json<IncomingCallsResponse>, ErrorData> {
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                let corpora = server.collect_repository_symbol_corpora(
                    params_for_blocking.repository_id.as_deref(),
                )?;
                scoped_repository_ids = corpora
                    .iter()
                    .map(|corpus| corpus.repository_id.clone())
                    .collect::<Vec<_>>();

                let resolved_target = Self::resolve_navigation_target(
                    &corpora,
                    params_for_blocking.symbol.as_deref(),
                    params_for_blocking.path.as_deref(),
                    params_for_blocking.line,
                    params_for_blocking.column,
                    params_for_blocking.repository_id.as_deref(),
                )?;
                resolution_source = Some(resolved_target.resolution_source.to_owned());
                let symbol_query = resolved_target.symbol_query;
                target_selection_candidate_count = resolved_target.target.candidate_count;
                target_selection_same_rank_count =
                    resolved_target.target.selected_rank_candidate_count;
                let target = resolved_target.target.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());
                let target_corpus = resolved_target.target.corpus;

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_targets = Self::matching_precise_symbols_for_resolved_target(
                    graph.as_ref(),
                    &target_corpus.repository_id,
                    &target.root,
                    &symbol_query,
                    &target.symbol,
                );
                let mut precise_matches = Vec::new();
                for precise_target in &precise_targets {
                    let matches = Self::precise_incoming_matches_from_relationships(
                        graph.as_ref(),
                        &target_corpus.repository_id,
                        &target.root,
                        &target.symbol.name,
                        precise_coverage,
                        precise_target,
                    );
                    if !matches.is_empty() {
                        selected_precise_symbol = Some(precise_target.symbol.clone());
                        precise_matches = matches;
                        break;
                    }
                }
                if precise_matches.is_empty() {
                    for precise_target in &precise_targets {
                        let matches = Self::precise_incoming_matches_from_occurrences(
                            graph.as_ref(),
                            target_corpus.as_ref(),
                            &target.root,
                            &target.symbol.name,
                            precise_coverage,
                            precise_target,
                            &target.symbol.stable_id,
                        );
                        if !matches.is_empty() {
                            selected_precise_symbol = Some(precise_target.symbol.clone());
                            precise_matches = matches;
                            break;
                        }
                    }
                }
                if precise_matches.len() > limit {
                    precise_matches.truncate(limit);
                }

                if !precise_matches.is_empty() {
                    let precision = Self::precise_resolution_precision(precise_coverage);
                    resolution_precision = Some(precision.to_owned());
                    match_count = precise_matches.len();
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "target_symbol_id": target.symbol.stable_id.clone(),
                        "target_precise_symbol": selected_precise_symbol.clone(),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "incoming_count",
                            precise_matches.len(),
                        )
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    return Ok(Json(IncomingCallsResponse {
                        matches: precise_matches,
                        metadata,
                        note,
                    }));
                }

                let mut matches = graph
                    .incoming_adjacency(&target.symbol.stable_id)
                    .into_iter()
                    .filter(|adjacent| Self::is_heuristic_call_relation(adjacent.relation))
                    .map(|adjacent| CallHierarchyMatch {
                        source_symbol: adjacent.symbol.display_name,
                        target_symbol: target.symbol.name.clone(),
                        repository_id: target_corpus.repository_id.clone(),
                        path: Self::canonicalize_navigation_path(
                            &target.root,
                            &adjacent.symbol.path,
                        ),
                        line: adjacent.symbol.line,
                        column: 1,
                        relation: adjacent.relation.as_str().to_owned(),
                        precision: Some("heuristic".to_owned()),
                        call_path: None,
                        call_line: None,
                        call_column: None,
                        call_end_line: None,
                        call_end_column: None,
                    })
                    .collect::<Vec<_>>();
                Self::sort_call_hierarchy_matches(&mut matches);
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                resolution_precision = Some("heuristic".to_owned());
                match_count = matches.len();
                let metadata = json!({
                    "precision": "heuristic",
                    "heuristic": true,
                    "fallback_reason": "precise_absent",
                    "precise_absence_reason": Self::precise_absence_reason(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        0,
                    ),
                    "target_symbol_id": target.symbol.stable_id.clone(),
                    "resolution_source": resolution_source.clone(),
                    "target_selection": Self::navigation_target_selection_note(
                        &symbol_query,
                        &target,
                        target_selection_candidate_count,
                        target_selection_same_rank_count,
                    ),
                    "precise": Self::precise_note_with_count(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        "incoming_count",
                        0,
                    )
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(IncomingCallsResponse {
                    matches,
                    metadata,
                    note,
                }))
            })();

            NavigationToolExecution {
                result,
                scoped_repository_ids,
                selected_symbol_id,
                selected_precise_symbol,
                resolution_precision,
                resolution_source,
                effective_limit,
                precise_artifacts_ingested,
                precise_artifacts_failed,
                match_count,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
                "incoming_calls",
                repository_hint.as_deref(),
                json!({
                    "symbol": params.symbol.map(|symbol| Self::bounded_text(&symbol)),
                    "repository_id": repository_hint,
                    "path": params.path.map(|path| Self::bounded_text(&path)),
                    "line": params.line,
                    "column": params.column,
                    "limit": params.limit,
                    "effective_limit": execution.effective_limit,
                }),
                json!({
                    "scoped_repository_ids": execution.scoped_repository_ids,
                    "selected_symbol_id": execution.selected_symbol_id,
                    "selected_precise_symbol": execution.selected_precise_symbol,
                    "resolution_precision": execution.resolution_precision,
                    "resolution_source": execution.resolution_source,
                    "precise_artifacts_ingested": execution.precise_artifacts_ingested,
                    "precise_artifacts_failed": execution.precise_artifacts_failed,
                    "match_count": execution.match_count,
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("incoming_calls", result, provenance_result)
    }

    #[tool(
        name = "outgoing_calls",
        description = "Find outgoing callees for a symbol or source position.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn outgoing_calls(
        &self,
        params: Parameters<OutgoingCallsParams>,
    ) -> Result<Json<OutgoingCallsResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let resource_budgets = self.find_references_resource_budgets();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("outgoing_calls", move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut selected_symbol_id: Option<String> = None;
            let mut selected_precise_symbol: Option<String> = None;
            let mut resolution_precision: Option<String> = None;
            let mut resolution_source: Option<String> = None;
            let mut target_selection_candidate_count = 0usize;
            let mut target_selection_same_rank_count = 0usize;
            let mut effective_limit: Option<usize> = None;
            let mut precise_artifacts_ingested = 0usize;
            let mut precise_artifacts_failed = 0usize;
            let mut match_count = 0usize;

            let result = (|| -> Result<Json<OutgoingCallsResponse>, ErrorData> {
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                let corpora = server.collect_repository_symbol_corpora(
                    params_for_blocking.repository_id.as_deref(),
                )?;
                scoped_repository_ids = corpora
                    .iter()
                    .map(|corpus| corpus.repository_id.clone())
                    .collect::<Vec<_>>();

                let resolved_target = Self::resolve_navigation_target(
                    &corpora,
                    params_for_blocking.symbol.as_deref(),
                    params_for_blocking.path.as_deref(),
                    params_for_blocking.line,
                    params_for_blocking.column,
                    params_for_blocking.repository_id.as_deref(),
                )?;
                resolution_source = Some(resolved_target.resolution_source.to_owned());
                let symbol_query = resolved_target.symbol_query;
                target_selection_candidate_count = resolved_target.target.candidate_count;
                target_selection_same_rank_count =
                    resolved_target.target.selected_rank_candidate_count;
                let target = resolved_target.target.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());
                let target_corpus = resolved_target.target.corpus;

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_targets = Self::matching_precise_symbols_for_resolved_target(
                    graph.as_ref(),
                    &target_corpus.repository_id,
                    &target.root,
                    &symbol_query,
                    &target.symbol,
                );
                let mut precise_matches = Vec::new();
                for precise_target in &precise_targets {
                    let mut matches = graph
                        .precise_relationships_from_symbol(
                            &target_corpus.repository_id,
                            &precise_target.symbol,
                        )
                        .into_iter()
                        .filter(|relationship| {
                            relationship.kind == PreciseRelationshipKind::Reference
                        })
                        .filter_map(|relationship| {
                            let callee_symbol = graph
                                .precise_symbol(
                                    &target_corpus.repository_id,
                                    &relationship.to_symbol,
                                )?
                                .clone();
                            if !Self::is_precise_callable_kind(&callee_symbol.kind) {
                                return None;
                            }
                            let callee_definition = Self::precise_definition_occurrence_for_symbol(
                                graph.as_ref(),
                                &target_corpus.repository_id,
                                &relationship.to_symbol,
                            )?;
                            Some(CallHierarchyMatch {
                                source_symbol: if precise_target.display_name.is_empty() {
                                    target.symbol.name.clone()
                                } else {
                                    precise_target.display_name.clone()
                                },
                                target_symbol: if callee_symbol.display_name.is_empty() {
                                    callee_symbol.symbol
                                } else {
                                    callee_symbol.display_name
                                },
                                repository_id: target_corpus.repository_id.clone(),
                                path: Self::canonicalize_navigation_path(
                                    &target.root,
                                    &callee_definition.path,
                                ),
                                line: callee_definition.range.start_line,
                                column: callee_definition.range.start_column,
                                relation: "calls".to_owned(),
                                precision: Some(
                                    Self::precise_match_precision(precise_coverage).to_owned(),
                                ),
                                call_path: None,
                                call_line: None,
                                call_column: None,
                                call_end_line: None,
                                call_end_column: None,
                            })
                        })
                        .collect::<Vec<_>>();
                    Self::sort_call_hierarchy_matches(&mut matches);
                    if !matches.is_empty() {
                        selected_precise_symbol = Some(precise_target.symbol.clone());
                        precise_matches = matches;
                        break;
                    }
                }
                if precise_matches.is_empty() {
                    for precise_target in &precise_targets {
                        let matches = Self::precise_outgoing_matches_from_occurrences(
                            graph.as_ref(),
                            target_corpus.as_ref(),
                            &target.root,
                            &target.symbol.name,
                            precise_coverage,
                            precise_target,
                            &target.symbol.stable_id,
                        );
                        if !matches.is_empty() {
                            selected_precise_symbol = Some(precise_target.symbol.clone());
                            precise_matches = matches;
                            break;
                        }
                    }
                }
                if precise_matches.len() > limit {
                    precise_matches.truncate(limit);
                }

                if !precise_matches.is_empty() {
                    let precision = Self::precise_resolution_precision(precise_coverage);
                    resolution_precision = Some(precision.to_owned());
                    match_count = precise_matches.len();
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "target_symbol_id": target.symbol.stable_id.clone(),
                        "target_precise_symbol": selected_precise_symbol.clone(),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "outgoing_count",
                            precise_matches.len(),
                        )
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    return Ok(Json(OutgoingCallsResponse {
                        matches: precise_matches,
                        metadata,
                        note,
                    }));
                }

                let mut matches = graph
                    .outgoing_adjacency(&target.symbol.stable_id)
                    .into_iter()
                    .filter(|adjacent| {
                        Self::is_heuristic_call_relation(adjacent.relation)
                            && Self::is_heuristic_callable_kind(&adjacent.symbol.kind)
                    })
                    .map(|adjacent| CallHierarchyMatch {
                        source_symbol: target.symbol.name.clone(),
                        target_symbol: adjacent.symbol.display_name,
                        repository_id: target_corpus.repository_id.clone(),
                        path: Self::canonicalize_navigation_path(
                            &target.root,
                            &adjacent.symbol.path,
                        ),
                        line: adjacent.symbol.line,
                        column: 1,
                        relation: adjacent.relation.as_str().to_owned(),
                        precision: Some("heuristic".to_owned()),
                        call_path: None,
                        call_line: None,
                        call_column: None,
                        call_end_line: None,
                        call_end_column: None,
                    })
                    .collect::<Vec<_>>();
                Self::sort_call_hierarchy_matches(&mut matches);
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                resolution_precision = Some("heuristic".to_owned());
                match_count = matches.len();
                let metadata = json!({
                    "precision": "heuristic",
                    "heuristic": true,
                    "fallback_reason": "precise_absent",
                    "precise_absence_reason": Self::precise_absence_reason(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        0,
                    ),
                    "target_symbol_id": target.symbol.stable_id.clone(),
                    "resolution_source": resolution_source.clone(),
                    "target_selection": Self::navigation_target_selection_note(
                        &symbol_query,
                        &target,
                        target_selection_candidate_count,
                        target_selection_same_rank_count,
                    ),
                    "precise": Self::precise_note_with_count(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        "outgoing_count",
                        0,
                    )
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(OutgoingCallsResponse {
                    matches,
                    metadata,
                    note,
                }))
            })();

            NavigationToolExecution {
                result,
                scoped_repository_ids,
                selected_symbol_id,
                selected_precise_symbol,
                resolution_precision,
                resolution_source,
                effective_limit,
                precise_artifacts_ingested,
                precise_artifacts_failed,
                match_count,
            }
        })
        .await?;

        let result = execution.result;
        let provenance_result = self
            .record_provenance_blocking(
                "outgoing_calls",
                repository_hint.as_deref(),
                json!({
                    "symbol": params.symbol.map(|symbol| Self::bounded_text(&symbol)),
                    "repository_id": repository_hint,
                    "path": params.path.map(|path| Self::bounded_text(&path)),
                    "line": params.line,
                    "column": params.column,
                    "limit": params.limit,
                    "effective_limit": execution.effective_limit,
                }),
                json!({
                    "scoped_repository_ids": execution.scoped_repository_ids,
                    "selected_symbol_id": execution.selected_symbol_id,
                    "selected_precise_symbol": execution.selected_precise_symbol,
                    "resolution_precision": execution.resolution_precision,
                    "resolution_source": execution.resolution_source,
                    "precise_artifacts_ingested": execution.precise_artifacts_ingested,
                    "precise_artifacts_failed": execution.precise_artifacts_failed,
                    "match_count": execution.match_count,
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("outgoing_calls", result, provenance_result)
    }

    #[tool(
        name = "document_symbols",
        description = "Outline symbols in one supported source file.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn document_symbols(
        &self,
        params: Parameters<DocumentSymbolsParams>,
    ) -> Result<Json<DocumentSymbolsResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("document_symbols", move || {
            let mut resolved_repository_id: Option<String> = None;
            let mut resolved_path: Option<String> = None;
            let mut symbol_count = 0usize;

            let result = (|| -> Result<Json<DocumentSymbolsResponse>, ErrorData> {
                let read_params = ReadFileParams {
                    path: params_for_blocking.path.clone(),
                    repository_id: params_for_blocking.repository_id.clone(),
                    max_bytes: None,
                    line_start: None,
                    line_end: None,
                };
                let (repository_id, absolute_path, display_path) =
                    server.resolve_file_path(&read_params)?;
                resolved_repository_id = Some(repository_id.clone());
                resolved_path = Some(display_path.clone());

                let language =
                    supported_language_for_path(&absolute_path, LanguageCapability::DocumentSymbols)
                        .ok_or_else(|| {
                            Self::invalid_params(
                                LanguageCapability::DocumentSymbols
                                    .unsupported_file_message("document_symbols"),
                                Some(json!({
                                    "path": display_path.clone(),
                                    "supported_extensions": LanguageCapability::DocumentSymbols.supported_extensions(),
                                })),
                            )
                        })?;
                let metadata = fs::metadata(&absolute_path).map_err(|err| {
                    Self::internal(
                        format!(
                            "failed to stat source file {}: {err}",
                            absolute_path.display()
                        ),
                        None,
                    )
                })?;
                let bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
                if bytes > server.config.max_file_bytes {
                    return Err(Self::invalid_params(
                        format!("file exceeds max_bytes={}", server.config.max_file_bytes),
                        Some(json!({
                            "path": display_path.clone(),
                            "bytes": bytes,
                            "max_bytes": server.config.max_file_bytes,
                            "config_max_file_bytes": server.config.max_file_bytes,
                            "suggested_max_bytes": bytes.min(server.config.max_file_bytes),
                        })),
                    ));
                }
                let source = fs::read_to_string(&absolute_path).map_err(|err| {
                    Self::internal(
                        format!(
                            "failed to read source file {}: {err}",
                            absolute_path.display()
                        ),
                        None,
                    )
                })?;
                let symbols = extract_symbols_from_source(language, &absolute_path, &source)
                    .map_err(Self::map_frigg_error)?;

                let outline =
                    Self::build_document_symbol_tree(&symbols, &repository_id, &display_path);
                symbol_count = outline.len();

                let metadata = if language == SymbolLanguage::Blade {
                    let blade_evidence =
                        extract_blade_source_evidence_from_source(&absolute_path, &source, &symbols);
                    json!({
                        "source": "tree_sitter",
                        "language": language.as_str(),
                        "symbol_count": symbol_count,
                        "heuristic": false,
                        "blade": {
                            "relations_detected": blade_evidence.relations.len(),
                            "livewire_components": blade_evidence.livewire_components,
                            "wire_directives": blade_evidence.wire_directives,
                            "flux_components": blade_evidence.flux_components,
                            "flux_registry_version": FLUX_REGISTRY_VERSION,
                            "flux_hints": blade_evidence.flux_hints,
                        },
                    })
                } else if language == SymbolLanguage::Php {
                    let php_metadata = extract_php_source_evidence_from_source(
                        &absolute_path,
                        &source,
                        &symbols,
                    )
                    .ok()
                    .map(|evidence| {
                        json!({
                            "canonical_name_count": evidence.canonical_names_by_stable_id.len(),
                            "type_evidence_count": evidence.type_evidence.len(),
                            "target_evidence_count": evidence.target_evidence.len(),
                            "literal_evidence_count": evidence.literal_evidence.len(),
                        })
                    });
                    json!({
                        "source": "tree_sitter",
                        "language": language.as_str(),
                        "symbol_count": symbol_count,
                        "heuristic": false,
                        "php": php_metadata,
                    })
                } else {
                    json!({
                        "source": "tree_sitter",
                        "language": language.as_str(),
                        "symbol_count": symbol_count,
                        "heuristic": false,
                    })
                };
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(DocumentSymbolsResponse {
                    symbols: outline,
                    metadata,
                    note,
                }))
            })();

            (result, resolved_repository_id, resolved_path, symbol_count)
        })
        .await?;

        let (result, resolved_repository_id, resolved_path, symbol_count) = execution;
        let provenance_result = self
            .record_provenance_blocking(
                "document_symbols",
                repository_hint.as_deref(),
                json!({
                    "repository_id": repository_hint,
                    "path": Self::bounded_text(&params.path),
                }),
                json!({
                    "resolved_repository_id": resolved_repository_id,
                    "resolved_path": resolved_path,
                    "symbol_count": symbol_count,
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("document_symbols", result, provenance_result)
    }

    #[tool(
        name = "search_structural",
        description = "Run tree-sitter structural queries for supported source files.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn search_structural(
        &self,
        params: Parameters<SearchStructuralParams>,
    ) -> Result<Json<SearchStructuralResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = Self::run_blocking_task("search_structural", move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut effective_limit: Option<usize> = None;
            let mut language_filter: Option<String> = None;
            let mut files_scanned = 0usize;
            let mut files_matched = 0usize;
            let mut diagnostics_count = 0usize;
            let mut blade_relations_detected = 0usize;
            let mut blade_livewire_components = BTreeSet::new();
            let mut blade_wire_directives = BTreeSet::new();
            let mut blade_flux_components = BTreeSet::new();

            let result = (|| -> Result<Json<SearchStructuralResponse>, ErrorData> {
                let query = params_for_blocking.query.trim().to_owned();
                if query.is_empty() {
                    return Err(Self::invalid_params("query must not be empty", None));
                }
                if query.chars().count() > Self::SEARCH_STRUCTURAL_MAX_QUERY_CHARS {
                    return Err(Self::invalid_params(
                        "query exceeds structural search maximum length",
                        Some(json!({
                            "query_chars": query.chars().count(),
                            "max_query_chars": Self::SEARCH_STRUCTURAL_MAX_QUERY_CHARS,
                        })),
                    ));
                }

                let path_regex = match params_for_blocking.path_regex.as_ref() {
                    Some(raw) => Some(compile_safe_regex(raw).map_err(|err| {
                        Self::invalid_params(
                            format!("invalid path_regex: {err}"),
                            Some(json!({
                                "path_regex": raw,
                                "regex_error_code": err.code(),
                            })),
                        )
                    })?),
                    None => None,
                };

                let target_language =
                    Self::parse_symbol_language(params_for_blocking.language.as_deref())?;
                language_filter = target_language.map(|language| language.as_str().to_owned());
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                let corpora = server.collect_repository_symbol_corpora(
                    params_for_blocking.repository_id.as_deref(),
                )?;
                scoped_repository_ids = corpora
                    .iter()
                    .map(|corpus| corpus.repository_id.clone())
                    .collect::<Vec<_>>();

                let mut matches = Vec::new();
                for corpus in corpora {
                    for source_path in &corpus.source_paths {
                        let Some(language) = supported_language_for_path(
                            source_path,
                            LanguageCapability::StructuralSearch,
                        ) else {
                            continue;
                        };
                        if let Some(target_language) = target_language {
                            if language != target_language {
                                continue;
                            }
                        }
                        let display_path =
                            Self::relative_display_path(&corpus.root, source_path.as_path());
                        if let Some(path_regex) = &path_regex {
                            if !path_regex.is_match(&display_path) {
                                continue;
                            }
                        }
                        files_scanned = files_scanned.saturating_add(1);

                        let source = match fs::read_to_string(source_path) {
                            Ok(source) => source,
                            Err(err) => {
                                diagnostics_count = diagnostics_count.saturating_add(1);
                                warn!(
                                    repository_id = corpus.repository_id,
                                    path = %source_path.display(),
                                    error = %err,
                                    "skipping source file for structural search"
                                );
                                continue;
                            }
                        };

                        let structural_matches =
                            search_structural_in_source(language, source_path, &source, &query)
                                .map_err(Self::map_frigg_error)?;
                        if language == SymbolLanguage::Blade {
                            let blade_evidence =
                                extract_blade_source_evidence_from_source(source_path, &source, &[]);
                            blade_relations_detected = blade_relations_detected
                                .saturating_add(blade_evidence.relations.len());
                            blade_livewire_components
                                .extend(blade_evidence.livewire_components.into_iter());
                            blade_wire_directives
                                .extend(blade_evidence.wire_directives.into_iter());
                            blade_flux_components
                                .extend(blade_evidence.flux_components.into_iter());
                        }
                        files_matched = files_matched
                            .saturating_add(usize::from(!structural_matches.is_empty()));

                        for structural_match in structural_matches {
                            matches.push(crate::mcp::types::StructuralMatch {
                                repository_id: corpus.repository_id.clone(),
                                path: display_path.clone(),
                                line: structural_match.span.start_line,
                                column: structural_match.span.start_column,
                                end_line: structural_match.span.end_line,
                                end_column: structural_match.span.end_column,
                                excerpt: structural_match.excerpt,
                            });
                        }
                    }
                }

                matches.sort_by(|left, right| {
                    left.repository_id
                        .cmp(&right.repository_id)
                        .then(left.path.cmp(&right.path))
                        .then(left.line.cmp(&right.line))
                        .then(left.column.cmp(&right.column))
                        .then(left.end_line.cmp(&right.end_line))
                        .then(left.end_column.cmp(&right.end_column))
                        .then(left.excerpt.cmp(&right.excerpt))
                });
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                let metadata = if target_language == Some(SymbolLanguage::Blade) {
                    json!({
                        "source": "tree_sitter_query",
                        "language": language_filter.clone().unwrap_or_else(|| "mixed".to_owned()),
                        "heuristic": false,
                        "diagnostics_count": diagnostics_count,
                        "files_scanned": files_scanned,
                        "files_matched": files_matched,
                        "blade": {
                            "relations_detected": blade_relations_detected,
                            "livewire_components": blade_livewire_components.into_iter().collect::<Vec<_>>(),
                            "wire_directives": blade_wire_directives.into_iter().collect::<Vec<_>>(),
                            "flux_components": blade_flux_components.into_iter().collect::<Vec<_>>(),
                            "flux_registry_version": FLUX_REGISTRY_VERSION,
                        },
                    })
                } else {
                    json!({
                        "source": "tree_sitter_query",
                        "language": language_filter.clone().unwrap_or_else(|| "mixed".to_owned()),
                        "heuristic": false,
                        "diagnostics_count": diagnostics_count,
                        "files_scanned": files_scanned,
                        "files_matched": files_matched,
                    })
                };
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(SearchStructuralResponse {
                    matches,
                    metadata,
                    note,
                }))
            })();

            (
                result,
                scoped_repository_ids,
                effective_limit,
                language_filter,
                files_scanned,
                files_matched,
                diagnostics_count,
            )
        })
        .await?;

        let (
            result,
            scoped_repository_ids,
            effective_limit,
            language_filter,
            files_scanned,
            files_matched,
            diagnostics_count,
        ) = execution;
        let provenance_result = self
            .record_provenance_blocking(
                "search_structural",
                repository_hint.as_deref(),
                json!({
                    "repository_id": repository_hint,
                    "query": Self::bounded_text(&params.query),
                    "language": params.language,
                    "path_regex": params.path_regex.map(|raw| Self::bounded_text(&raw)),
                    "limit": params.limit,
                    "effective_limit": effective_limit,
                }),
                json!({
                    "scoped_repository_ids": scoped_repository_ids,
                    "language_filter": language_filter,
                    "files_scanned": files_scanned,
                    "files_matched": files_matched,
                    "diagnostics_count": diagnostics_count,
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("search_structural", result, provenance_result)
    }

    #[tool(
        name = "deep_search_run",
        description = "Run a deep-search playbook and return a trace artifact.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn deep_search_run(
        &self,
        params: Parameters<DeepSearchRunParams>,
    ) -> Result<Json<DeepSearchRunResponse>, ErrorData> {
        let playbook: DeepSearchPlaybook = params.0.into();
        let playbook_id = Self::bounded_text(&playbook.playbook_id);
        let step_count = playbook.steps.len();
        let step_tools = playbook
            .steps
            .iter()
            .map(|step| step.tool_name.clone())
            .collect::<Vec<_>>();
        let harness = DeepSearchHarness::new(self.clone());
        let internal_result = harness.run_playbook(&playbook).await;
        let budget_metadata = internal_result
            .as_ref()
            .ok()
            .map(Self::deep_search_budget_metadata_from_trace)
            .unwrap_or_else(|| json!({ "resource_budgets": [], "resource_usage": [] }));
        let result: Result<Json<DeepSearchRunResponse>, ErrorData> = internal_result
            .map(|trace_artifact| Json(trace_artifact.into()))
            .map_err(Self::map_frigg_error);
        let provenance_result = self
            .record_provenance_blocking(
                "deep_search_run",
                None,
                json!({
                    "playbook_id": playbook_id,
                    "step_count": step_count,
                    "step_tools": step_tools,
                }),
                json!({
                    "resource_budgets": budget_metadata["resource_budgets"].clone(),
                    "resource_usage": budget_metadata["resource_usage"].clone(),
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("deep_search_run", result, provenance_result)
    }

    #[tool(
        name = "deep_search_replay",
        description = "Replay a deep-search playbook against an expected trace.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn deep_search_replay(
        &self,
        params: Parameters<DeepSearchReplayParams>,
    ) -> Result<Json<DeepSearchReplayResponse>, ErrorData> {
        let params = params.0;
        let playbook_id = Self::bounded_text(&params.playbook.playbook_id);
        let step_count = params.playbook.steps.len();
        let step_tools = params
            .playbook
            .steps
            .iter()
            .map(|step| step.tool_name.clone())
            .collect::<Vec<_>>();
        let expected_trace_schema =
            Self::bounded_text(&params.expected_trace_artifact.trace_schema);
        let expected_step_count = params.expected_trace_artifact.step_count;
        let (playbook, expected_trace_artifact) = params.into_internal();
        let harness = DeepSearchHarness::new(self.clone());
        let internal_result = harness
            .replay_and_diff(&playbook, &expected_trace_artifact)
            .await;
        let budget_metadata = internal_result
            .as_ref()
            .ok()
            .map(|replay| Self::deep_search_budget_metadata_from_trace(&replay.replayed))
            .unwrap_or_else(|| json!({ "resource_budgets": [], "resource_usage": [] }));
        let result: Result<Json<DeepSearchReplayResponse>, ErrorData> = internal_result
            .map(|replay| Json(replay.into()))
            .map_err(Self::map_frigg_error);
        let provenance_result = self
            .record_provenance_blocking(
                "deep_search_replay",
                None,
                json!({
                    "playbook_id": playbook_id,
                    "step_count": step_count,
                    "step_tools": step_tools,
                    "expected_trace_schema": expected_trace_schema,
                    "expected_step_count": expected_step_count,
                }),
                json!({
                    "matches": result.as_ref().ok().map(|response| response.0.matches),
                    "diff": result
                        .as_ref()
                        .ok()
                        .and_then(|response| response.0.diff.as_ref().map(|diff| Self::bounded_text(diff))),
                    "resource_budgets": budget_metadata["resource_budgets"].clone(),
                    "resource_usage": budget_metadata["resource_usage"].clone(),
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("deep_search_replay", result, provenance_result)
    }

    #[tool(
        name = "deep_search_compose_citations",
        description = "Compose citation payloads from a deep-search trace artifact.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn deep_search_compose_citations(
        &self,
        params: Parameters<DeepSearchComposeCitationsParams>,
    ) -> Result<Json<DeepSearchComposeCitationsResponse>, ErrorData> {
        let params = params.0;
        let playbook_id = Self::bounded_text(&params.trace_artifact.playbook_id);
        let trace_schema = Self::bounded_text(&params.trace_artifact.trace_schema);
        let step_count = params.trace_artifact.step_count;
        let answer = params.answer;
        let answer_supplied = answer
            .as_ref()
            .map(|candidate| !candidate.trim().is_empty())
            .unwrap_or(false);

        let trace_artifact = params.trace_artifact.into();
        let budget_metadata = Self::deep_search_budget_metadata_from_trace(&trace_artifact);
        let result: Result<Json<DeepSearchComposeCitationsResponse>, ErrorData> =
            DeepSearchHarness::compose_citation_payload(
                &trace_artifact,
                answer.unwrap_or_default(),
            )
            .map(|citation_payload| Json(citation_payload.into()))
            .map_err(Self::map_frigg_error);
        let provenance_result = self
            .record_provenance_blocking(
                "deep_search_compose_citations",
                None,
                json!({
                    "playbook_id": playbook_id,
                    "trace_schema": trace_schema,
                    "step_count": step_count,
                    "answer_supplied": answer_supplied,
                }),
                json!({
                    "claims_count": result
                        .as_ref()
                        .ok()
                        .map(|response| response.0.citation_payload.claims.len()),
                    "citations_count": result
                        .as_ref()
                        .ok()
                        .map(|response| response.0.citation_payload.citations.len()),
                    "resource_budgets": budget_metadata["resource_budgets"].clone(),
                    "resource_usage": budget_metadata["resource_usage"].clone(),
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("deep_search_compose_citations", result, provenance_result)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for FriggMcpServer {
    fn get_info(&self) -> ServerInfo {
        let active_profile = active_runtime_tool_surface_profile().as_str();
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("frigg", env!("CARGO_PKG_VERSION"))
                    .with_title("Frigg Deep Search MCP")
                    .with_description(
                        "Local-first deterministic code search + navigation MCP server",
                    ),
            )
            .with_instructions(
                format!(
                    "Use list_repositories first; if no repository is attached or you want a session-local default repo, call workspace_attach. Runtime tool-surface profile is `{active_profile}` (set `{TOOL_SURFACE_PROFILE_ENV}=extended` to include explore plus deep-search tools). For simple local file reads or literal scans in the checked-out workspace, shell tools may be faster than read_file or search_text. Use search_hybrid for broad doc/runtime questions, then pivot to search_symbol or scoped search_text.path_regex for concrete anchors. Use explore after discovery when you want bounded single-artifact probe/zoom/refine follow-up. Use read_file to confirm exact source when repository-aware evidence is useful, and treat search_hybrid warnings or non-`ok` semantic_status as weaker evidence."
                ),
            )
    }
}

#[cfg(test)]
mod runtime_gate_tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::domain::FriggError;
    use crate::indexer::FileMetadataDigest;
    use crate::settings::{FriggConfig, SemanticRuntimeConfig, SemanticRuntimeProvider};
    use crate::storage::{ManifestEntry, SemanticChunkEmbeddingRecord, Storage};
    use protobuf::{EnumOrUnknown, Message};
    use rmcp::model::ErrorCode;
    use scip::types::{
        Document as ScipDocumentProto, Index as ScipIndexProto, Occurrence as ScipOccurrenceProto,
        SymbolInformation as ScipSymbolInformationProto,
    };

    use super::FriggMcpServer;

    fn fixture_config() -> FriggConfig {
        let workspace_root = std::env::current_dir()
            .expect("current working directory should exist for runtime gate tests");
        FriggConfig::from_workspace_roots(vec![workspace_root])
            .expect("runtime gate tests should build a valid FriggConfig")
    }

    fn to_set(values: Vec<String>) -> BTreeSet<String> {
        values.into_iter().collect()
    }

    fn temp_workspace_root(test_name: &str) -> PathBuf {
        let nanos_since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "frigg-runtime-gate-tests-{test_name}-{}-{nanos_since_epoch}",
            std::process::id()
        ))
    }

    fn semantic_runtime_enabled_openai() -> SemanticRuntimeConfig {
        SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: Some("text-embedding-3-small".to_owned()),
            strict_mode: false,
        }
    }

    fn seed_manifest_snapshot(
        workspace_root: &Path,
        repository_id: &str,
        snapshot_id: &str,
        paths: &[&str],
    ) {
        let db_path = crate::storage::ensure_provenance_db_parent_dir(workspace_root)
            .expect("manifest storage path should work");
        let storage = Storage::new(db_path);
        storage
            .initialize()
            .expect("manifest storage should initialize");

        let mut manifest_entries = paths
            .iter()
            .map(|path| {
                let metadata = fs::metadata(workspace_root.join(path))
                    .expect("manifest snapshot path should exist for test");
                ManifestEntry {
                    path: (*path).to_owned(),
                    sha256: format!("hash-{path}"),
                    size_bytes: metadata.len(),
                    mtime_ns: metadata
                        .modified()
                        .ok()
                        .and_then(FriggMcpServer::system_time_to_unix_nanos),
                }
            })
            .collect::<Vec<_>>();
        manifest_entries.sort_by(|left, right| left.path.cmp(&right.path));
        manifest_entries.dedup_by(|left, right| left.path == right.path);

        storage
            .upsert_manifest(repository_id, snapshot_id, &manifest_entries)
            .expect("manifest snapshot should persist");
    }

    fn semantic_record(
        repository_id: &str,
        snapshot_id: &str,
        path: &str,
    ) -> SemanticChunkEmbeddingRecord {
        SemanticChunkEmbeddingRecord {
            chunk_id: format!("chunk-{}", path.replace('/', "_")),
            repository_id: repository_id.to_owned(),
            snapshot_id: snapshot_id.to_owned(),
            path: path.to_owned(),
            language: "rust".to_owned(),
            chunk_index: 0,
            start_line: 1,
            end_line: 1,
            provider: "openai".to_owned(),
            model: "text-embedding-3-small".to_owned(),
            trace_id: Some("trace-001".to_owned()),
            content_hash_blake3: format!("hash-{}", path.replace('/', "_")),
            content_text: path.to_owned(),
            embedding: vec![0.25, 0.75],
        }
    }

    fn write_scip_protobuf_fixture(workspace_root: &Path, file_name: &str) {
        let fixture_dir = workspace_root.join(".frigg/scip");
        fs::create_dir_all(&fixture_dir).expect("failed to create scip fixture directory");

        let mut index = ScipIndexProto::new();
        let mut document = ScipDocumentProto::new();
        document.relative_path = "src/lib.rs".to_owned();

        let mut definition = ScipOccurrenceProto::new();
        definition.symbol = "scip-rust pkg repo#User".to_owned();
        definition.range = vec![0, 11, 15];
        definition.symbol_roles = 1;
        document.occurrences.push(definition);

        let mut reference = ScipOccurrenceProto::new();
        reference.symbol = "scip-rust pkg repo#User".to_owned();
        reference.range = vec![2, 31, 35];
        reference.symbol_roles = 8;
        document.occurrences.push(reference);

        let mut symbol = ScipSymbolInformationProto::new();
        symbol.symbol = "scip-rust pkg repo#User".to_owned();
        symbol.display_name = "User".to_owned();
        symbol.kind = EnumOrUnknown::from_i32(7);
        document.symbols.push(symbol);

        index.documents.push(document);
        let payload = index
            .write_to_bytes()
            .expect("protobuf fixture payload should serialize");
        fs::write(fixture_dir.join(file_name), payload)
            .expect("failed to write scip protobuf fixture payload");
    }

    #[test]
    fn extended_only_tools_are_hidden_by_default_runtime_options() {
        let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false, false);
        let names = to_set(server.runtime_registered_tool_names());

        for tool_name in FriggMcpServer::EXTENDED_ONLY_TOOL_NAMES {
            assert!(
                !names.contains(tool_name),
                "extended-only tool should not be registered by default: {tool_name}"
            );
        }
        assert!(
            names.contains("list_repositories"),
            "core tools should remain registered when extended-only tools are disabled"
        );
    }

    #[test]
    fn extended_only_tools_are_registered_when_runtime_option_enabled() {
        let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false, true);
        let names = to_set(server.runtime_registered_tool_names());

        for tool_name in FriggMcpServer::EXTENDED_ONLY_TOOL_NAMES {
            assert!(
                names.contains(tool_name),
                "extended-only tool should be registered when enabled: {tool_name}"
            );
        }
    }

    #[test]
    fn strict_semantic_failure_maps_to_unavailable_error_code() {
        let error = FriggMcpServer::map_frigg_error(FriggError::Internal(
            "semantic_status=strict_failure: provider outage".to_owned(),
        ));

        assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|value| value.get("error_code")),
            Some(&serde_json::Value::String("unavailable".to_owned()))
        );
        assert_eq!(
            error.data.as_ref().and_then(|value| value.get("retryable")),
            Some(&serde_json::Value::Bool(true))
        );
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|value| value.get("semantic_status"))
                .and_then(|value| value.as_str()),
            Some("strict_failure")
        );
    }

    #[test]
    fn search_hybrid_warning_surfaces_semantic_ok_empty_channel() {
        let warning = FriggMcpServer::search_hybrid_warning(Some("ok"), None, Some(0), Some(0));

        assert_eq!(
            warning.as_deref(),
            Some(
                "semantic retrieval completed successfully but retained no query-relevant semantic hits; results are ranked from lexical and graph signals only"
            )
        );
    }

    #[test]
    fn search_hybrid_warning_surfaces_semantic_ok_noncontributing_hits() {
        let warning = FriggMcpServer::search_hybrid_warning(Some("ok"), None, Some(3), Some(0));

        assert_eq!(
            warning.as_deref(),
            Some(
                "semantic retrieval retained semantic hits, but none contributed to the returned top results; ranking is effectively lexical and graph for this result set"
            )
        );
    }

    #[test]
    fn precise_artifact_discovery_is_scoped_to_runtime_scip_directory() {
        let workspace_root = PathBuf::from("/tmp/frigg-runtime-scip-scope");
        let directories = FriggMcpServer::scip_candidate_directories(&workspace_root);

        assert_eq!(directories, [workspace_root.join(".frigg/scip")]);
    }

    #[test]
    fn precise_artifact_discovery_includes_json_and_scip_files() {
        let workspace_root = temp_workspace_root("scip-discovery-extensions");
        let scip_root = workspace_root.join(".frigg/scip");
        fs::create_dir_all(&scip_root).expect("failed to create scip fixture directory");
        fs::write(scip_root.join("a.json"), "{}").expect("failed to write json fixture");
        fs::write(scip_root.join("b.scip"), [0_u8, 1_u8, 2_u8])
            .expect("failed to write protobuf fixture");
        fs::write(scip_root.join("ignored.txt"), "x").expect("failed to write ignored fixture");

        let discovery = FriggMcpServer::collect_scip_artifact_digests(&workspace_root);
        assert_eq!(discovery.artifact_digests.len(), 2);
        assert_eq!(
            discovery
                .artifact_digests
                .iter()
                .map(|digest| digest.path.file_name().and_then(|name| name.to_str()))
                .collect::<Vec<_>>(),
            vec![Some("a.json"), Some("b.scip")]
        );
        assert_eq!(
            discovery
                .artifact_digests
                .iter()
                .map(|digest| digest.format.as_str())
                .collect::<Vec<_>>(),
            vec!["json", "protobuf"]
        );

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn manifest_source_paths_filter_to_symbol_corpus_capability() {
        let digests = vec![
            FileMetadataDigest {
                path: PathBuf::from("src/lib.rs"),
                size_bytes: 10,
                mtime_ns: Some(1),
            },
            FileMetadataDigest {
                path: PathBuf::from("src/server.php"),
                size_bytes: 20,
                mtime_ns: Some(2),
            },
            FileMetadataDigest {
                path: PathBuf::from("src/app.ts"),
                size_bytes: 30,
                mtime_ns: Some(3),
            },
            FileMetadataDigest {
                path: PathBuf::from("README.md"),
                size_bytes: 40,
                mtime_ns: Some(4),
            },
        ];

        let source_paths = FriggMcpServer::manifest_source_paths_for_digests(&digests);

        assert_eq!(
            source_paths,
            vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/server.php")]
        );
    }

    #[test]
    fn semantic_refresh_plan_detects_latest_snapshot_missing_active_model() {
        let workspace_root = temp_workspace_root("semantic-refresh-plan");
        fs::create_dir_all(workspace_root.join("src"))
            .expect("failed to create workspace src directory");
        fs::write(workspace_root.join("src/lib.rs"), "pub struct User;\n")
            .expect("failed to write source fixture");

        let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config");
        config.semantic_runtime = semantic_runtime_enabled_openai();
        let server = FriggMcpServer::new_with_runtime_options(config, false, false);
        let workspace = server
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .attached_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");

        seed_manifest_snapshot(
            &workspace_root,
            &workspace.repository_id,
            "snapshot-001",
            &["src/lib.rs"],
        );
        let storage = Storage::new(&workspace.db_path);
        storage
            .replace_semantic_embeddings_for_repository(
                &workspace.repository_id,
                "snapshot-001",
                &[semantic_record(
                    &workspace.repository_id,
                    "snapshot-001",
                    "src/lib.rs",
                )],
            )
            .expect("seed semantic embeddings should persist");
        seed_manifest_snapshot(
            &workspace_root,
            &workspace.repository_id,
            "snapshot-002",
            &["src/lib.rs"],
        );

        let plan = server
            .workspace_semantic_refresh_plan(&workspace)
            .expect("latest snapshot without active-model semantic rows should trigger refresh");
        assert_eq!(plan.latest_snapshot_id, "snapshot-002");
        assert_eq!(plan.compatible_snapshot_id, "snapshot-001");
        assert_eq!(plan.reason, "semantic_snapshot_missing_for_active_model");

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[test]
    fn precise_graph_prewarm_populates_latest_precise_cache() {
        let workspace_root = temp_workspace_root("precise-prewarm");
        fs::create_dir_all(workspace_root.join("src"))
            .expect("failed to create workspace src directory");
        fs::write(
            workspace_root.join("src/lib.rs"),
            "pub struct User;\n\npub fn current_user() -> User { User }\n",
        )
        .expect("failed to write source fixture");
        write_scip_protobuf_fixture(&workspace_root, "fixture.scip");

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config");
        let server = FriggMcpServer::new_with_runtime_options(config, false, false);
        let workspace = server
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .attached_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");

        server.prewarm_precise_graph_for_workspace(&workspace);

        let cached = server
            .latest_precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&workspace.repository_id)
            .cloned()
            .expect("precise prewarm should populate the latest precise graph cache");
        assert_eq!(cached.ingest_stats.artifacts_ingested, 1);
        assert_eq!(cached.ingest_stats.artifacts_failed, 0);
        assert_eq!(
            cached.coverage_mode,
            crate::mcp::server_state::PreciseCoverageMode::Full
        );

        let _ = fs::remove_dir_all(workspace_root);
    }
}
