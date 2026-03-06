use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::domain::FriggError;
use crate::domain::model::{ReferenceMatch, SymbolMatch};
use crate::graph::{
    PreciseRelationshipKind, RelationKind, ScipIngestError, ScipResourceBudgets, SymbolGraph,
};
use crate::indexer::{
    FileMetadataDigest, HeuristicReferenceConfidence, HeuristicReferenceEvidence,
    HeuristicReferenceResolver, ManifestBuilder, ManifestDiagnosticKind, SymbolDefinition,
    SymbolExtractionOutput, SymbolLanguage, extract_symbols_for_paths, extract_symbols_from_source,
    navigation_symbol_target_rank, register_symbol_definitions, search_structural_in_source,
};
use crate::manifest_validation::validate_manifest_digests_for_root;
use crate::searcher::{
    HybridChannelWeights, SearchDiagnosticKind, SearchFilters, SearchHybridQuery, SearchTextQuery,
    TextSearcher, compile_safe_regex,
};
use crate::settings::FriggConfig;
use crate::storage::{Storage, ensure_provenance_db_parent_dir, resolve_provenance_db_path};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::transport::{
    StreamableHttpServerConfig, StreamableHttpService,
    streamable_http_server::session::local::LocalSessionManager,
};
use rmcp::{ErrorData, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde_json::{Value, json};
use tokio::task;
use tracing::warn;

use crate::mcp::deep_search::{
    DeepSearchHarness, DeepSearchPlaybook, DeepSearchTraceArtifact, DeepSearchTraceOutcome,
};
use crate::mcp::tool_surface::{
    TOOL_SURFACE_PROFILE_ENV, ToolSurfaceParityDiff, ToolSurfaceProfile,
    active_runtime_tool_surface_profile, diff_runtime_against_profile_manifest,
};
use crate::mcp::types::{
    CallHierarchyMatch, DeepSearchComposeCitationsParams, DeepSearchComposeCitationsResponse,
    DeepSearchReplayParams, DeepSearchReplayResponse, DeepSearchRunParams, DeepSearchRunResponse,
    DocumentSymbolsParams, DocumentSymbolsResponse, FindDeclarationsParams,
    FindDeclarationsResponse, FindImplementationsParams, FindImplementationsResponse,
    FindReferencesParams, FindReferencesResponse, GoToDefinitionParams, GoToDefinitionResponse,
    ImplementationMatch, IncomingCallsParams, IncomingCallsResponse, ListRepositoriesParams,
    ListRepositoriesResponse, NavigationLocation, OutgoingCallsParams, OutgoingCallsResponse,
    ReadFileParams, ReadFileResponse, RepositorySummary, SearchHybridChannelWeightsParams,
    SearchHybridMatch, SearchHybridParams, SearchHybridResponse, SearchPatternType,
    SearchStructuralParams, SearchStructuralResponse, SearchSymbolParams, SearchSymbolResponse,
    SearchTextParams, SearchTextResponse,
};

pub type FriggMcpService = StreamableHttpService<FriggMcpServer, LocalSessionManager>;

#[derive(Clone)]
pub struct FriggMcpServer {
    config: Arc<FriggConfig>,
    tool_router: ToolRouter<Self>,
    symbol_corpus_cache: Arc<RwLock<BTreeMap<SymbolCorpusCacheKey, Arc<RepositorySymbolCorpus>>>>,
    precise_graph_cache: Arc<RwLock<BTreeMap<PreciseGraphCacheKey, Arc<CachedPreciseGraph>>>>,
    latest_precise_graph_cache: Arc<RwLock<BTreeMap<String, Arc<CachedPreciseGraph>>>>,
    provenance_storage_cache: Arc<RwLock<BTreeMap<ProvenanceStorageCacheKey, Arc<Storage>>>>,
    provenance_best_effort: bool,
}

#[derive(Clone)]
struct RepositorySymbolCorpus {
    repository_id: String,
    root: PathBuf,
    root_signature: String,
    source_paths: Vec<PathBuf>,
    symbols: Vec<SymbolDefinition>,
    symbols_by_relative_path: BTreeMap<String, Vec<usize>>,
    symbol_index_by_stable_id: BTreeMap<String, usize>,
    symbol_indices_by_name: BTreeMap<String, Vec<usize>>,
    symbol_indices_by_lower_name: BTreeMap<String, Vec<usize>>,
    diagnostics: RepositoryDiagnosticsSummary,
}

#[derive(Debug, Clone, Default)]
struct RepositoryDiagnosticsSummary {
    manifest_walk_count: usize,
    manifest_read_count: usize,
    symbol_extraction_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SymbolCorpusCacheKey {
    repository_id: String,
    manifest_token: String,
}

#[derive(Clone)]
struct SymbolCandidate {
    rank: u8,
    repository_id: String,
    root: PathBuf,
    symbol: SymbolDefinition,
}

#[derive(Clone)]
struct ResolvedSymbolTarget {
    candidate: SymbolCandidate,
    corpus: Arc<RepositorySymbolCorpus>,
    candidate_count: usize,
    selected_rank_candidate_count: usize,
}

#[derive(Clone)]
struct ResolvedNavigationTarget {
    symbol_query: String,
    target: ResolvedSymbolTarget,
    resolution_source: &'static str,
}

struct ReadFileExecution {
    result: Result<Json<ReadFileResponse>, ErrorData>,
    resolved_repository_id: Option<String>,
    resolved_path: Option<String>,
    resolved_absolute_path: Option<String>,
    effective_max_bytes: Option<usize>,
    effective_line_start: Option<usize>,
    effective_line_end: Option<usize>,
}

struct SearchTextExecution {
    result: Result<Json<SearchTextResponse>, ErrorData>,
    scoped_repository_ids: Vec<String>,
    effective_limit: Option<usize>,
    effective_pattern_type: Option<SearchPatternType>,
    diagnostics_count: usize,
    walk_diagnostics_count: usize,
    read_diagnostics_count: usize,
}

struct SearchHybridExecution {
    result: Result<Json<SearchHybridResponse>, ErrorData>,
    scoped_repository_ids: Vec<String>,
    effective_limit: Option<usize>,
    effective_weights: Option<SearchHybridChannelWeightsParams>,
    diagnostics_count: usize,
    walk_diagnostics_count: usize,
    read_diagnostics_count: usize,
    semantic_requested: Option<bool>,
    semantic_enabled: Option<bool>,
    semantic_status: Option<String>,
    semantic_reason: Option<String>,
}

struct SearchSymbolExecution {
    result: Result<Json<SearchSymbolResponse>, ErrorData>,
    scoped_repository_ids: Vec<String>,
    diagnostics_count: usize,
    manifest_walk_diagnostics_count: usize,
    manifest_read_diagnostics_count: usize,
    symbol_extraction_diagnostics_count: usize,
    effective_limit: Option<usize>,
}

struct FindReferencesExecution {
    result: Result<Json<FindReferencesResponse>, ErrorData>,
    scoped_repository_ids: Vec<String>,
    selected_symbol_id: Option<String>,
    selected_precise_symbol: Option<String>,
    resolution_precision: Option<String>,
    diagnostics_count: usize,
    manifest_walk_diagnostics_count: usize,
    manifest_read_diagnostics_count: usize,
    symbol_extraction_diagnostics_count: usize,
    source_read_diagnostics_count: usize,
    precise_artifacts_discovered: usize,
    precise_artifacts_discovered_bytes: u64,
    precise_artifacts_ingested: usize,
    precise_artifacts_ingested_bytes: u64,
    precise_artifacts_failed: usize,
    precise_artifacts_failed_bytes: u64,
    precise_reference_count: usize,
    source_files_discovered: usize,
    source_files_loaded: usize,
    source_bytes_loaded: u64,
    effective_limit: Option<usize>,
}

struct NavigationToolExecution<T> {
    result: Result<Json<T>, ErrorData>,
    scoped_repository_ids: Vec<String>,
    selected_symbol_id: Option<String>,
    selected_precise_symbol: Option<String>,
    resolution_precision: Option<String>,
    resolution_source: Option<String>,
    effective_limit: Option<usize>,
    precise_artifacts_ingested: usize,
    precise_artifacts_failed: usize,
    match_count: usize,
}

#[derive(Debug, Clone)]
struct PreciseArtifactFailureSample {
    artifact_label: String,
    stage: String,
    detail: String,
}

#[derive(Debug, Clone, Default)]
struct PreciseIngestStats {
    candidate_directories: Vec<String>,
    discovered_artifacts: Vec<String>,
    artifacts_discovered: usize,
    artifacts_discovered_bytes: u64,
    artifacts_ingested: usize,
    artifacts_ingested_bytes: u64,
    artifacts_failed: usize,
    artifacts_failed_bytes: u64,
    failed_artifacts: Vec<PreciseArtifactFailureSample>,
}

#[derive(Debug, Clone, Copy)]
struct FindReferencesResourceBudgets {
    scip_max_artifacts: usize,
    scip_max_artifact_bytes: usize,
    scip_max_total_bytes: usize,
    scip_max_documents_per_artifact: usize,
    scip_max_elapsed_ms: u64,
    source_max_files: usize,
    source_max_file_bytes: usize,
    source_max_total_bytes: usize,
    source_max_elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PreciseGraphCacheKey {
    repository_id: String,
    scip_signature: String,
    corpus_signature: String,
}

#[derive(Debug, Clone)]
struct CachedPreciseGraph {
    graph: Arc<SymbolGraph>,
    ingest_stats: PreciseIngestStats,
    corpus_signature: String,
    discovery: ScipArtifactDiscovery,
    coverage_mode: PreciseCoverageMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreciseCoverageMode {
    Full,
    Partial,
    None,
}

impl PreciseCoverageMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProvenanceStorageCacheKey {
    repository_id: String,
    db_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ScipArtifactDigest {
    path: PathBuf,
    format: ScipArtifactFormat,
    size_bytes: u64,
    mtime_ns: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct ScipArtifactDiscovery {
    candidate_directories: Vec<String>,
    candidate_directory_digests: Vec<ScipCandidateDirectoryDigest>,
    artifact_digests: Vec<ScipArtifactDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ScipCandidateDirectoryDigest {
    path: PathBuf,
    exists: bool,
    mtime_ns: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ScipArtifactFormat {
    Json,
    Protobuf,
}

impl ScipArtifactFormat {
    fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Some(Self::Json),
            Some("scip") => Some(Self::Protobuf),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Protobuf => "protobuf",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DeterministicSignatureHasher {
    state: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProvenancePersistenceStage {
    ResolveStoragePath,
    InitializeStorage,
    AppendEvent,
}

impl ProvenancePersistenceStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::ResolveStoragePath => "resolve_storage_path",
            Self::InitializeStorage => "initialize_storage",
            Self::AppendEvent => "append_event",
        }
    }

    fn retryable(self) -> bool {
        matches!(self, Self::InitializeStorage | Self::AppendEvent)
    }
}

impl DeterministicSignatureHasher {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self {
            state: Self::OFFSET_BASIS,
        }
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(Self::FNV_PRIME);
        }
    }

    fn write_separator(&mut self) {
        self.write_bytes(&[0xff]);
    }

    fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
        self.write_separator();
    }

    fn write_u64(&mut self, value: u64) {
        self.write_bytes(&value.to_le_bytes());
        self.write_separator();
    }

    fn write_optional_u64(&mut self, value: Option<u64>) {
        match value {
            Some(value) => {
                self.write_bytes(&[1]);
                self.write_u64(value);
            }
            None => {
                self.write_bytes(&[0]);
                self.write_separator();
            }
        }
    }

    fn finish_hex(self) -> String {
        format!("{:016x}", self.state)
    }
}

impl FriggMcpServer {
    const DEEP_SEARCH_TOOL_NAMES: [&str; 3] = [
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

    fn filtered_tool_router(enable_deep_search_tools: bool) -> ToolRouter<Self> {
        let mut router = Self::tool_router();
        if !enable_deep_search_tools {
            for tool_name in Self::DEEP_SEARCH_TOOL_NAMES {
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
        candidates.push(SymbolCandidate {
            rank,
            repository_id: corpus.repository_id.clone(),
            root: corpus.root.clone(),
            symbol,
        });
    }

    fn push_ranked_symbol_match(
        ranked_matches: &mut Vec<(u8, SymbolMatch)>,
        corpus: &RepositorySymbolCorpus,
        symbol_index: usize,
        rank: u8,
    ) {
        let symbol = &corpus.symbols[symbol_index];
        ranked_matches.push((
            rank,
            SymbolMatch {
                repository_id: corpus.repository_id.clone(),
                symbol: symbol.name.clone(),
                kind: symbol.kind.as_str().to_owned(),
                path: Self::relative_display_path(&corpus.root, &symbol.path),
                line: symbol.line,
            },
        ));
    }

    fn sort_ranked_symbol_matches(ranked_matches: &mut [(u8, SymbolMatch)]) {
        ranked_matches.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.repository_id.cmp(&right.1.repository_id))
                .then(left.1.path.cmp(&right.1.path))
                .then(left.1.line.cmp(&right.1.line))
                .then(left.1.kind.cmp(&right.1.kind))
                .then(left.1.symbol.cmp(&right.1.symbol))
        });
    }

    fn retain_bounded_ranked_symbol_match(
        ranked_matches: &mut Vec<(u8, SymbolMatch)>,
        limit: usize,
        candidate: (u8, SymbolMatch),
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
        for corpus in corpora {
            if let Some(symbol_index) = corpus.symbol_index_by_stable_id.get(symbol_query) {
                Self::push_symbol_candidate(&mut candidates, corpus, *symbol_index, 0);
            }
            if let Some(symbol_indices) = corpus.symbol_indices_by_name.get(symbol_query) {
                for symbol_index in symbol_indices {
                    let symbol = &corpus.symbols[*symbol_index];
                    if navigation_symbol_target_rank(symbol, symbol_query) == Some(1) {
                        Self::push_symbol_candidate(&mut candidates, corpus, *symbol_index, 1);
                    }
                }
            }
            if let Some(symbol_indices) = corpus.symbol_indices_by_lower_name.get(&query_lower) {
                for symbol_index in symbol_indices {
                    let symbol = &corpus.symbols[*symbol_index];
                    if navigation_symbol_target_rank(symbol, symbol_query) == Some(2) {
                        Self::push_symbol_candidate(&mut candidates, corpus, *symbol_index, 2);
                    }
                }
            }
        }

        candidates.sort_by(|left, right| {
            left.rank
                .cmp(&right.rank)
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

    fn precise_relationships_to_symbol_by_kind(
        graph: &SymbolGraph,
        repository_id: &str,
        to_symbol: &str,
        kinds: &[PreciseRelationshipKind],
    ) -> Vec<crate::graph::PreciseRelationshipRecord> {
        graph.precise_relationships_to_symbol_by_kinds(repository_id, to_symbol, kinds)
    }

    fn read_line_slice_lossy(
        path: &Path,
        line_start: usize,
        line_end: Option<usize>,
        max_bytes: usize,
    ) -> Result<(String, usize, usize), ErrorData> {
        let file = fs::File::open(path).map_err(|err| {
            Self::internal(
                format!("failed to read file {}: {err}", path.display()),
                None,
            )
        })?;
        let mut reader = BufReader::new(file);
        let mut raw_line = Vec::new();
        let mut content = String::new();
        let mut total_lines = 0usize;
        let mut sliced_bytes = 0usize;
        let mut exceeded_limit = false;
        let mut first_selected_line = true;

        loop {
            raw_line.clear();
            let bytes_read = reader.read_until(b'\n', &mut raw_line).map_err(|err| {
                Self::internal(
                    format!("failed to read file {}: {err}", path.display()),
                    None,
                )
            })?;
            if bytes_read == 0 {
                break;
            }

            total_lines = total_lines.saturating_add(1);
            let include_line = total_lines >= line_start
                && line_end.is_none_or(|effective_end| total_lines <= effective_end);
            if !include_line {
                if line_end.is_some_and(|effective_end| total_lines >= effective_end) {
                    break;
                }
                continue;
            }

            let normalized_line = Self::normalize_lossy_line_bytes(&raw_line);
            if !first_selected_line {
                sliced_bytes = sliced_bytes.saturating_add(1);
                if !exceeded_limit {
                    content.push('\n');
                }
            }
            sliced_bytes = sliced_bytes.saturating_add(normalized_line.len());
            if sliced_bytes > max_bytes {
                exceeded_limit = true;
            }
            if !exceeded_limit {
                content.push_str(&normalized_line);
            }
            first_selected_line = false;

            if line_end.is_some_and(|effective_end| total_lines >= effective_end) {
                break;
            }
        }

        if total_lines > 0 && line_start > total_lines {
            return Err(Self::invalid_params(
                "line_start is outside file bounds",
                Some(serde_json::json!({
                    "line_start": line_start,
                    "line_end": line_end,
                    "total_lines": total_lines,
                })),
            ));
        }

        Ok((content, sliced_bytes, total_lines))
    }

    fn normalize_lossy_line_bytes(raw_line: &[u8]) -> String {
        let mut line_bytes = raw_line;
        if line_bytes.ends_with(b"\n") {
            line_bytes = &line_bytes[..line_bytes.len() - 1];
        }
        if line_bytes.ends_with(b"\r") {
            line_bytes = &line_bytes[..line_bytes.len() - 1];
        }
        String::from_utf8_lossy(line_bytes).into_owned()
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
        exclude_symbol_id: &str,
    ) -> Option<&'a SymbolDefinition> {
        let occurrence_path = Self::canonicalize_navigation_path(root, &occurrence.path);
        target_corpus
            .symbols_by_relative_path
            .get(&occurrence_path)
            .into_iter()
            .flat_map(|indices| indices.iter())
            .map(|index| &target_corpus.symbols[*index])
            .filter(|symbol| symbol.stable_id != exclude_symbol_id)
            .filter(|symbol| {
                occurrence.range.start_line >= symbol.span.start_line
                    && occurrence.range.start_line <= symbol.span.end_line
            })
            .min_by(|left, right| {
                let left_span = left.span.end_line.saturating_sub(left.span.start_line);
                let right_span = right.span.end_line.saturating_sub(right.span.start_line);
                left_span
                    .cmp(&right_span)
                    .then(left.span.start_line.cmp(&right.span.start_line))
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
        let mut matches = graph
            .precise_references_for_symbol(&target_corpus.repository_id, &precise_target.symbol)
            .into_iter()
            .filter_map(|occurrence| {
                let enclosing_symbol = Self::precise_enclosing_symbol_for_occurrence(
                    target_corpus,
                    root,
                    &occurrence,
                    exclude_symbol_id,
                )?;
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
                    relation: "refers_to".to_owned(),
                    precision: Some(precision.clone()),
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
        });
        matches
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
                repository_id: target_corpus.repository_id.clone(),
                path: Self::relative_display_path(target_root, &symbol.path),
                line: symbol.line,
                column: 1,
                relation,
                precision: Some("heuristic".to_owned()),
                fallback_reason: Some("precise_absent".to_owned()),
            });
        }

        Self::sort_implementation_matches(&mut matches);
        matches.dedup_by(|left, right| {
            left.repository_id == right.repository_id
                && left.path == right.path
                && left.line == right.line
                && left.column == right.column
                && left.symbol == right.symbol
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

        let language = match normalized.as_str() {
            "rust" => SymbolLanguage::Rust,
            "php" => SymbolLanguage::Php,
            _ => {
                return Err(Self::invalid_params(
                    format!("unsupported language `{value}` for structural search"),
                    Some(json!({
                        "language": value,
                        "supported_languages": ["rust", "php"],
                    })),
                ));
            }
        };
        Ok(Some(language))
    }

    fn is_heuristic_call_relation(relation: RelationKind) -> bool {
        matches!(relation, RelationKind::Calls | RelationKind::RefersTo)
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
                .filter(|path| SymbolLanguage::from_path(path).is_some())
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

    fn manifest_source_paths_for_digests(file_digests: &[FileMetadataDigest]) -> Vec<PathBuf> {
        let mut source_paths = Vec::new();
        for digest in file_digests {
            if SymbolLanguage::from_path(&digest.path).is_some() {
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
        self.config
            .repositories()
            .into_iter()
            .map(|repo| (repo.repository_id.0, PathBuf::from(repo.root_path)))
            .min_by(|left, right| left.0.cmp(&right.0))
    }

    fn provenance_target_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Option<(String, PathBuf)> {
        match repository_id {
            Some(repository_id) => self
                .config
                .root_by_repository_id(repository_id)
                .map(|root| (repository_id.to_owned(), root.to_path_buf())),
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
        enable_deep_search_tools: bool,
    ) -> Self {
        Self {
            config: Arc::new(config),
            tool_router: Self::filtered_tool_router(enable_deep_search_tools),
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
        let enable_deep_search_tools =
            active_runtime_tool_surface_profile() == ToolSurfaceProfile::Extended;
        Self::new_with_runtime_options(config, provenance_best_effort, enable_deep_search_tools)
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

    pub async fn serve_stdio(self) -> Result<(), rmcp::RmcpError> {
        let service = self.serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
        Ok(())
    }

    pub fn streamable_http_service(self, config: StreamableHttpServerConfig) -> FriggMcpService {
        StreamableHttpService::new(
            move || Ok(self.clone()),
            Arc::new(LocalSessionManager::default()),
            config,
        )
    }

    fn roots_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Result<Vec<(String, PathBuf)>, ErrorData> {
        if let Some(repository_id) = repository_id {
            if let Some(root) = self.config.root_by_repository_id(repository_id) {
                return Ok(vec![(repository_id.to_owned(), root.to_path_buf())]);
            }
            return Err(Self::resource_not_found(
                "repository_id not found",
                Some(serde_json::json!({ "repository_id": repository_id })),
            ));
        }

        Ok(self
            .config
            .repositories()
            .into_iter()
            .map(|repo| (repo.repository_id.0, PathBuf::from(repo.root_path)))
            .collect())
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
        description = "List configured local repositories/workspace roots.",
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
            .config
            .repositories()
            .into_iter()
            .map(|repo| RepositorySummary {
                repository_id: repo.repository_id.0,
                display_name: repo.display_name,
                root_path: repo.root_path,
            })
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
        name = "read_file",
        description = "Read a file from the configured workspace roots.",
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

                let (sliced_content, sliced_bytes, total_lines) =
                    Self::read_line_slice_lossy(&path, line_start, requested_line_end, max_bytes)?;
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
        name = "search_text",
        description = "Search text across configured repositories (literal plus optional path regex filter).",
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

                let scoped_roots_with_id =
                    server.roots_for_repository(params_for_blocking.repository_id.as_deref())?;
                scoped_repository_ids = scoped_roots_with_id
                    .iter()
                    .map(|(repository_id, _)| repository_id.clone())
                    .collect::<Vec<_>>();
                let scoped_roots = scoped_roots_with_id
                    .into_iter()
                    .map(|(_, root)| root)
                    .collect::<Vec<_>>();
                let scoped_config = FriggConfig {
                    workspace_roots: scoped_roots,
                    ..(*server.config).clone()
                };

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

                if let Some(repository_id) = params_for_blocking.repository_id.clone() {
                    for found in &mut matches {
                        found.repository_id = repository_id.clone();
                    }
                }

                Ok(Json(SearchTextResponse { matches }))
            })();

            SearchTextExecution {
                result,
                scoped_repository_ids,
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
        description = "Search with deterministic hybrid ranking across lexical, graph, and semantic channels.",
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

                let scoped_roots_with_id =
                    server.roots_for_repository(params_for_blocking.repository_id.as_deref())?;
                scoped_repository_ids = scoped_roots_with_id
                    .iter()
                    .map(|(repository_id, _)| repository_id.clone())
                    .collect::<Vec<_>>();
                let scoped_roots = scoped_roots_with_id
                    .into_iter()
                    .map(|(_, root)| root)
                    .collect::<Vec<_>>();
                let scoped_config = FriggConfig {
                    workspace_roots: scoped_roots,
                    ..(*server.config).clone()
                };

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

                if let Some(repository_id) = params_for_blocking.repository_id.clone() {
                    for found in &mut matches {
                        found.repository_id = repository_id.clone();
                    }
                }

                let note = Some(
                    json!({
                        "semantic_requested": semantic_requested,
                        "semantic_enabled": semantic_enabled,
                        "semantic_status": semantic_status,
                        "semantic_reason": semantic_reason,
                        "diagnostics_count": diagnostics_count,
                        "diagnostics": {
                            "walk": walk_diagnostics_count,
                            "read": read_diagnostics_count,
                            "total": diagnostics_count,
                        },
                    })
                    .to_string(),
                );

                Ok(Json(SearchHybridResponse { matches, note }))
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
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("search_hybrid", result, provenance_result)
    }

    #[tool(
        name = "search_symbol",
        description = "Search symbols extracted from Rust/PHP sources.",
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

                let query_lower = query.to_ascii_lowercase();
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

                let mut ranked_matches: Vec<(u8, SymbolMatch)> = Vec::new();
                for corpus in &corpora {
                    if let Some(symbol_indices) = corpus.symbol_indices_by_name.get(&query) {
                        for symbol_index in symbol_indices {
                            Self::push_ranked_symbol_match(
                                &mut ranked_matches,
                                corpus,
                                *symbol_index,
                                0,
                            );
                        }
                    }
                    if let Some(symbol_indices) =
                        corpus.symbol_indices_by_lower_name.get(&query_lower)
                    {
                        for symbol_index in symbol_indices {
                            if corpus.symbols[*symbol_index].name != query {
                                Self::push_ranked_symbol_match(
                                    &mut ranked_matches,
                                    corpus,
                                    *symbol_index,
                                    1,
                                );
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
                            Self::push_ranked_symbol_match(
                                &mut ranked_matches,
                                corpus,
                                *symbol_index,
                                2,
                            );
                        }
                    }
                }
                if ranked_matches.len() < limit {
                    let infix_limit = limit.saturating_sub(ranked_matches.len());
                    let mut infix_matches = Vec::new();
                    for corpus in &corpora {
                        for symbol in &corpus.symbols {
                            if Self::symbol_name_match_rank(&symbol.name, &query, &query_lower)
                                != Some(3)
                            {
                                continue;
                            }
                            Self::retain_bounded_ranked_symbol_match(
                                &mut infix_matches,
                                infix_limit,
                                (
                                    3,
                                    SymbolMatch {
                                        repository_id: corpus.repository_id.clone(),
                                        symbol: symbol.name.clone(),
                                        kind: symbol.kind.as_str().to_owned(),
                                        path: Self::relative_display_path(
                                            &corpus.root,
                                            &symbol.path,
                                        ),
                                        line: symbol.line,
                                    },
                                ),
                            );
                        }
                    }
                    ranked_matches.extend(infix_matches);
                }

                Self::sort_ranked_symbol_matches(&mut ranked_matches);
                let matches = ranked_matches
                    .into_iter()
                    .take(limit)
                    .map(|(_, matched)| matched)
                    .collect::<Vec<_>>();

                let note = Some(
                    json!({
                        "source": "tree_sitter",
                        "diagnostics_count": diagnostics_count,
                        "diagnostics": {
                            "manifest_walk": manifest_walk_diagnostics_count,
                            "manifest_read": manifest_read_diagnostics_count,
                            "symbol_extraction": symbol_extraction_diagnostics_count,
                            "total": diagnostics_count,
                        },
                        "heuristic": false,
                    })
                    .to_string(),
                );
                Ok(Json(SearchSymbolResponse { matches, note }))
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
        description = "Find symbol references preferring precise SCIP data with deterministic heuristic fallback.",
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
            let mut selected_symbol_id: Option<String> = None;
            let mut selected_precise_symbol: Option<String> = None;
            let mut resolution_precision: Option<String> = None;
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
                let symbol_query = params_for_blocking.symbol.trim().to_owned();
                if symbol_query.is_empty() {
                    return Err(Self::invalid_params("symbol must not be empty", None));
                }

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

                let precise_target = graph.select_precise_symbol_for_navigation(
                    &target_corpus.repository_id,
                    &symbol_query,
                    &target.symbol.name,
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

                    let precision = Self::precise_resolution_precision(precise_coverage);
                    resolution_precision = Some(precision.to_owned());
                    let note = Some(
                        json!({
                            "precision": precision,
                            "heuristic": false,
                            "target_symbol_id": target.symbol.stable_id,
                            "target_precise_symbol": precise_target
                                .as_ref()
                                .map(|selected| selected.symbol.clone()),
                            "resolution_source": "symbol",
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
                        })
                        .to_string(),
                    );

                    return Ok(Json(FindReferencesResponse { matches, note }));
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

                let references = resolver.finish().into_iter().take(limit).collect::<Vec<_>>();

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
                let note = Some(
                    json!({
                        "precision": "heuristic",
                        "heuristic": true,
                        "fallback_reason": "precise_absent",
                        "precise_absence_reason": Self::precise_absence_reason(
                            precise_coverage,
                            &target_precise_stats,
                            precise_reference_count,
                        ),
                        "target_symbol_id": target.symbol.stable_id,
                        "resolution_source": "symbol",
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
                    })
                    .to_string(),
                );
                resolution_precision = Some("heuristic".to_owned());

                Ok(Json(FindReferencesResponse { matches, note }))
            })();

            FindReferencesExecution {
                result,
                scoped_repository_ids,
                selected_symbol_id,
                selected_precise_symbol,
                resolution_precision,
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
                "symbol": Self::bounded_text(&params.symbol),
                "limit": params.limit,
                "effective_limit": execution.effective_limit,
            }),
            json!({
                "scoped_repository_ids": execution.scoped_repository_ids,
                "selected_symbol_id": execution.selected_symbol_id,
                "selected_precise_symbol": execution.selected_precise_symbol,
                "resolution_precision": execution.resolution_precision,
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
        description = "Resolve definition locations for a symbol or source position with precise-first deterministic fallback.",
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
                let target = resolved_target.target.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());
                let target_corpus = resolved_target.target.corpus;

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_target = graph.select_precise_symbol_for_navigation(
                    &target_corpus.repository_id,
                    &symbol_query,
                    &target.symbol.name,
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
                                kind: if precise_target.kind.is_empty() {
                                    None
                                } else {
                                    Some(precise_target.kind.clone())
                                },
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
                    let note = Some(
                        json!({
                            "precision": precision,
                            "heuristic": false,
                            "target_symbol_id": target.symbol.stable_id.clone(),
                            "target_precise_symbol": selected_precise_symbol.clone(),
                            "resolution_source": resolution_source.clone(),
                            "precise": Self::precise_note_with_count(
                                precise_coverage,
                                &cached_precise_graph.ingest_stats,
                                "definition_count",
                                precise_matches.len(),
                            )
                        })
                        .to_string(),
                    );
                    return Ok(Json(GoToDefinitionResponse {
                        matches: precise_matches,
                        note,
                    }));
                }

                let mut matches = vec![NavigationLocation {
                    symbol: target.symbol.name.clone(),
                    repository_id: target_corpus.repository_id.clone(),
                    path: Self::relative_display_path(&target.root, &target.symbol.path),
                    line: target.symbol.line,
                    column: 1,
                    kind: Some(target.symbol.kind.as_str().to_owned()),
                    precision: Some("heuristic".to_owned()),
                }];
                Self::sort_navigation_locations(&mut matches);
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                resolution_precision = Some("heuristic".to_owned());
                match_count = matches.len();
                let note = Some(
                    json!({
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
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "definition_count",
                            0,
                        )
                    })
                    .to_string(),
                );
                Ok(Json(GoToDefinitionResponse { matches, note }))
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
        description = "Resolve declaration anchors (v1 uses definition anchors) for symbol or source position with precise-first deterministic fallback.",
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
                let target = resolved_target.target.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());
                let target_corpus = resolved_target.target.corpus;

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_target = graph.select_precise_symbol_for_navigation(
                    &target_corpus.repository_id,
                    &symbol_query,
                    &target.symbol.name,
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
                                kind: if precise_target.kind.is_empty() {
                                    None
                                } else {
                                    Some(precise_target.kind.clone())
                                },
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
                    let note = Some(
                        json!({
                            "precision": precision,
                            "heuristic": false,
                            "declaration_mode": "definition_anchor_v1",
                            "target_symbol_id": target.symbol.stable_id.clone(),
                            "target_precise_symbol": selected_precise_symbol.clone(),
                            "resolution_source": resolution_source.clone(),
                            "precise": Self::precise_note_with_count(
                                precise_coverage,
                                &cached_precise_graph.ingest_stats,
                                "declaration_count",
                                precise_matches.len(),
                            )
                        })
                        .to_string(),
                    );
                    return Ok(Json(FindDeclarationsResponse {
                        matches: precise_matches,
                        note,
                    }));
                }

                let mut matches = vec![NavigationLocation {
                    symbol: target.symbol.name.clone(),
                    repository_id: target_corpus.repository_id.clone(),
                    path: Self::relative_display_path(&target.root, &target.symbol.path),
                    line: target.symbol.line,
                    column: 1,
                    kind: Some(target.symbol.kind.as_str().to_owned()),
                    precision: Some("heuristic".to_owned()),
                }];
                Self::sort_navigation_locations(&mut matches);
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                resolution_precision = Some("heuristic".to_owned());
                match_count = matches.len();
                let note = Some(
                    json!({
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
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "declaration_count",
                            0,
                        )
                    })
                    .to_string(),
                );
                Ok(Json(FindDeclarationsResponse { matches, note }))
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
        description = "Find implementation targets for a symbol with precise-first deterministic fallback.",
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
                let target = resolved_target.target.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());
                let target_corpus = resolved_target.target.corpus;

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_targets = graph.matching_precise_symbols_for_navigation(
                    &target_corpus.repository_id,
                    &symbol_query,
                    &target.symbol.name,
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
                if precise_matches.len() > limit {
                    precise_matches.truncate(limit);
                }

                if !precise_matches.is_empty() {
                    let precision = Self::precise_resolution_precision(precise_coverage);
                    resolution_precision = Some(precision.to_owned());
                    match_count = precise_matches.len();
                    let note = Some(
                        json!({
                            "precision": precision,
                            "heuristic": false,
                            "target_symbol_id": target.symbol.stable_id.clone(),
                            "target_precise_symbol": selected_precise_symbol.clone(),
                            "resolution_source": resolution_source.clone(),
                            "precise": Self::precise_note_with_count(
                                precise_coverage,
                                &cached_precise_graph.ingest_stats,
                                "implementation_count",
                                precise_matches.len(),
                            )
                        })
                        .to_string(),
                    );
                    return Ok(Json(FindImplementationsResponse {
                        matches: precise_matches,
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
                        && left.relation == right.relation
                        && left.precision == right.precision
                        && left.fallback_reason == right.fallback_reason
                });
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                resolution_precision = Some("heuristic".to_owned());
                match_count = matches.len();
                let note = Some(
                    json!({
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
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "implementation_count",
                            matches.len(),
                        )
                    })
                    .to_string(),
                );
                Ok(Json(FindImplementationsResponse { matches, note }))
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
        description = "Return incoming call hierarchy entries for a symbol with precise-first deterministic fallback.",
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
                let target = resolved_target.target.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());
                let target_corpus = resolved_target.target.corpus;

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_targets = graph.matching_precise_symbols_for_navigation(
                    &target_corpus.repository_id,
                    &symbol_query,
                    &target.symbol.name,
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
                    let note = Some(
                        json!({
                            "precision": precision,
                            "heuristic": false,
                            "target_symbol_id": target.symbol.stable_id.clone(),
                            "target_precise_symbol": selected_precise_symbol.clone(),
                            "resolution_source": resolution_source.clone(),
                            "precise": Self::precise_note_with_count(
                                precise_coverage,
                                &cached_precise_graph.ingest_stats,
                                "incoming_count",
                                precise_matches.len(),
                            )
                        })
                        .to_string(),
                    );
                    return Ok(Json(IncomingCallsResponse {
                        matches: precise_matches,
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
                    })
                    .collect::<Vec<_>>();
                Self::sort_call_hierarchy_matches(&mut matches);
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                resolution_precision = Some("heuristic".to_owned());
                match_count = matches.len();
                let note = Some(
                    json!({
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
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "incoming_count",
                            0,
                        )
                    })
                    .to_string(),
                );
                Ok(Json(IncomingCallsResponse { matches, note }))
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
        description = "Return outgoing call hierarchy entries for a symbol with precise-first deterministic fallback.",
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
                let target = resolved_target.target.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());
                let target_corpus = resolved_target.target.corpus;

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_targets = graph.matching_precise_symbols_for_navigation(
                    &target_corpus.repository_id,
                    &symbol_query,
                    &target.symbol.name,
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
                if precise_matches.len() > limit {
                    precise_matches.truncate(limit);
                }

                if !precise_matches.is_empty() {
                    let precision = Self::precise_resolution_precision(precise_coverage);
                    resolution_precision = Some(precision.to_owned());
                    match_count = precise_matches.len();
                    let note = Some(
                        json!({
                            "precision": precision,
                            "heuristic": false,
                            "target_symbol_id": target.symbol.stable_id.clone(),
                            "target_precise_symbol": selected_precise_symbol.clone(),
                            "resolution_source": resolution_source.clone(),
                            "precise": Self::precise_note_with_count(
                                precise_coverage,
                                &cached_precise_graph.ingest_stats,
                                "outgoing_count",
                                precise_matches.len(),
                            )
                        })
                        .to_string(),
                    );
                    return Ok(Json(OutgoingCallsResponse {
                        matches: precise_matches,
                        note,
                    }));
                }

                let mut matches = graph
                    .outgoing_adjacency(&target.symbol.stable_id)
                    .into_iter()
                    .filter(|adjacent| Self::is_heuristic_call_relation(adjacent.relation))
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
                    })
                    .collect::<Vec<_>>();
                Self::sort_call_hierarchy_matches(&mut matches);
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                resolution_precision = Some("heuristic".to_owned());
                match_count = matches.len();
                let note = Some(
                    json!({
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
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "outgoing_count",
                            0,
                        )
                    })
                    .to_string(),
                );
                Ok(Json(OutgoingCallsResponse { matches, note }))
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
        description = "Return a deterministic symbol outline for a supported source file.",
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

                let language = SymbolLanguage::from_path(&absolute_path).ok_or_else(|| {
                    Self::invalid_params(
                        "document_symbols only supports Rust and PHP files",
                        Some(json!({
                            "path": display_path.clone(),
                            "supported_extensions": [".rs", ".php"],
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

                let outline = symbols
                    .into_iter()
                    .map(|symbol| crate::mcp::types::DocumentSymbolItem {
                        symbol: symbol.name,
                        kind: symbol.kind.as_str().to_owned(),
                        repository_id: repository_id.clone(),
                        path: display_path.clone(),
                        line: symbol.span.start_line,
                        column: symbol.span.start_column,
                        end_line: Some(symbol.span.end_line),
                        end_column: Some(symbol.span.end_column),
                        container: None,
                    })
                    .collect::<Vec<_>>();
                symbol_count = outline.len();

                let note = Some(
                    json!({
                        "source": "tree_sitter",
                        "language": language.as_str(),
                        "symbol_count": symbol_count,
                        "heuristic": false,
                    })
                    .to_string(),
                );
                Ok(Json(DocumentSymbolsResponse {
                    symbols: outline,
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
        description = "Run deterministic tree-sitter structural query search for Rust/PHP sources.",
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
                        let Some(language) = SymbolLanguage::from_path(source_path) else {
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

                let note = Some(
                    json!({
                        "source": "tree_sitter_query",
                        "language": language_filter.clone().unwrap_or_else(|| "mixed".to_owned()),
                        "heuristic": false,
                        "diagnostics_count": diagnostics_count,
                        "files_scanned": files_scanned,
                        "files_matched": files_matched,
                    })
                    .to_string(),
                );
                Ok(Json(SearchStructuralResponse { matches, note }))
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
        description = "Run a deep-search playbook and return a deterministic trace artifact.",
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
        description = "Replay a deep-search playbook against an expected trace and return deterministic diff output.",
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
        description = "Compose deterministic citation payloads from a deep-search trace artifact.",
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
                    "Use list_repositories -> search_text/search_hybrid/read_file first. Runtime tool-surface profile is `{active_profile}` (set `{TOOL_SURFACE_PROFILE_ENV}=extended` to include deep-search tools). For focused traces, provide search_text.path_regex to constrain noise. read_file returns full content when max_bytes is omitted; when capped, invalid_params includes suggested_max_bytes; use line_start/line_end for targeted slices. search_symbol returns tree-sitter symbol matches and find_references prefers precise SCIP references with deterministic heuristic fallback metadata in note."
                ),
            )
    }
}

#[cfg(test)]
mod runtime_gate_tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::domain::FriggError;
    use crate::settings::FriggConfig;
    use rmcp::model::ErrorCode;

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

    #[test]
    fn deep_search_tools_are_hidden_by_default_runtime_options() {
        let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false, false);
        let names = to_set(server.runtime_registered_tool_names());

        for tool_name in FriggMcpServer::DEEP_SEARCH_TOOL_NAMES {
            assert!(
                !names.contains(tool_name),
                "deep-search tool should not be registered by default: {tool_name}"
            );
        }
        assert!(
            names.contains("list_repositories"),
            "core tools should remain registered when deep-search tools are disabled"
        );
    }

    #[test]
    fn deep_search_tools_are_registered_when_runtime_option_enabled() {
        let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false, true);
        let names = to_set(server.runtime_registered_tool_names());

        for tool_name in FriggMcpServer::DEEP_SEARCH_TOOL_NAMES {
            assert!(
                names.contains(tool_name),
                "deep-search tool should be registered when enabled: {tool_name}"
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
}
