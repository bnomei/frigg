use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::domain::model::{ReferenceMatch, SymbolMatch};
use crate::domain::{ChannelResult, EvidenceChannel, FriggError, WorkloadPrecisionMode};
use crate::graph::{
    PreciseRelationshipKind, RelationKind, ScipIngestError, ScipResourceBudgets, SymbolGraph,
};
use crate::indexer::{
    FileMetadataDigest, HeuristicReference, HeuristicReferenceConfidence,
    HeuristicReferenceEvidence, HeuristicReferenceResolver, ManifestBuilder,
    ManifestDiagnosticKind, ReindexMode, SourceSpan, SymbolDefinition, SymbolExtractionOutput,
    byte_offset_for_line_column, extract_php_source_evidence_from_source,
    extract_symbols_for_paths, extract_symbols_from_source, inspect_syntax_tree_in_source,
    navigation_symbol_target_rank, php_declaration_relation_edges_for_file,
    php_heuristic_implementation_candidates_for_target, register_symbol_definitions,
    reindex_repository_with_runtime_config, resolve_php_target_evidence_edges,
    search_structural_in_source,
};
use crate::languages::{
    FLUX_REGISTRY_VERSION, HeuristicImplementationStrategy, LanguageCapability,
    LanguageSupportCapability, SymbolLanguage, extract_blade_source_evidence_from_source,
    heuristic_implementation_strategy, heuristic_rust_implementation_candidates,
    mark_local_flux_overlays, parse_rust_impl_signature, parse_supported_language,
    resolve_blade_relation_evidence_edges, rust_enclosing_symbol_context,
    rust_navigation_query_hint_from_source, rust_relative_path_module_segments,
    rust_source_suffix_looks_like_call, supported_language_for_path,
};
use crate::manifest_validation::{
    RepositoryManifestFreshness, RepositorySemanticFreshness, repository_freshness_status,
};
use crate::path_class::{repository_path_class, repository_path_class_rank};
use crate::searcher::{
    HybridChannelWeights, ProjectionStoreService, SearchDiagnosticKind, SearchFilters,
    SearchHybridQuery, SearchTextQuery, TextSearcher, ValidatedManifestCandidateCache,
    compile_safe_regex,
};
use crate::settings::SemanticRuntimeCredentials;
use crate::settings::{FriggConfig, SemanticRuntimeConfig};
use crate::storage::{Storage, ensure_provenance_db_parent_dir, resolve_provenance_db_path};
use protobuf::Enum;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{
    Implementation, Meta, ProgressNotificationParam, ServerCapabilities, ServerInfo,
};
use rmcp::transport::{
    StreamableHttpServerConfig, StreamableHttpService,
    streamable_http_server::session::local::LocalSessionManager,
};
use rmcp::{
    ErrorData, Peer, RoleServer, ServerHandler, ServiceExt, tool, tool_handler, tool_router,
};
use scip::types::symbol_information::Kind as ScipSymbolKind;
use serde_json::{Value, json};
use tokio::task;
use tracing::warn;

use crate::mcp::advanced::deep_search::{
    DeepSearchHarness, DeepSearchPlaybook, DeepSearchTraceArtifact, DeepSearchTraceOutcome,
};
use crate::mcp::explorer::{
    DEFAULT_CONTEXT_LINES, DEFAULT_MAX_MATCHES, ExploreMatcher, ExploreScopeRequest,
    LossyLineSliceError, MAX_CONTEXT_LINES, line_window_around_anchor, validate_anchor,
    validate_cursor,
};
use crate::mcp::guidance::{
    ROUTING_GUIDE_PROMPT_NAME, SHELL_GUIDANCE_RESOURCE_URI, SUPPORT_MATRIX_RESOURCE_URI,
    TOOL_SURFACE_RESOURCE_URI, guidance_prompts, policy_resources, read_guidance_prompt,
    read_policy_resource,
};
use crate::mcp::provenance_cache::{ProvenancePersistenceStage, ProvenanceStorageCacheKey};
use crate::mcp::server_cache::{
    CachedFindDeclarationsResponse, CachedGoToDefinitionResponse, CachedHeuristicReferences,
    CachedRepositorySummary, CachedSearchHybridResponse, CachedSearchSymbolResponse,
    CachedSearchTextResponse, CachedWorkspacePreciseGeneration, FileContentSnapshot,
    FileContentWindowCache, FileContentWindowCacheKey, FindDeclarationsResponseCacheKey,
    GoToDefinitionResponseCacheKey, HeuristicReferenceCacheKey, RepositoryFreshnessCacheScope,
    RepositoryResponseCacheFreshness, RepositoryResponseCacheFreshnessMode, RuntimeCacheBudget,
    RuntimeCacheEvent, RuntimeCacheFamily, RuntimeCacheRegistry, RuntimeCacheTelemetry,
    SearchHybridResponseCacheKey, SearchSymbolResponseCacheKey, SearchTextResponseCacheKey,
    WorkspaceSemanticRefreshPlan, response_cache_scopes_include_repository,
};
use crate::mcp::server_state::{
    CachedPreciseGraph, DeterministicSignatureHasher, ExploreExecution, FindReferencesExecution,
    FindReferencesResourceBudgets, NavigationToolExecution, PreciseArtifactFailureSample,
    PreciseCoverageMode, PreciseGraphCacheKey, PreciseIngestStats, RankedSymbolMatch,
    ReadFileExecution, RepositoryDiagnosticsSummary, RepositorySymbolCorpus,
    ResolvedNavigationTarget, ResolvedSymbolTarget, RuntimeTaskRegistry, ScipArtifactDigest,
    ScipArtifactDiscovery, ScipArtifactFormat, ScipCandidateDirectoryDigest, SearchHybridExecution,
    SearchSymbolExecution, SearchTextExecution, SymbolCandidate, SymbolCorpusCacheKey,
};
use crate::mcp::tool_surface::{
    TOOL_SURFACE_PROFILE_ENV, ToolSurfaceParityDiff, ToolSurfaceProfile,
    active_runtime_tool_surface_profile, diff_runtime_against_profile_manifest,
    manifest_for_tool_surface_profile,
};
use crate::mcp::types::{
    CallHierarchyMatch, DeepSearchComposeCitationsParams, DeepSearchComposeCitationsResponse,
    DeepSearchReplayParams, DeepSearchReplayResponse, DeepSearchRunParams, DeepSearchRunResponse,
    DocumentSymbolsParams, DocumentSymbolsResponse, ExploreMatch, ExploreMetadata,
    ExploreOperation, ExploreParams, ExploreResponse, ExploreWindow, FindDeclarationsParams,
    FindDeclarationsResponse, FindImplementationsParams, FindImplementationsResponse,
    FindReferencesParams, FindReferencesResponse, GoToDefinitionParams, GoToDefinitionResponse,
    ImplementationMatch, IncomingCallsParams, IncomingCallsResponse, InspectSyntaxTreeParams,
    InspectSyntaxTreeResponse, ListRepositoriesParams, ListRepositoriesResponse,
    NavigationAvailability, NavigationLocation, NavigationMode, OutgoingCallsParams,
    OutgoingCallsResponse, ReadFileParams, ReadFileResponse, RecentProvenanceSummary,
    RepositorySummary, RuntimeStatusSummary, RuntimeTaskKind, RuntimeTaskStatus,
    SearchHybridChannelWeightsParams, SearchHybridMatch, SearchHybridParams, SearchHybridResponse,
    SearchPatternType, SearchStructuralParams, SearchStructuralResponse, SearchSymbolParams,
    SearchSymbolPathClass, SearchSymbolResponse, SearchTextParams, SearchTextResponse,
    SyntaxTreeNodeItem, WRITE_CONFIRM_PARAM, WRITE_CONFIRMATION_REQUIRED_ERROR_CODE,
    WorkspaceAttachAction, WorkspaceAttachParams, WorkspaceAttachResponse, WorkspaceCurrentParams,
    WorkspaceCurrentResponse, WorkspaceDetachParams, WorkspaceDetachResponse,
    WorkspaceIndexComponentState, WorkspaceIndexComponentSummary, WorkspaceIndexHealthSummary,
    WorkspacePreciseGenerationStatus, WorkspacePreciseGenerationSummary,
    WorkspacePreciseGeneratorState, WorkspacePreciseGeneratorSummary, WorkspacePrepareParams,
    WorkspacePrepareResponse, WorkspaceReindexParams, WorkspaceReindexResponse,
    WorkspaceResolveMode, WorkspaceStorageIndexState, WorkspaceStorageSummary,
};
use crate::mcp::workspace_registry::{AttachedWorkspace, WorkspaceRegistry};
use crate::settings::RuntimeProfile;

mod content;
mod deep_search;
mod execution;
mod navigation_cache;
mod navigation_tools;
mod precise_graph;
mod provenance;
mod runtime_status;
mod search_tools;
mod symbol_index;
mod workspace;

#[derive(Debug, Clone)]
struct NavigationLocationTokenHint {
    symbol_query: String,
    relative_path: String,
    resolution_source: &'static str,
    rust_hint: Option<crate::languages::RustNavigationQueryHint>,
}
pub type FriggMcpService = StreamableHttpService<FriggMcpServer, LocalSessionManager>;

#[derive(Clone)]
pub struct FriggMcpServer {
    config: Arc<FriggConfig>,
    tool_router: ToolRouter<Self>,
    tool_surface_profile: ToolSurfaceProfile,
    runtime_state: FriggMcpRuntimeState,
    session_state: FriggMcpSessionState,
    cache_state: FriggMcpCacheState,
    provenance_state: FriggMcpProvenanceState,
}

#[derive(Clone)]
struct FriggMcpRuntimeState {
    runtime_profile: RuntimeProfile,
    runtime_watch_active: bool,
    workspace_registry: Arc<RwLock<WorkspaceRegistry>>,
    watch_runtime: Arc<RwLock<Option<Arc<crate::watch::WatchRuntime>>>>,
    runtime_task_registry: Arc<RwLock<RuntimeTaskRegistry>>,
    validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    searcher_projection_store_service: ProjectionStoreService,
    runtime_cache_registry: Arc<RwLock<RuntimeCacheRegistry>>,
    runtime_cache_telemetry: Arc<RwLock<BTreeMap<RuntimeCacheFamily, RuntimeCacheTelemetry>>>,
    precise_generation_status_cache:
        Arc<RwLock<BTreeMap<String, CachedWorkspacePreciseGeneration>>>,
}

#[derive(Clone)]
struct FriggMcpSessionState {
    inner: Arc<FriggMcpSessionStateInner>,
}

struct FriggMcpSessionStateInner {
    workspace_registry: Arc<RwLock<WorkspaceRegistry>>,
    watch_runtime: Arc<RwLock<Option<Arc<crate::watch::WatchRuntime>>>>,
    adopted_repository_ids: RwLock<BTreeSet<String>>,
    session_default_repository_id: RwLock<Option<String>>,
}

#[derive(Clone)]
struct FriggMcpCacheState {
    symbol_corpus_cache: Arc<RwLock<BTreeMap<SymbolCorpusCacheKey, Arc<RepositorySymbolCorpus>>>>,
    precise_graph_cache: Arc<RwLock<BTreeMap<PreciseGraphCacheKey, Arc<CachedPreciseGraph>>>>,
    latest_precise_graph_cache: Arc<RwLock<BTreeMap<String, Arc<CachedPreciseGraph>>>>,
    provenance_storage_cache: Arc<RwLock<BTreeMap<ProvenanceStorageCacheKey, Arc<Storage>>>>,
    repository_summary_cache: Arc<RwLock<BTreeMap<String, CachedRepositorySummary>>>,
    file_content_window_cache: Arc<RwLock<FileContentWindowCache>>,
    search_text_response_cache:
        Arc<RwLock<BTreeMap<SearchTextResponseCacheKey, CachedSearchTextResponse>>>,
    search_hybrid_response_cache:
        Arc<RwLock<BTreeMap<SearchHybridResponseCacheKey, CachedSearchHybridResponse>>>,
    search_symbol_response_cache:
        Arc<RwLock<BTreeMap<SearchSymbolResponseCacheKey, CachedSearchSymbolResponse>>>,
    go_to_definition_response_cache:
        Arc<RwLock<BTreeMap<GoToDefinitionResponseCacheKey, CachedGoToDefinitionResponse>>>,
    find_declarations_response_cache:
        Arc<RwLock<BTreeMap<FindDeclarationsResponseCacheKey, CachedFindDeclarationsResponse>>>,
    heuristic_reference_cache:
        Arc<RwLock<BTreeMap<HeuristicReferenceCacheKey, CachedHeuristicReferences>>>,
    compiled_safe_regex_cache: Arc<RwLock<BTreeMap<String, regex::Regex>>>,
}

#[derive(Clone)]
struct FriggMcpProvenanceState {
    best_effort: bool,
    enabled: bool,
}

#[allow(clippy::enum_variant_names)]
enum FriggErrorTransportCode {
    InvalidParams,
    ResourceNotFound,
    InvalidRequest,
    Internal,
}

struct FriggErrorTranslation {
    transport_code: FriggErrorTransportCode,
    message: String,
    error_code: &'static str,
    retryable: bool,
    detail: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReadOnlyToolExecutionContext {
    pub(super) tool_name: &'static str,
    pub(super) repository_hint: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ScopedReadOnlyToolExecutionContext {
    #[cfg(test)]
    pub(super) base: ReadOnlyToolExecutionContext,
    pub(super) scoped_workspaces: Vec<AttachedWorkspace>,
    pub(super) scoped_repository_ids: Vec<String>,
    pub(super) cache_freshness: RepositoryResponseCacheFreshness,
}

impl FriggMcpServer {
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
    const PROVENANCE_MATCH_SAMPLE_LIMIT: usize = 4;
    const RUNTIME_RECENT_PROVENANCE_LIMIT: usize = 8;
    const REPOSITORY_SUMMARY_CACHE_TTL: Duration = Duration::from_secs(1);
    fn filtered_tool_router(profile: ToolSurfaceProfile) -> ToolRouter<Self> {
        let mut router = Self::tool_router();
        let allowed_tools = manifest_for_tool_surface_profile(profile)
            .tool_names
            .into_iter()
            .collect::<BTreeSet<_>>();
        for tool_name in router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect::<Vec<_>>()
        {
            if !allowed_tools.contains(&tool_name) {
                router.remove_route(&tool_name);
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

    fn confirmation_required(tool_name: &'static str) -> ErrorData {
        Self::internal_with_code(
            format!("{tool_name} requires explicit {WRITE_CONFIRM_PARAM}=true before side effects"),
            WRITE_CONFIRMATION_REQUIRED_ERROR_CODE,
            false,
            Some(json!({
                "tool_name": tool_name,
                "confirm_param": WRITE_CONFIRM_PARAM,
            })),
        )
    }

    fn require_confirm(tool_name: &'static str, confirm: Option<bool>) -> Result<(), ErrorData> {
        if confirm == Some(true) {
            return Ok(());
        }
        Err(Self::confirmation_required(tool_name))
    }

    async fn notify_progress(
        meta: &Meta,
        client: &Peer<RoleServer>,
        progress: f64,
        total: f64,
        message: impl Into<String>,
    ) {
        let Some(progress_token) = meta.get_progress_token() else {
            return;
        };
        let _ = client
            .notify_progress(ProgressNotificationParam {
                progress_token,
                progress,
                total: Some(total),
                message: Some(message.into()),
            })
            .await;
    }

    fn build_frigg_error_data(translation: FriggErrorTranslation) -> ErrorData {
        match translation.transport_code {
            FriggErrorTransportCode::InvalidParams => {
                Self::invalid_params(translation.message, translation.detail)
            }
            FriggErrorTransportCode::ResourceNotFound => {
                Self::resource_not_found(translation.message, translation.detail)
            }
            FriggErrorTransportCode::InvalidRequest => {
                Self::access_denied(translation.message, translation.detail)
            }
            FriggErrorTransportCode::Internal => Self::internal_with_code(
                translation.message,
                translation.error_code,
                translation.retryable,
                translation.detail,
            ),
        }
    }

    fn translate_frigg_error(err: FriggError) -> FriggErrorTranslation {
        match err {
            FriggError::InvalidInput(message) => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::InvalidParams,
                message,
                error_code: "invalid_params",
                retryable: false,
                detail: None,
            },
            FriggError::NotFound(message) => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::ResourceNotFound,
                message,
                error_code: "resource_not_found",
                retryable: false,
                detail: None,
            },
            FriggError::AccessDenied(message) => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::InvalidRequest,
                message,
                error_code: "access_denied",
                retryable: false,
                detail: None,
            },
            FriggError::Io(err) => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::Internal,
                message: "IO failure".to_string(),
                error_code: "internal",
                retryable: false,
                detail: Some(json!({
                    "error_class": "io",
                    "io_error": Self::bounded_text(&err.to_string()),
                })),
            },
            FriggError::StrictSemanticFailure { reason } => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::Internal,
                message: format!("semantic channel strict failure: {reason}"),
                error_code: "unavailable",
                retryable: true,
                detail: Some(json!({
                    "error_class": "semantic",
                    "semantic_status": "strict_failure",
                    "semantic_reason": Self::bounded_text(&reason),
                })),
            },
            FriggError::Internal(message) => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::Internal,
                message,
                error_code: "internal",
                retryable: false,
                detail: None,
            },
        }
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
        Self::build_frigg_error_data(Self::translate_frigg_error(err))
    }

    pub(super) fn read_only_tool_execution_context(
        &self,
        tool_name: &'static str,
        repository_hint: Option<String>,
    ) -> ReadOnlyToolExecutionContext {
        ReadOnlyToolExecutionContext {
            tool_name,
            repository_hint,
        }
    }

    pub(super) fn scoped_read_only_tool_execution_context(
        &self,
        tool_name: &'static str,
        repository_hint: Option<String>,
        freshness_mode: RepositoryResponseCacheFreshnessMode,
    ) -> Result<ScopedReadOnlyToolExecutionContext, ErrorData> {
        let base = self.read_only_tool_execution_context(tool_name, repository_hint);
        let scoped_workspaces =
            self.attached_workspaces_for_repository(base.repository_hint.as_deref())?;
        let scoped_repository_ids = scoped_workspaces
            .iter()
            .map(|workspace| workspace.repository_id.clone())
            .collect::<Vec<_>>();
        let cache_freshness =
            self.repository_response_cache_freshness(&scoped_workspaces, freshness_mode)?;

        Ok(ScopedReadOnlyToolExecutionContext {
            #[cfg(test)]
            base,
            scoped_workspaces,
            scoped_repository_ids,
            cache_freshness,
        })
    }

    async fn run_read_only_tool_blocking<T, F>(
        &self,
        context: &ReadOnlyToolExecutionContext,
        task_fn: F,
    ) -> Result<T, ErrorData>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        Self::run_blocking_task(context.tool_name, task_fn).await
    }

    fn finalize_read_only_tool<T>(
        &self,
        context: &ReadOnlyToolExecutionContext,
        result: Result<Json<T>, ErrorData>,
        provenance_result: Result<(), ErrorData>,
    ) -> Result<Json<T>, ErrorData> {
        self.finalize_with_provenance(context.tool_name, result, provenance_result)
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
        location_relative_path: Option<&str>,
        rust_hint: Option<&crate::languages::RustNavigationQueryHint>,
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
            let left_context = Self::navigation_symbol_context_ranks(
                corpora,
                left,
                location_relative_path,
                rust_hint,
            );
            let right_context = Self::navigation_symbol_context_ranks(
                corpora,
                right,
                location_relative_path,
                rust_hint,
            );
            left.rank
                .cmp(&right.rank)
                .then(left_context.cmp(&right_context))
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

    fn navigation_symbol_context_ranks(
        corpora: &[Arc<RepositorySymbolCorpus>],
        candidate: &SymbolCandidate,
        location_relative_path: Option<&str>,
        rust_hint: Option<&crate::languages::RustNavigationQueryHint>,
    ) -> (u8, u8, u8, u8, u8) {
        let relative_path = Self::relative_display_path(&candidate.root, &candidate.symbol.path);
        let same_file_rank = rust_hint.map_or(1, |hint| {
            if hint.prefer_same_file && location_relative_path == Some(relative_path.as_str()) {
                0
            } else {
                1
            }
        });
        let method_rank = rust_hint.map_or(0, |hint| {
            if hint.prefer_method && candidate.symbol.kind != crate::indexer::SymbolKind::Method {
                1
            } else {
                0
            }
        });
        let module_rank = rust_hint.map_or(0, |hint| {
            Self::rust_navigation_module_affinity_rank(&hint.module_path_segments, &relative_path)
        });
        let impl_rank = rust_hint.map_or(0, |hint| {
            if hint.enclosing_impl_type.is_none() {
                return 0;
            }
            let Some(corpus) = corpora
                .iter()
                .find(|corpus| corpus.repository_id == candidate.repository_id)
            else {
                return 1;
            };
            let context = rust_enclosing_symbol_context(&candidate.symbol, &corpus.symbols);
            if context
                .impl_type
                .as_deref()
                .zip(hint.enclosing_impl_type.as_deref())
                .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
            {
                0
            } else {
                1
            }
        });
        let trait_rank = rust_hint.map_or(0, |hint| {
            if hint.enclosing_trait.is_none() {
                return 0;
            }
            let Some(corpus) = corpora
                .iter()
                .find(|corpus| corpus.repository_id == candidate.repository_id)
            else {
                return 1;
            };
            let context = rust_enclosing_symbol_context(&candidate.symbol, &corpus.symbols);
            let target_trait = hint.enclosing_trait.as_deref().unwrap_or_default();
            if context
                .trait_name
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(target_trait))
                || context
                    .impl_trait
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(target_trait))
            {
                0
            } else {
                1
            }
        });

        (
            same_file_rank,
            method_rank,
            module_rank,
            impl_rank,
            trait_rank,
        )
    }

    fn rust_navigation_module_affinity_rank(hint_segments: &[String], relative_path: &str) -> u8 {
        if hint_segments.is_empty() {
            return 0;
        }
        let candidate_segments = rust_relative_path_module_segments(relative_path);
        if candidate_segments.is_empty() {
            return 3;
        }
        if candidate_segments == hint_segments {
            return 0;
        }
        if candidate_segments.starts_with(hint_segments)
            || candidate_segments.ends_with(hint_segments)
        {
            return 0;
        }
        if hint_segments
            .iter()
            .all(|segment| candidate_segments.contains(segment))
        {
            return 1;
        }
        if hint_segments
            .iter()
            .any(|segment| candidate_segments.contains(segment))
        {
            return 2;
        }
        3
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

    fn navigation_symbol_query_token_from_location(
        corpora: &[Arc<RepositorySymbolCorpus>],
        raw_path: &str,
        line: usize,
        column: usize,
    ) -> Option<NavigationLocationTokenHint> {
        for corpus in corpora {
            let requested_path = Self::requested_location_path_for_corpus(corpus, raw_path);
            let absolute_path = corpus.root.join(&requested_path);
            let Ok(source) = fs::read_to_string(&absolute_path) else {
                continue;
            };
            if supported_language_for_path(&absolute_path, LanguageCapability::StructuralSearch)
                == Some(SymbolLanguage::Rust)
                && let Some(rust_hint) =
                    rust_navigation_query_hint_from_source(&absolute_path, &source, line, column)
                && !rust_hint.symbol_query.is_empty()
            {
                return Some(NavigationLocationTokenHint {
                    symbol_query: rust_hint.symbol_query.clone(),
                    relative_path: requested_path,
                    resolution_source: "location_token_rust",
                    rust_hint: Some(rust_hint),
                });
            }
            let Some(offset) = byte_offset_for_line_column(&source, line, column) else {
                continue;
            };
            let Some(token) = Self::identifier_token_around_offset(&source, offset) else {
                continue;
            };
            if !token.is_empty() {
                return Some(NavigationLocationTokenHint {
                    symbol_query: token,
                    relative_path: requested_path,
                    resolution_source: "location_token",
                    rust_hint: None,
                });
            }
        }
        None
    }

    fn identifier_token_around_offset(source: &str, offset: usize) -> Option<String> {
        fn is_identifier_byte(byte: u8) -> bool {
            byte.is_ascii_alphanumeric() || byte == b'_'
        }

        let bytes = source.as_bytes();
        if bytes.is_empty() {
            return None;
        }
        let mut index = offset.min(bytes.len().saturating_sub(1));
        if !is_identifier_byte(bytes[index]) {
            if index > 0 && is_identifier_byte(bytes[index - 1]) {
                index -= 1;
            } else {
                let mut probe = index;
                while probe < bytes.len()
                    && !is_identifier_byte(bytes[probe])
                    && bytes[probe] != b'\n'
                {
                    probe += 1;
                }
                if probe >= bytes.len() || !is_identifier_byte(bytes[probe]) {
                    return None;
                }
                index = probe;
            }
        }

        let mut start = index;
        while start > 0 && is_identifier_byte(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = index + 1;
        while end < bytes.len() && is_identifier_byte(bytes[end]) {
            end += 1;
        }
        (start < end).then(|| source[start..end].to_owned())
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
            let target = Self::resolve_navigation_symbol_target(
                corpora,
                query,
                repository_id_hint,
                None,
                None,
            )?;
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
        if let Some(column) = column
            && let Some(location_hint) =
                Self::navigation_symbol_query_token_from_location(corpora, raw_path, line, column)
            && let Ok(target) = Self::resolve_navigation_symbol_target(
                corpora,
                &location_hint.symbol_query,
                repository_id_hint,
                Some(location_hint.relative_path.as_str()),
                location_hint.rust_hint.as_ref(),
            )
        {
            return Ok(ResolvedNavigationTarget {
                symbol_query: location_hint.symbol_query,
                target,
                resolution_source: location_hint.resolution_source,
            });
        }
        let symbol_query = Self::resolve_navigation_symbol_query_from_location(
            corpora,
            raw_path,
            line,
            column,
            repository_id_hint,
        )?;
        let target = Self::resolve_navigation_symbol_target(
            corpora,
            &symbol_query,
            repository_id_hint,
            None,
            None,
        )?;
        Ok(ResolvedNavigationTarget {
            symbol_query,
            target,
            resolution_source: "location_enclosing_symbol",
        })
    }

    fn try_precise_definition_fast_path(
        &self,
        repository_id_hint: Option<&str>,
        raw_path: &str,
        line: usize,
        column: Option<usize>,
        limit: usize,
    ) -> Result<Option<(Json<GoToDefinitionResponse>, String, String, String)>, ErrorData> {
        let scoped_roots = self.roots_for_repository(repository_id_hint)?;
        if repository_id_hint.is_none() && scoped_roots.len() != 1 {
            return Ok(None);
        }

        let mut scoped_roots = scoped_roots;
        scoped_roots.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

        for (repository_id, root) in scoped_roots {
            let Ok(cached_precise_graph) = self.precise_graph_for_repository_root(
                &repository_id,
                &root,
                self.find_references_resource_budgets(),
            ) else {
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
                    kind: Self::display_symbol_kind(&precise_target.kind),
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
                    mode: Self::navigation_mode_from_precision_label(Some(&precision)),
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
                    parse_rust_impl_signature(enclosing_symbol.name.as_str())?;
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
        line.match_indices(target_name.as_str()).any(|(index, _)| {
            let suffix_start = index.saturating_add(target_name.len()).min(line.len());
            line.get(suffix_start..)
                .map(rust_source_suffix_looks_like_call)
                .unwrap_or(false)
        })
    }

    fn precise_symbol_label(precise_symbol: &crate::graph::PreciseSymbolRecord) -> String {
        crate::graph::precise_navigation_identifier(&precise_symbol.display_name)
            .or_else(|| crate::graph::precise_navigation_identifier(&precise_symbol.symbol))
            .unwrap_or_else(|| precise_symbol.symbol.clone())
    }

    fn precise_target_call_name(precise_target: &crate::graph::PreciseSymbolRecord) -> String {
        Self::precise_symbol_label(precise_target)
    }

    fn source_line_for_precise_range<'a>(
        source: &'a str,
        range: &crate::graph::PreciseRange,
    ) -> Option<&'a str> {
        source.lines().nth(range.start_line.saturating_sub(1))
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
        let mut source_cache: BTreeMap<String, Option<String>> = BTreeMap::new();
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
                if !Self::is_precise_callable_kind(&callee_symbol.kind)
                    && !Self::precise_occurrence_has_call_like_source(
                        root,
                        &callee_symbol,
                        &occurrence,
                        &mut source_cache,
                    )
                {
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
                    target_symbol: Self::precise_symbol_label(&callee_symbol),
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
        let note =
            Some(serde_json::to_string(&metadata).expect("metadata payload should serialize"));
        (Some(metadata), note)
    }

    fn metadata_with_freshness_basis(mut metadata: Value, freshness_basis: &Value) -> Value {
        metadata
            .as_object_mut()
            .expect("metadata payload should be an object")
            .insert("freshness_basis".to_owned(), freshness_basis.clone());
        metadata
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

    fn call_hierarchy_availability(
        coverage_mode: PreciseCoverageMode,
        stats: &PreciseIngestStats,
        precise_match_count: usize,
        heuristic_match_count: usize,
    ) -> NavigationAvailability {
        if precise_match_count > 0 {
            return NavigationAvailability {
                status: "available".to_owned(),
                reason: None,
                precise_required_for_complete_results: false,
            };
        }
        if heuristic_match_count > 0 {
            return NavigationAvailability {
                status: "heuristic".to_owned(),
                reason: Some(
                    Self::precise_absence_reason(coverage_mode, stats, precise_match_count)
                        .to_owned(),
                ),
                precise_required_for_complete_results: true,
            };
        }
        if coverage_mode == PreciseCoverageMode::Full {
            return NavigationAvailability {
                status: "available".to_owned(),
                reason: None,
                precise_required_for_complete_results: false,
            };
        }

        NavigationAvailability {
            status: "unavailable".to_owned(),
            reason: Some(
                Self::precise_absence_reason(coverage_mode, stats, precise_match_count).to_owned(),
            ),
            precise_required_for_complete_results: true,
        }
    }

    fn navigation_mode_from_precision_label(label: Option<&str>) -> NavigationMode {
        match label {
            Some("precise") => NavigationMode::Precise,
            Some("precise_partial") => NavigationMode::PrecisePartial,
            Some("heuristic") => NavigationMode::HeuristicNoPrecise,
            _ => NavigationMode::UnavailableNoPrecise,
        }
    }

    fn navigation_mode_from_call_hierarchy_availability(
        availability: &NavigationAvailability,
    ) -> NavigationMode {
        match availability.status.as_str() {
            "available" => NavigationMode::Precise,
            "heuristic" => NavigationMode::HeuristicNoPrecise,
            _ => NavigationMode::UnavailableNoPrecise,
        }
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
        let matches =
            heuristic_rust_implementation_candidates(target_symbol, &target_corpus.symbols)
                .into_iter()
                .map(|candidate| ImplementationMatch {
                    symbol: candidate.symbol,
                    kind: Self::display_symbol_kind(candidate.source_symbol.kind.as_str()),
                    repository_id: target_corpus.repository_id.clone(),
                    path: Self::relative_display_path(target_root, &candidate.source_symbol.path),
                    line: candidate.source_symbol.line,
                    column: 1,
                    relation: Some(candidate.relation.to_owned()),
                    precision: Some("heuristic".to_owned()),
                    fallback_reason: Some("precise_absent".to_owned()),
                })
                .collect::<Vec<_>>();

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
                relation: Some(RelationKind::as_str(relation).to_owned()),
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

    fn is_heuristic_call_relation(relation: RelationKind) -> bool {
        matches!(relation, RelationKind::Calls)
    }

    pub fn new(config: FriggConfig) -> Self {
        Self::new_with_provenance_best_effort(config, Self::provenance_best_effort_from_env())
    }

    pub fn new_with_runtime(
        config: FriggConfig,
        runtime_profile: RuntimeProfile,
        runtime_watch_active: bool,
        runtime_task_registry: Arc<RwLock<RuntimeTaskRegistry>>,
        validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    ) -> Self {
        let provenance_best_effort = Self::provenance_best_effort_from_env();
        let enable_extended_tools =
            active_runtime_tool_surface_profile() == ToolSurfaceProfile::Extended;
        Self::new_with_runtime_context(
            config,
            provenance_best_effort,
            enable_extended_tools,
            runtime_profile,
            runtime_watch_active,
            None,
            runtime_task_registry,
            validated_manifest_candidate_cache,
        )
    }

    pub fn new_with_runtime_context(
        config: FriggConfig,
        provenance_best_effort: bool,
        enable_extended_tools: bool,
        runtime_profile: RuntimeProfile,
        runtime_watch_active: bool,
        watch_runtime: Option<Arc<crate::watch::WatchRuntime>>,
        runtime_task_registry: Arc<RwLock<RuntimeTaskRegistry>>,
        validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
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
        let workspace_registry = Arc::new(RwLock::new(workspace_registry));
        let tool_surface_profile = if enable_extended_tools {
            ToolSurfaceProfile::Extended
        } else {
            ToolSurfaceProfile::Core
        };
        let watch_runtime = Arc::new(RwLock::new(watch_runtime));
        Self {
            config: Arc::new(config),
            tool_router: Self::filtered_tool_router(tool_surface_profile),
            tool_surface_profile,
            runtime_state: FriggMcpRuntimeState {
                runtime_profile,
                runtime_watch_active,
                workspace_registry: Arc::clone(&workspace_registry),
                watch_runtime: Arc::clone(&watch_runtime),
                runtime_task_registry,
                validated_manifest_candidate_cache,
                searcher_projection_store_service: ProjectionStoreService::new(),
                runtime_cache_registry: Arc::new(RwLock::new(RuntimeCacheRegistry::default())),
                runtime_cache_telemetry: Arc::new(RwLock::new(BTreeMap::new())),
                precise_generation_status_cache: Arc::new(RwLock::new(BTreeMap::new())),
            },
            session_state: FriggMcpSessionState::new(workspace_registry, watch_runtime),
            cache_state: FriggMcpCacheState {
                symbol_corpus_cache: Arc::new(RwLock::new(BTreeMap::new())),
                precise_graph_cache: Arc::new(RwLock::new(BTreeMap::new())),
                latest_precise_graph_cache: Arc::new(RwLock::new(BTreeMap::new())),
                provenance_storage_cache: Arc::new(RwLock::new(BTreeMap::new())),
                repository_summary_cache: Arc::new(RwLock::new(BTreeMap::new())),
                file_content_window_cache: Arc::new(RwLock::new(FileContentWindowCache::default())),
                search_text_response_cache: Arc::new(RwLock::new(BTreeMap::new())),
                search_hybrid_response_cache: Arc::new(RwLock::new(BTreeMap::new())),
                search_symbol_response_cache: Arc::new(RwLock::new(BTreeMap::new())),
                go_to_definition_response_cache: Arc::new(RwLock::new(BTreeMap::new())),
                find_declarations_response_cache: Arc::new(RwLock::new(BTreeMap::new())),
                heuristic_reference_cache: Arc::new(RwLock::new(BTreeMap::new())),
                compiled_safe_regex_cache: Arc::new(RwLock::new(BTreeMap::new())),
            },
            provenance_state: FriggMcpProvenanceState {
                best_effort: provenance_best_effort,
                enabled: true,
            },
        }
    }

    pub fn new_with_runtime_options(
        config: FriggConfig,
        provenance_best_effort: bool,
        enable_extended_tools: bool,
    ) -> Self {
        Self::new_with_runtime_context(
            config,
            provenance_best_effort,
            enable_extended_tools,
            RuntimeProfile::StdioEphemeral,
            false,
            None,
            Arc::new(RwLock::new(RuntimeTaskRegistry::new())),
            Arc::new(RwLock::new(ValidatedManifestCandidateCache::default())),
        )
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
            tool_surface_profile: self.tool_surface_profile,
            runtime_state: self.runtime_state.clone(),
            session_state: FriggMcpSessionState::new(
                Arc::clone(&self.runtime_state.workspace_registry),
                self.runtime_state.watch_runtime.clone(),
            ),
            cache_state: self.cache_state.clone(),
            provenance_state: self.provenance_state.clone(),
        }
    }

    pub fn repository_cache_invalidation_callback(
        &self,
    ) -> crate::watch::RepositoryCacheInvalidationCallback {
        let server = self.clone();
        Arc::new(move |repository_id: &str| {
            server.invalidate_repository_summary_cache(repository_id);
            server.invalidate_repository_file_content_cache(repository_id);
            server.scip_invalidate_repository_precise_generation_cache(repository_id);
            server.invalidate_repository_precise_graph_caches(repository_id);
            server.invalidate_repository_search_response_caches(repository_id);
            server.invalidate_repository_navigation_response_caches(repository_id);
        })
    }

    fn runtime_cache_max_entries(&self, family: RuntimeCacheFamily) -> Option<usize> {
        self.runtime_state
            .runtime_cache_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .policy(family)
            .and_then(|policy| policy.budget.max_entries)
    }

    fn runtime_text_searcher(&self, config: FriggConfig) -> TextSearcher {
        TextSearcher::with_runtime_projection_store_service(
            config,
            Arc::clone(&self.runtime_state.validated_manifest_candidate_cache),
            self.runtime_state.searcher_projection_store_service.clone(),
        )
    }

    fn record_runtime_cache_event(
        &self,
        family: RuntimeCacheFamily,
        event: RuntimeCacheEvent,
        count: usize,
    ) {
        if count == 0 {
            return;
        }
        let mut telemetry = self
            .runtime_state
            .runtime_cache_telemetry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        telemetry.entry(family).or_default().record(event, count);
    }

    fn trim_runtime_cache_to_entry_limit<K, V>(
        &self,
        family: RuntimeCacheFamily,
        cache: &mut BTreeMap<K, V>,
    ) where
        K: Ord,
    {
        let Some(limit) = self.runtime_cache_max_entries(family) else {
            return;
        };
        while cache.len() > limit {
            let _ = cache.pop_first();
            self.record_runtime_cache_event(family, RuntimeCacheEvent::Eviction, 1);
        }
    }

    fn runtime_cache_budget(&self, family: RuntimeCacheFamily) -> RuntimeCacheBudget {
        self.runtime_state
            .runtime_cache_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .policy(family)
            .map(|policy| policy.budget)
            .expect("runtime cache family policy should exist")
    }

    fn cached_file_content_window(
        &self,
        cache_key: &FileContentWindowCacheKey,
    ) -> Option<Arc<FileContentSnapshot>> {
        let cached = self
            .cache_state
            .file_content_window_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key);
        self.record_runtime_cache_event(
            RuntimeCacheFamily::FileContentWindow,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    fn file_content_snapshot_for_workspace(
        &self,
        workspace: &AttachedWorkspace,
        canonical_path: &Path,
    ) -> Result<Arc<FileContentSnapshot>, ErrorData> {
        let freshness = self.repository_response_cache_freshness(
            std::slice::from_ref(workspace),
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )?;
        let Some(scopes) = freshness.scopes else {
            let bytes = fs::read(canonical_path).map_err(|err| {
                Self::internal(
                    format!("failed to read file {}: {err}", canonical_path.display()),
                    None,
                )
            })?;
            return Ok(Arc::new(FileContentSnapshot::from_bytes(bytes)));
        };
        let mut scoped_repository_ids = vec![workspace.repository_id.clone()];
        scoped_repository_ids.sort();
        let cache_key = FileContentWindowCacheKey {
            scoped_repository_ids,
            freshness_scopes: scopes,
            canonical_path: canonical_path.to_path_buf(),
        };
        if let Some(cached) = self.cached_file_content_window(&cache_key) {
            return Ok(cached);
        }

        let bytes = fs::read(canonical_path).map_err(|err| {
            Self::internal(
                format!("failed to read file {}: {err}", canonical_path.display()),
                None,
            )
        })?;
        let snapshot = Arc::new(FileContentSnapshot::from_bytes(bytes));
        self.cache_file_content_window(cache_key, Arc::clone(&snapshot));
        Ok(snapshot)
    }

    fn cache_file_content_window(
        &self,
        cache_key: FileContentWindowCacheKey,
        snapshot: Arc<FileContentSnapshot>,
    ) {
        let mut cache = self
            .cache_state
            .file_content_window_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let budget = self.runtime_cache_budget(RuntimeCacheFamily::FileContentWindow);
        let (inserted, evictions) = cache.insert(cache_key, snapshot, budget);
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::FileContentWindow,
                RuntimeCacheEvent::Insert,
                1,
            );
            self.record_runtime_cache_event(
                RuntimeCacheFamily::FileContentWindow,
                RuntimeCacheEvent::Eviction,
                evictions,
            );
        } else {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::FileContentWindow,
                RuntimeCacheEvent::Bypass,
                1,
            );
        }
    }

    fn invalidate_repository_file_content_cache(&self, repository_id: &str) {
        let mut cache = self
            .cache_state
            .file_content_window_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = cache.retain_repository(repository_id);
        self.record_runtime_cache_event(
            RuntimeCacheFamily::FileContentWindow,
            RuntimeCacheEvent::Invalidation,
            before,
        );
    }

    fn runtime_cache_contract_summary(&self, families: &[RuntimeCacheFamily]) -> Value {
        let registry = self
            .runtime_state
            .runtime_cache_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let telemetry = self
            .runtime_state
            .runtime_cache_telemetry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        Value::Array(
            families
                .iter()
                .filter_map(|family| {
                    let policy = registry.policy(*family)?;
                    let counters = telemetry.get(family).copied().unwrap_or_default();
                    Some(json!({
                        "family": family.as_str(),
                        "residency": match policy.residency {
                            crate::mcp::server_cache::RuntimeCacheResidency::ProcessWide => "process_wide",
                            crate::mcp::server_cache::RuntimeCacheResidency::RequestLocal => "request_local",
                        },
                        "reuse_class": match policy.reuse_class {
                            crate::mcp::server_cache::RuntimeCacheReuseClass::SnapshotScopedReusable => "snapshot_scoped_reusable",
                            crate::mcp::server_cache::RuntimeCacheReuseClass::QueryResultMicroCache => "query_result_micro_cache",
                            crate::mcp::server_cache::RuntimeCacheReuseClass::ProcessMetadata => "process_metadata",
                            crate::mcp::server_cache::RuntimeCacheReuseClass::RequestLocalOnly => "request_local_only",
                            crate::mcp::server_cache::RuntimeCacheReuseClass::DeferredUntilReadOnly => "deferred_until_read_only",
                        },
                        "freshness_contract": match policy.freshness_contract {
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::RepositorySnapshot => "repository_snapshot",
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::RepositoryFreshnessScopes => "repository_freshness_scopes",
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::RepositoryId => "repository_id",
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::ExactInput => "exact_input",
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::RequestLocal => "request_local",
                        },
                        "budget": {
                            "max_entries": policy.budget.max_entries,
                            "max_bytes": policy.budget.max_bytes,
                        },
                        "dirty_root_bypass": policy.dirty_root_bypass,
                        "telemetry": {
                            "hits": counters.hits,
                            "misses": counters.misses,
                            "bypasses": counters.bypasses,
                            "inserts": counters.inserts,
                            "evictions": counters.evictions,
                            "invalidations": counters.invalidations,
                        },
                    }))
                })
                .collect::<Vec<_>>(),
        )
    }

    #[cfg(test)]
    fn runtime_cache_telemetry(&self, family: RuntimeCacheFamily) -> RuntimeCacheTelemetry {
        self.runtime_state
            .runtime_cache_telemetry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&family)
            .copied()
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn runtime_cache_policy(
        &self,
        family: RuntimeCacheFamily,
    ) -> crate::mcp::server_cache::RuntimeCacheFamilyPolicy {
        *self
            .runtime_state
            .runtime_cache_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .policy(family)
            .expect("runtime cache family policy should exist")
    }

    pub fn set_watch_runtime(&self, watch_runtime: Option<Arc<crate::watch::WatchRuntime>>) {
        let mut state = self
            .runtime_state
            .watch_runtime
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *state = watch_runtime;
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

    fn prewarm_precise_graph_for_workspace(
        &self,
        workspace: &AttachedWorkspace,
    ) -> Result<(), String> {
        let discovery = Self::collect_scip_artifact_digests(&workspace.root);
        if discovery.artifact_digests.is_empty() {
            return Ok(());
        }
        if self
            .try_reuse_latest_precise_graph_for_repository(
                &workspace.repository_id,
                &workspace.root,
            )
            .is_some()
        {
            return Ok(());
        }

        self.precise_graph_for_repository_root(
            &workspace.repository_id,
            &workspace.root,
            self.find_references_resource_budgets(),
        )
        .map(|_| ())
        .map_err(|err| err.message.to_string())
    }

    fn runtime_status_workspace(&self) -> Option<AttachedWorkspace> {
        self.current_workspace().or_else(|| {
            self.attached_workspaces()
                .into_iter()
                .min_by(|left, right| left.repository_id.cmp(&right.repository_id))
                .or_else(|| {
                    self.known_workspaces()
                        .into_iter()
                        .min_by(|left, right| left.repository_id.cmp(&right.repository_id))
                })
        })
    }

    fn runtime_recent_provenance_repository_id(payload_json: &str) -> Option<String> {
        let payload = serde_json::from_str::<Value>(payload_json).ok()?;
        payload
            .get("target_repository_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                payload
                    .get("source_refs")
                    .and_then(|source_refs| source_refs.get("repository_id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| {
                payload
                    .get("source_refs")
                    .and_then(|source_refs| source_refs.get("repository_ids"))
                    .and_then(Value::as_array)
                    .and_then(|ids| ids.first())
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
    }

    fn runtime_recent_provenance_summaries(&self) -> Vec<RecentProvenanceSummary> {
        let Some(workspace) = self.runtime_status_workspace() else {
            return Vec::new();
        };
        let storage = Storage::new(&workspace.db_path);
        match storage.load_recent_provenance_events(Self::RUNTIME_RECENT_PROVENANCE_LIMIT) {
            Ok(rows) => rows
                .into_iter()
                .map(|row| RecentProvenanceSummary {
                    trace_id: row.trace_id,
                    tool_name: row.tool_name,
                    created_at: row.created_at,
                    repository_id: Self::runtime_recent_provenance_repository_id(&row.payload_json),
                })
                .collect(),
            Err(err) => {
                warn!(
                    repository_id = workspace.repository_id,
                    error = %err,
                    "failed to load recent runtime provenance summaries"
                );
                Vec::new()
            }
        }
    }

    fn runtime_status_summary(&self) -> RuntimeStatusSummary {
        let (active_tasks, recent_tasks) = {
            let registry = self
                .runtime_state
                .runtime_task_registry
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (registry.active_tasks(), registry.recent_tasks())
        };

        RuntimeStatusSummary {
            profile: self.runtime_state.runtime_profile,
            persistent_state_available: self
                .runtime_state
                .runtime_profile
                .persistent_state_available(),
            watch_active: self.runtime_state.runtime_watch_active,
            tool_surface_profile: self.tool_surface_profile.as_str().to_owned(),
            status_tool: "workspace_current".to_owned(),
            active_tasks,
            recent_tasks,
            recent_provenance: self.runtime_recent_provenance_summaries(),
        }
    }

    fn resolve_workspace_target(
        &self,
        path: Option<&str>,
        repository_id: Option<&str>,
        resolve_mode: WorkspaceResolveMode,
    ) -> Result<
        (
            AttachedWorkspace,
            Option<String>,
            Option<WorkspaceResolveMode>,
        ),
        ErrorData,
    > {
        match (path, repository_id) {
            (Some(path), None) => {
                if path.trim().is_empty() {
                    return Err(Self::invalid_params(
                        "workspace_attach.path must not be empty",
                        None,
                    ));
                }
                let path = Path::new(path);
                let resolved_from = Self::effective_attach_directory(path)?;
                let (root, resolution) = match resolve_mode {
                    WorkspaceResolveMode::GitRoot => match Self::find_git_root(&resolved_from) {
                        Some(git_root) => (git_root, WorkspaceResolveMode::GitRoot),
                        None => (resolved_from.clone(), WorkspaceResolveMode::Direct),
                    },
                    WorkspaceResolveMode::Direct => {
                        (resolved_from.clone(), WorkspaceResolveMode::Direct)
                    }
                };
                let workspace = {
                    let mut registry = self
                        .runtime_state
                        .workspace_registry
                        .write()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    registry.get_or_insert(root)
                };
                Ok((
                    workspace,
                    Some(resolved_from.display().to_string()),
                    Some(resolution),
                ))
            }
            (None, Some(repository_id)) => {
                let workspace = self
                    .runtime_state
                    .workspace_registry
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .workspace_by_repository_id(repository_id)
                    .ok_or_else(|| {
                        Self::resource_not_found(
                            "repository_id not found",
                            Some(json!({ "repository_id": repository_id })),
                        )
                    })?;
                Ok((workspace, None, None))
            }
            (Some(_), Some(_)) => Err(Self::invalid_params(
                "workspace target must provide either `path` or `repository_id`, not both",
                None,
            )),
            (None, None) => Err(Self::invalid_params(
                "workspace target requires either `path` or `repository_id`",
                None,
            )),
        }
    }

    fn attach_workspace_target_internal(
        &self,
        path: Option<&str>,
        repository_id: Option<&str>,
        set_default: bool,
        resolve_mode: WorkspaceResolveMode,
    ) -> Result<WorkspaceAttachResponse, ErrorData> {
        let (workspace, resolved_from, resolution) =
            self.resolve_workspace_target(path, repository_id, resolve_mode)?;

        let newly_adopted = self.adopt_workspace(&workspace, set_default)?;

        self.runtime_state
            .validated_manifest_candidate_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .invalidate_root(&workspace.root);
        self.invalidate_repository_summary_cache(&workspace.repository_id);
        self.invalidate_repository_file_content_cache(&workspace.repository_id);
        self.scip_invalidate_repository_precise_generation_cache(&workspace.repository_id);
        self.invalidate_repository_precise_graph_caches(&workspace.repository_id);
        self.invalidate_repository_search_response_caches(&workspace.repository_id);
        self.invalidate_repository_navigation_response_caches(&workspace.repository_id);
        self.maybe_refresh_workspace_semantic_snapshot(&workspace);

        let mut repository = self.repository_summary(&workspace);
        let storage = repository
            .storage
            .clone()
            .unwrap_or_else(|| Self::workspace_storage_summary(&workspace));
        repository.storage = None;
        self.maybe_spawn_workspace_runtime_prewarm(&workspace);
        let precise_generation_action =
            self.maybe_spawn_workspace_precise_generation_for_paths(&workspace, &[], &[]);
        let precise = self
            .workspace_precise_summary_for_workspace(&workspace, Some(precise_generation_action));

        Ok(WorkspaceAttachResponse {
            repository,
            resolved_from: resolved_from.unwrap_or_else(|| workspace.root.display().to_string()),
            resolution: resolution.unwrap_or(WorkspaceResolveMode::Direct),
            session_default: self.current_repository_id().as_deref()
                == Some(workspace.repository_id.as_str()),
            storage,
            action: if newly_adopted {
                WorkspaceAttachAction::AttachedFresh
            } else {
                WorkspaceAttachAction::ReusedWorkspace
            },
            precise,
        })
    }

    #[cfg(test)]
    fn attach_workspace_internal(
        &self,
        path: &Path,
        set_default: bool,
        resolve_mode: WorkspaceResolveMode,
    ) -> Result<WorkspaceAttachResponse, ErrorData> {
        let owned_path = path.display().to_string();
        self.attach_workspace_target_internal(Some(&owned_path), None, set_default, resolve_mode)
    }

    fn repository_has_active_runtime_work(&self, repository_id: &str) -> bool {
        let registry = self
            .runtime_state
            .runtime_task_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        [
            RuntimeTaskKind::ChangedReindex,
            RuntimeTaskKind::SemanticRefresh,
            RuntimeTaskKind::PreciseGenerate,
            RuntimeTaskKind::WorkspacePrepare,
            RuntimeTaskKind::WorkspaceReindex,
        ]
        .into_iter()
        .any(|kind| registry.has_active_task_for_repository(kind, repository_id))
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
        let roots = if requested.is_absolute() && params.repository_id.is_none() {
            self.known_workspaces()
                .into_iter()
                .map(|workspace| (workspace.repository_id, workspace.root))
                .collect::<Vec<_>>()
        } else {
            self.roots_for_repository(params.repository_id.as_deref())?
        }
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
        let execution_context = self.read_only_tool_execution_context("list_repositories", None);
        let execution_context_for_blocking = execution_context.clone();
        let server = self.clone();
        let (result, provenance_result) = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let repositories = server
                    .known_workspaces()
                    .into_iter()
                    .map(|workspace| server.repository_summary(&workspace))
                    .collect::<Vec<_>>();
                let repository_ids = repositories
                    .iter()
                    .map(|repo| repo.repository_id.clone())
                    .collect::<Vec<_>>();

                let response = ListRepositoriesResponse { repositories };
                let finalization = server.tool_execution_finalization(
                    json!({
                        "repository_ids": repository_ids,
                    }),
                    Some(
                        execution_context_for_blocking.normalized_workload(
                            &response
                                .repositories
                                .iter()
                                .map(|repo| repo.repository_id.clone())
                                .collect::<Vec<_>>(),
                            WorkloadPrecisionMode::Exact,
                        ),
                    ),
                );
                let result = Ok(Json(response));
                let provenance_result = server.record_provenance_with_outcome(
                    "list_repositories",
                    None,
                    json!({}),
                    finalization.source_refs,
                    Self::provenance_outcome(&result),
                );
                (result, provenance_result)
            })
            .await?;
        self.finalize_read_only_tool(&execution_context, result, provenance_result)
    }

    #[tool(
        name = "workspace_attach",
        description = "Explicitly attach a workspace for this session and optionally set it as the session default repository.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn workspace_attach(
        &self,
        params: Parameters<WorkspaceAttachParams>,
    ) -> Result<Json<WorkspaceAttachResponse>, ErrorData> {
        let params = params.0;
        let set_default = params.set_default.unwrap_or(true);
        let resolve_mode = params.resolve_mode.unwrap_or(WorkspaceResolveMode::GitRoot);
        let response = self.attach_workspace_target_internal(
            params.path.as_deref(),
            params.repository_id.as_deref(),
            set_default,
            resolve_mode,
        )?;
        let finalization = self.tool_execution_finalization(
            json!({
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
            }),
            Some(FriggMcpServer::provenance_normalized_workload_metadata(
                "workspace_attach",
                std::slice::from_ref(&response.repository.repository_id),
                WorkloadPrecisionMode::Exact,
                None,
                None,
                None,
            )),
        );
        let result = Ok(Json(response));
        let provenance_result = self
            .record_provenance_blocking(
                "workspace_attach",
                None,
                json!({
                    "path": params.path.as_deref().map(Self::bounded_text),
                    "repository_id": params.repository_id,
                    "set_default": params.set_default,
                    "resolve_mode": params.resolve_mode,
                }),
                finalization.source_refs,
                &result,
            )
            .await;
        self.finalize_with_provenance("workspace_attach", result, provenance_result)
    }

    #[tool(
        name = "workspace_detach",
        description = "Detach a repository from this session and release its session-local default and watch lease when applicable.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn workspace_detach(
        &self,
        params: Parameters<WorkspaceDetachParams>,
    ) -> Result<Json<WorkspaceDetachResponse>, ErrorData> {
        let params = params.0;
        let repository_id = params
            .repository_id
            .or_else(|| self.current_repository_id())
            .ok_or_else(|| Self::no_attached_workspaces_error("workspace_detach"))?;
        let detached = self.detach_workspace(&repository_id)?;
        let Some(workspace) = detached else {
            return Err(Self::resource_not_found(
                "repository_id is not adopted in this session",
                Some(json!({ "repository_id": repository_id })),
            ));
        };
        self.invalidate_repository_summary_cache(&workspace.repository_id);
        self.invalidate_repository_file_content_cache(&workspace.repository_id);
        self.scip_invalidate_repository_precise_generation_cache(&workspace.repository_id);
        self.invalidate_repository_precise_graph_caches(&workspace.repository_id);
        let response = WorkspaceDetachResponse {
            repository_id: workspace.repository_id.clone(),
            session_default: self.current_repository_id().as_deref()
                == Some(workspace.repository_id.as_str()),
            detached: true,
        };
        let finalization = self.tool_execution_finalization(
            json!({
                "repository_id": response.repository_id,
                "detached": response.detached,
                "session_default": response.session_default,
            }),
            Some(FriggMcpServer::provenance_normalized_workload_metadata(
                "workspace_detach",
                std::slice::from_ref(&workspace.repository_id),
                WorkloadPrecisionMode::Exact,
                None,
                None,
                None,
            )),
        );
        let result = Ok(Json(response));
        let provenance_result = self
            .record_provenance_blocking(
                "workspace_detach",
                None,
                json!({ "repository_id": repository_id }),
                finalization.source_refs,
                &result,
            )
            .await;
        self.finalize_with_provenance("workspace_detach", result, provenance_result)
    }

    #[tool(
        name = "workspace_prepare",
        description = "Initialize and verify repo-local Frigg storage for a target repository, then adopt it for this session.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn workspace_prepare(
        &self,
        meta: Meta,
        client: Peer<RoleServer>,
        params: Parameters<WorkspacePrepareParams>,
    ) -> Result<Json<WorkspacePrepareResponse>, ErrorData> {
        let params = params.0;
        Self::require_confirm("workspace_prepare", params.confirm)?;
        let set_default = params.set_default.unwrap_or(true);
        let resolve_mode = params.resolve_mode.unwrap_or(WorkspaceResolveMode::GitRoot);
        let (workspace, resolved_from, resolution) = self.resolve_workspace_target(
            params.path.as_deref(),
            params.repository_id.as_deref(),
            resolve_mode,
        )?;
        if self.repository_has_active_runtime_work(&workspace.repository_id) {
            return Err(Self::invalid_params(
                "repository already has active runtime work",
                Some(json!({ "repository_id": workspace.repository_id })),
            ));
        }

        Self::notify_progress(&meta, &client, 0.0, 4.0, "resolve target").await;
        let task_id = self
            .runtime_state
            .runtime_task_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .start_task(
                RuntimeTaskKind::WorkspacePrepare,
                workspace.repository_id.clone(),
                "workspace_prepare",
                Some(format!("prepare {}", workspace.root.display())),
            );

        Self::notify_progress(&meta, &client, 1.0, 4.0, "initialize storage").await;
        let prepared_storage = Self::run_blocking_task("workspace_prepare", {
            let workspace = workspace.clone();
            move || -> Result<WorkspaceStorageSummary, String> {
                let db_path = ensure_provenance_db_parent_dir(&workspace.root)
                    .map_err(|err| err.to_string())?;
                let storage = Storage::new(&db_path);
                storage.initialize().map_err(|err| err.to_string())?;
                storage.verify().map_err(|err| err.to_string())?;
                Ok(FriggMcpServer::workspace_storage_summary(&workspace))
            }
        })
        .await?
        .map_err(|err| {
            self.runtime_state
                .runtime_task_registry
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .finish_task(&task_id, RuntimeTaskStatus::Failed, Some(err.clone()));
            Self::internal(
                err,
                Some(json!({ "repository_id": workspace.repository_id })),
            )
        })?;

        Self::notify_progress(&meta, &client, 2.0, 4.0, "verify storage").await;
        self.runtime_state
            .validated_manifest_candidate_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .invalidate_root(&workspace.root);
        self.invalidate_repository_summary_cache(&workspace.repository_id);
        self.invalidate_repository_file_content_cache(&workspace.repository_id);
        self.scip_invalidate_repository_precise_generation_cache(&workspace.repository_id);
        self.invalidate_repository_precise_graph_caches(&workspace.repository_id);
        self.invalidate_repository_search_response_caches(&workspace.repository_id);
        self.invalidate_repository_navigation_response_caches(&workspace.repository_id);

        Self::notify_progress(&meta, &client, 3.0, 4.0, "activate watcher lease").await;
        self.adopt_workspace(&workspace, set_default)
            .inspect_err(|error| {
                self.runtime_state
                    .runtime_task_registry
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .finish_task(
                        &task_id,
                        RuntimeTaskStatus::Failed,
                        Some(error.message.to_string()),
                    );
            })?;
        self.maybe_spawn_workspace_runtime_prewarm(&workspace);
        let _ = self.maybe_spawn_workspace_precise_generation_for_paths(&workspace, &[], &[]);

        let mut repository = self.repository_summary(&workspace);
        repository.storage = None;
        let response = WorkspacePrepareResponse {
            repository,
            resolved_from,
            resolution,
            session_default: self.current_repository_id().as_deref()
                == Some(workspace.repository_id.as_str()),
            storage: prepared_storage,
        };
        self.runtime_state
            .runtime_task_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .finish_task(&task_id, RuntimeTaskStatus::Succeeded, None);
        Self::notify_progress(&meta, &client, 4.0, 4.0, "finalize").await;

        let finalization = self.tool_execution_finalization(
            json!({
                "repository_id": response.repository.repository_id.clone(),
                "resolved_from": response.resolved_from,
                "resolution": response.resolution,
                "session_default": response.session_default,
                "storage": {
                    "db_path": response.storage.db_path.clone(),
                    "exists": response.storage.exists,
                    "initialized": response.storage.initialized,
                    "index_state": response.storage.index_state,
                },
            }),
            Some(FriggMcpServer::provenance_normalized_workload_metadata(
                "workspace_prepare",
                std::slice::from_ref(&response.repository.repository_id),
                WorkloadPrecisionMode::Exact,
                None,
                None,
                None,
            )),
        );
        let result = Ok(Json(response));
        let provenance_result = self
            .record_provenance_blocking(
                "workspace_prepare",
                None,
                json!({
                    "path": params.path.as_deref().map(Self::bounded_text),
                    "repository_id": params.repository_id,
                    "set_default": params.set_default,
                    "resolve_mode": params.resolve_mode,
                    "confirm": params.confirm,
                }),
                finalization.source_refs,
                &result,
            )
            .await;
        self.finalize_with_provenance("workspace_prepare", result, provenance_result)
    }

    #[tool(
        name = "workspace_reindex",
        description = "Run a changed-only foreground reindex for a target repository, bootstrapping initial state when needed, then adopt it for this session.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn workspace_reindex(
        &self,
        meta: Meta,
        client: Peer<RoleServer>,
        params: Parameters<WorkspaceReindexParams>,
    ) -> Result<Json<WorkspaceReindexResponse>, ErrorData> {
        let params = params.0;
        Self::require_confirm("workspace_reindex", params.confirm)?;
        let set_default = params.set_default.unwrap_or(true);
        let resolve_mode = params.resolve_mode.unwrap_or(WorkspaceResolveMode::GitRoot);
        let (workspace, resolved_from, resolution) = self.resolve_workspace_target(
            params.path.as_deref(),
            params.repository_id.as_deref(),
            resolve_mode,
        )?;
        if self.repository_has_active_runtime_work(&workspace.repository_id) {
            return Err(Self::invalid_params(
                "repository already has active runtime work",
                Some(json!({ "repository_id": workspace.repository_id })),
            ));
        }

        let task_id = self
            .runtime_state
            .runtime_task_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .start_task(
                RuntimeTaskKind::WorkspaceReindex,
                workspace.repository_id.clone(),
                "workspace_reindex",
                Some(format!("reindex {}", workspace.root.display())),
            );
        Self::notify_progress(&meta, &client, 0.0, 4.0, "resolve target").await;
        Self::notify_progress(&meta, &client, 1.0, 4.0, "lexical refresh").await;
        let semantic_runtime = self.config.semantic_runtime.clone();
        let reindex_summary = Self::run_blocking_task("workspace_reindex", {
            let workspace = workspace.clone();
            move || -> Result<crate::indexer::ReindexSummary, String> {
                let db_path = ensure_provenance_db_parent_dir(&workspace.root)
                    .map_err(|err| err.to_string())?;
                let credentials = SemanticRuntimeCredentials::from_process_env();
                reindex_repository_with_runtime_config(
                    &workspace.repository_id,
                    &workspace.root,
                    &db_path,
                    ReindexMode::ChangedOnly,
                    &semantic_runtime,
                    &credentials,
                )
                .map_err(|err| err.to_string())
            }
        })
        .await?
        .map_err(|err| {
            self.runtime_state
                .runtime_task_registry
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .finish_task(&task_id, RuntimeTaskStatus::Failed, Some(err.clone()));
            Self::internal(
                err,
                Some(json!({ "repository_id": workspace.repository_id })),
            )
        })?;

        Self::notify_progress(&meta, &client, 2.0, 4.0, "semantic refresh").await;
        self.runtime_state
            .validated_manifest_candidate_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .invalidate_root(&workspace.root);
        self.invalidate_repository_summary_cache(&workspace.repository_id);
        self.invalidate_repository_file_content_cache(&workspace.repository_id);
        self.scip_invalidate_repository_precise_generation_cache(&workspace.repository_id);
        self.invalidate_repository_precise_graph_caches(&workspace.repository_id);
        self.invalidate_repository_search_response_caches(&workspace.repository_id);
        self.invalidate_repository_navigation_response_caches(&workspace.repository_id);

        Self::notify_progress(&meta, &client, 3.0, 4.0, "finalize").await;
        self.adopt_workspace(&workspace, set_default)
            .inspect_err(|error| {
                self.runtime_state
                    .runtime_task_registry
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .finish_task(
                        &task_id,
                        RuntimeTaskStatus::Failed,
                        Some(error.message.to_string()),
                    );
            })?;
        let mut repository = self.repository_summary(&workspace);
        let storage = repository
            .storage
            .clone()
            .unwrap_or_else(|| Self::workspace_storage_summary(&workspace));
        repository.storage = None;
        let response = WorkspaceReindexResponse {
            repository,
            resolved_from,
            resolution,
            session_default: self.current_repository_id().as_deref()
                == Some(workspace.repository_id.as_str()),
            storage,
            snapshot_id: reindex_summary.snapshot_id.clone(),
            files_scanned: reindex_summary.files_scanned,
            files_changed: reindex_summary.files_changed,
            files_deleted: reindex_summary.files_deleted,
            diagnostics_count: reindex_summary.diagnostics.total_count(),
        };
        let _ = self.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &reindex_summary.changed_paths,
            &reindex_summary.deleted_paths,
        );
        self.runtime_state
            .runtime_task_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .finish_task(&task_id, RuntimeTaskStatus::Succeeded, None);
        Self::notify_progress(&meta, &client, 4.0, 4.0, "done").await;

        let finalization = self.tool_execution_finalization(
            json!({
                "repository_id": response.repository.repository_id.clone(),
                "snapshot_id": response.snapshot_id,
                "files_scanned": response.files_scanned,
                "files_changed": response.files_changed,
                "files_deleted": response.files_deleted,
                "diagnostics_count": response.diagnostics_count,
                "session_default": response.session_default,
            }),
            Some(FriggMcpServer::provenance_normalized_workload_metadata(
                "workspace_reindex",
                std::slice::from_ref(&response.repository.repository_id),
                WorkloadPrecisionMode::Exact,
                None,
                None,
                None,
            )),
        );
        let result = Ok(Json(response));
        let provenance_result = self
            .record_provenance_blocking(
                "workspace_reindex",
                None,
                json!({
                    "path": params.path.as_deref().map(Self::bounded_text),
                    "repository_id": params.repository_id,
                    "set_default": params.set_default,
                    "resolve_mode": params.resolve_mode,
                    "confirm": params.confirm,
                }),
                finalization.source_refs,
                &result,
            )
            .await;
        self.finalize_with_provenance("workspace_reindex", result, provenance_result)
    }

    #[tool(
        name = "workspace_current",
        description = "Return the current session repository, attached repositories, and runtime status, if any.",
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
        let execution_context = self.read_only_tool_execution_context("workspace_current", None);
        let current_workspace = self.current_workspace();
        let repositories = self
            .attached_workspaces()
            .into_iter()
            .map(|workspace| self.repository_summary(&workspace))
            .collect::<Vec<_>>();
        let runtime = self.runtime_status_summary();
        let precise = current_workspace
            .as_ref()
            .map(|workspace| self.workspace_precise_summary_for_workspace(workspace, None));
        let response = WorkspaceCurrentResponse {
            repository: current_workspace
                .as_ref()
                .map(|workspace| self.repository_summary(workspace)),
            session_default: current_workspace.is_some(),
            repositories,
            precise,
            runtime: Some(runtime),
        };
        let repository_ids = response
            .repositories
            .iter()
            .map(|repository| repository.repository_id.clone())
            .collect::<Vec<_>>();
        let source_refs = json!({
            "repository_id": response
                .repository
                .as_ref()
                .map(|repository| repository.repository_id.clone()),
            "repository_ids": repository_ids,
            "runtime_profile": response
                .runtime
                .as_ref()
                .map(|runtime| runtime.profile.as_str().to_owned()),
            "watch_active": response.runtime.as_ref().map(|runtime| runtime.watch_active),
            "active_task_count": response
                .runtime
                .as_ref()
                .map(|runtime| runtime.active_tasks.len()),
            "recent_task_count": response
                .runtime
                .as_ref()
                .map(|runtime| runtime.recent_tasks.len()),
            "recent_provenance_count": response
                .runtime
                .as_ref()
                .map(|runtime| runtime.recent_provenance.len()),
        });
        let normalized_workload =
            execution_context.normalized_workload(&repository_ids, WorkloadPrecisionMode::Exact);
        let finalization = self.tool_execution_finalization(source_refs, Some(normalized_workload));
        let result = Ok(Json(response));
        let provenance_result = self
            .record_provenance_blocking_with_metadata(
                "workspace_current",
                None,
                json!({}),
                finalization.source_refs,
                finalization.normalized_workload,
                &result,
            )
            .await;
        self.finalize_read_only_tool(&execution_context, result, provenance_result)
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
        self.read_file_impl(params.0).await
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
        self.explore_impl(params.0).await
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
        self.search_text_impl(params.0).await
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
        self.search_hybrid_impl(params.0).await
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
        self.search_symbol_impl(params.0).await
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
        self.find_references_impl(params.0).await
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
        self.go_to_definition_impl(params.0).await
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
        self.find_declarations_impl(params.0).await
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
        self.find_implementations_impl(params.0).await
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
        self.incoming_calls_impl(params.0).await
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
        self.outgoing_calls_impl(params.0).await
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
        self.document_symbols_impl(params.0).await
    }

    #[tool(
        name = "inspect_syntax_tree",
        description = "Inspect the tree-sitter node stack around a source file location.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn inspect_syntax_tree(
        &self,
        params: Parameters<InspectSyntaxTreeParams>,
    ) -> Result<Json<InspectSyntaxTreeResponse>, ErrorData> {
        self.inspect_syntax_tree_impl(params.0).await
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
        self.search_structural_impl(params.0).await
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
        self.deep_search_run_impl(params.0.into()).await
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
        self.deep_search_replay_impl(params.0).await
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
        self.deep_search_compose_citations_impl(params.0).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for FriggMcpServer {
    fn get_info(&self) -> ServerInfo {
        let tool_surface_profile = self.tool_surface_profile.as_str();
        let runtime_profile = self.runtime_state.runtime_profile.as_str();
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_prompts()
                .enable_resources()
                .enable_tools()
                .build(),
        )
            .with_server_info(
                Implementation::new("frigg", env!("CARGO_PKG_VERSION"))
                    .with_title("Frigg Deep Search MCP")
                    .with_description(
                        "Local-first deterministic code search + navigation MCP server",
                    ),
            )
            .with_instructions(
                format!(
                    "Use list_repositories first; if no repository is attached or you want a session-local default repo, call workspace_attach explicitly. Frigg no longer auto-attaches the current directory or MCP-declared client roots for empty sessions, so local storage and provenance stay opt-in. Runtime tool-surface profile is `{tool_surface_profile}` (set `{TOOL_SURFACE_PROFILE_ENV}=extended` to include explore plus deep-search tools). Runtime profile is `{runtime_profile}`; call workspace_current to inspect attached repositories, watch/index health, active or recent runtime tasks, and recent provenance summaries. For simple local file reads or literal scans in the checked-out workspace, shell tools may be faster than read_file or search_text. Use search_hybrid for broad doc/runtime questions, then pivot to search_symbol or scoped search_text.path_regex for concrete anchors. Use explore after discovery when you want bounded single-artifact probe/zoom/refine follow-up. Use read_file to confirm exact source when repository-aware evidence is useful, and treat search_hybrid warnings or non-`ok` semantic_status as weaker evidence. Policy resources are available at `{SUPPORT_MATRIX_RESOURCE_URI}`, `{TOOL_SURFACE_RESOURCE_URI}`, and `{SHELL_GUIDANCE_RESOURCE_URI}`. Prompt guidance is available via `{ROUTING_GUIDE_PROMPT_NAME}`."
                ),
            )
    }

    async fn on_initialized(&self, _context: rmcp::service::NotificationContext<rmcp::RoleServer>) {
    }

    async fn on_roots_list_changed(
        &self,
        _context: rmcp::service::NotificationContext<rmcp::RoleServer>,
    ) {
    }

    async fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListResourcesResult, ErrorData> {
        Ok(rmcp::model::ListResourcesResult::with_all_items(
            policy_resources(),
        ))
    }

    async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ReadResourceResult, ErrorData> {
        read_policy_resource(&request.uri, self.tool_surface_profile).ok_or_else(|| {
            Self::resource_not_found(
                format!("unknown resource `{}`", request.uri),
                Some(json!({ "uri": request.uri })),
            )
        })
    }

    async fn list_prompts(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListPromptsResult, ErrorData> {
        Ok(rmcp::model::ListPromptsResult::with_all_items(
            guidance_prompts(),
        ))
    }

    async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::GetPromptResult, ErrorData> {
        read_guidance_prompt(
            &request.name,
            request.arguments.as_ref(),
            self.tool_surface_profile,
        )
        .ok_or_else(|| {
            Self::invalid_params(
                format!("unknown prompt `{}`", request.name),
                Some(json!({ "name": request.name })),
            )
        })
    }
}

#[cfg(test)]
mod runtime_gate_tests;
