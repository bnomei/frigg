//! MCP server orchestration: tool routing, session-scoped workspace adoption, runtime caches,
//! and the public tool handlers agents invoke over streamable HTTP or stdio.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::domain::model::{GeneratedStructuralFollowUp, ReferenceMatch, SymbolMatch};
use crate::domain::{
    ChannelResult, EvidenceChannel, FriggError, FriggResult, WorkloadPrecisionMode,
};
use crate::graph::{
    PreciseRelationshipKind, RelationKind, ScipIngestError, ScipResourceBudgets, SymbolGraph,
};
use crate::indexer::{
    FileMetadataDigest, HeuristicReference, HeuristicReferenceConfidence,
    HeuristicReferenceEvidence, HeuristicReferenceResolver, IndexMode, ManifestBuilder,
    ManifestDiagnosticKind, SourceSpan, SymbolDefinition, SymbolExtractionOutput,
    byte_offset_for_line_column, extract_php_source_evidence_from_source,
    extract_symbols_for_paths, extract_symbols_from_source,
    generated_follow_up_structural_at_location_in_source, index_repository_with_runtime_config,
    inspect_syntax_tree_in_source, inspect_syntax_tree_with_follow_up_in_source,
    navigation_symbol_target_rank, php_declaration_relation_edges_for_file,
    php_heuristic_implementation_candidates_for_target, register_symbol_definitions,
    resolve_php_target_evidence_edges, search_structural_in_source,
    search_structural_with_follow_up_in_source,
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
    CallToolResult, ContentBlock, Implementation, Meta, ServerCapabilities, ServerInfo,
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
use tracing::{info, warn};
use uuid::Uuid;

use crate::agent_directive;
#[cfg(feature = "playbook")]
use crate::mcp::advanced::deep_search::{
    DeepSearchHarness, DeepSearchPlaybook, DeepSearchTraceArtifact, DeepSearchTraceOutcome,
};
use crate::mcp::explorer::{
    DEFAULT_CONTEXT_LINES, DEFAULT_MAX_MATCHES, ExploreMatcher, ExploreScopeRequest,
    LossyLineSliceError, MAX_CONTEXT_LINES, line_window_around_anchor, validate_anchor,
    validate_cursor,
};
use crate::mcp::guidance::{
    ROUTING_GUIDE_PROMPT_NAME, ROUTING_STATS_RESOURCE_URI, SHELL_GUIDANCE_RESOURCE_URI,
    SHELL_REPLACEMENT_MAP_RESOURCE_URI, SUPPORT_MATRIX_RESOURCE_URI, TOOL_SURFACE_RESOURCE_URI,
    guidance_prompts, policy_resources, read_guidance_prompt, read_policy_resource,
};
use crate::mcp::server_cache::{
    CachedHeuristicReferences, CachedPreciseGeneratorProbe, CachedRepositoryResponseFreshness,
    CachedWorkspacePreciseGeneration, FileContentSnapshot, HeuristicReferenceCacheKey,
    PreciseGeneratorProbeCacheKey, RepositoryFreshnessCacheScope, RepositoryResponseCacheFreshness,
    RepositoryResponseCacheFreshnessMode, RepositoryResponseFreshnessCacheKey, RuntimeCacheBudget,
    RuntimeCacheEvent, RuntimeCacheFamily, RuntimeCacheRegistry, RuntimeCacheTelemetry,
    SessionResultHandleCache, SessionResultHandleEntry, WorkspaceSemanticRefreshPlan,
};
use crate::mcp::server_state::{
    CachedPreciseGraph, DeterministicSignatureHasher, DisambiguationRequiredSymbolTarget,
    ExploreExecution, FindReferencesExecution, FindReferencesResourceBudgets,
    NavigationTargetSelection, PreciseArtifactFailureSample, PreciseCoverageMode,
    PreciseGraphCacheKey, PreciseIngestStats, RankedSymbolMatch, ReadFileExecution,
    RepositoryDiagnosticsSummary, RepositorySymbolCorpus, ResolvedNavigationTarget,
    ResolvedSymbolTarget, RuntimeTaskGuard, RuntimeTaskRegistry, ScipArtifactDigest,
    ScipArtifactDiscovery, ScipArtifactFormat, ScipCandidateDirectoryDigest, SearchHybridExecution,
    SearchSymbolExecution, SearchTextExecution, SymbolCandidate, SymbolCorpusCacheKey,
};
use crate::mcp::tool_surface::{
    TOOL_SURFACE_PROFILE_ENV, ToolSurfaceParityDiff, ToolSurfaceProfile,
    active_runtime_tool_surface_profile, diff_runtime_against_profile_manifest,
    manifest_for_tool_surface_profile,
};
use crate::mcp::types::{
    CallHierarchyMatch, DocumentSymbolsParams, DocumentSymbolsResponse, ExploreMatch,
    ExploreMetadata, ExploreOperation, ExploreParams, ExploreResponse, ExploreWindow,
    FindDeclarationsParams, FindDeclarationsResponse, FindImplementationsParams,
    FindImplementationsResponse, FindReferencesParams, FindReferencesResponse,
    GoToDefinitionParams, GoToDefinitionResponse, ImpactBundleParams, ImpactBundleResponse,
    ImplementationMatch, IncomingCallsParams,
    IncomingCallsResponse, InspectSyntaxTreeParams, InspectSyntaxTreeResponse, ListFilesParams,
    ListFilesResponse, ListRepositoriesParams, ListRepositoriesResponse, NavigationAvailability,
    NavigationLocation, NavigationMode, NavigationTargetSelectionStatus,
    NavigationTargetSelectionSummary, OutgoingCallsParams, OutgoingCallsResponse, ReadFileParams,
    ReadFileResponse, ReadMatchParams, ReadMatchResponse, ReadPresentationMode, RecoveryFields,
    ZeroHitDiagnostics, ZeroHitInput, ZeroHitReason, ZeroHitScope,
    RepositorySummary, ResponseMode, RuntimeStatusSummary, RuntimeTaskKind, RuntimeTaskStatus,
    RuntimeTaskSummary, SearchBatchParams, SearchBatchResponse, SearchHybridChannelWeightsParams,
    SearchHybridMatch, SearchHybridParams, SearchHybridResponse, SearchPatternType,
    SearchStructuralParams, SearchStructuralResponse, SearchSymbolParams, SearchSymbolPathClass,
    SearchSymbolResponse, SearchTextParams, SearchTextResponse,
    SyntaxTreeNodeItem, WRITE_CONFIRM_PARAM, WRITE_CONFIRMATION_REQUIRED_ERROR_CODE,
    WorkspaceAttachAction, WorkspaceAttachIndexMode, WorkspaceAttachParams,
    WorkspaceAttachResponse, WorkspaceCurrentParams, WorkspaceCurrentResponse,
    WorkspaceDetachParams, WorkspaceDetachResponse, WorkspaceIndexAction,
    WorkspaceIndexComponentState, WorkspaceIndexComponentSummary, WorkspaceIndexHealthSummary,
    WorkspaceIndexLifecyclePhase, WorkspaceIndexLifecycleSummary, WorkspaceIndexParams,
    WorkspaceIndexResponse, WorkspaceParams, WorkspacePreciseArtifactFailureSummary,
    WorkspacePreciseCoverageMode, WorkspacePreciseGenerationAction,
    WorkspacePreciseGenerationStatus, WorkspacePreciseGenerationSummary,
    WorkspacePreciseGeneratorState, WorkspacePreciseGeneratorSummary, WorkspacePreciseIngestState,
    WorkspacePreciseIngestSummary, WorkspacePreciseLifecyclePhase,
    WorkspacePreciseLifecycleSummary, WorkspacePreciseState, WorkspacePreciseSummary,
    WorkspaceGateAction, WorkspacePrepareParams, WorkspacePrepareResponse,
    WorkspaceRecommendedAction, WorkspaceResolveMode, WorkspaceResponse,
    WorkspaceStorageIndexState, WorkspaceStorageSummary,
};
#[cfg(feature = "playbook")]
use crate::mcp::types::{
    PlaybookComposeCitationsParams, PlaybookComposeCitationsResponse, PlaybookReplayParams,
    PlaybookReplayResponse, PlaybookRunParams, PlaybookRunResponse,
};
use crate::mcp::workspace_registry::{AttachedWorkspace, WorkspaceRegistry};
use crate::settings::RuntimeProfile;

mod content;
mod errors;
mod execution;
mod navigation_cache;
mod navigation_metadata;
mod navigation_precise;
mod navigation_resolution;
mod navigation_tools;
#[cfg(feature = "playbook")]
mod playbook;
mod precise_graph;
mod presentation;
mod provenance;
mod runtime_cache;
mod runtime_status;
mod search_tools;
mod symbol_index;
mod workspace;
mod workspace_session;

/// Aggregate counts from building repository symbol corpora for diagnostics benchmarks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SymbolCorpusBenchmarkSummary {
    pub repository_count: usize,
    pub source_file_count: usize,
    pub symbol_count: usize,
    pub php_evidence_files: usize,
    pub blade_evidence_files: usize,
}

/// Aggregate counts from building or reusing a workspace precise graph for diagnostics benchmarks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PreciseGraphBenchmarkSummary {
    pub artifact_count: usize,
    pub artifacts_ingested: usize,
    pub artifacts_failed: usize,
    pub precise_symbol_count: usize,
    pub precise_occurrence_count: usize,
    pub precise_relationship_count: usize,
    pub reused_cache: bool,
}

/// Completion status for a display-only MCP tool call event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallDisplayStatus {
    Ok,
    Failed,
}

impl ToolCallDisplayStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
        }
    }
}

/// Display-only summary emitted when an MCP tool call completes.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallDisplayEvent {
    pub tool_name: String,
    pub duration_ms: u64,
    pub status: ToolCallDisplayStatus,
    pub context_saved_percent: Option<f64>,
    pub session_id: String,
}

/// Callback used by CLI transports to mirror completed MCP tool calls in human progress output.
pub type ToolCallDisplaySink = Arc<dyn Fn(ToolCallDisplayEvent) + Send + Sync + 'static>;

#[doc(hidden)]
pub fn benchmark_build_symbol_corpora_for_server(
    server: &FriggMcpServer,
    repository_id: Option<&str>,
) -> crate::domain::FriggResult<SymbolCorpusBenchmarkSummary> {
    let corpora = server
        .collect_repository_symbol_corpora(repository_id)
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to build symbol corpora for benchmark: {err:?}"
            ))
        })?;
    let mut summary = SymbolCorpusBenchmarkSummary {
        repository_count: corpora.len(),
        source_file_count: 0,
        symbol_count: 0,
        php_evidence_files: 0,
        blade_evidence_files: 0,
    };
    for corpus in corpora {
        summary.source_file_count = summary
            .source_file_count
            .saturating_add(corpus.source_paths.len());
        summary.symbol_count = summary.symbol_count.saturating_add(corpus.symbols.len());
        summary.php_evidence_files = summary
            .php_evidence_files
            .saturating_add(corpus.php_evidence_by_relative_path.len());
        summary.blade_evidence_files = summary
            .blade_evidence_files
            .saturating_add(corpus.blade_evidence_by_relative_path.len());
    }
    Ok(summary)
}

#[doc(hidden)]
pub fn benchmark_build_symbol_corpora(
    config: FriggConfig,
    repository_id: Option<&str>,
) -> crate::domain::FriggResult<SymbolCorpusBenchmarkSummary> {
    let server = FriggMcpServer::new(config);
    if server.attached_workspaces().is_empty() {
        let known = server.known_workspaces();
        let workspace = if let Some(repository_id) = repository_id {
            known
                .into_iter()
                .find(|workspace| workspace.repository_id == repository_id)
        } else {
            known.into_iter().next()
        }
        .ok_or_else(|| {
            FriggError::Internal(
                "failed to prepare benchmark server: no known workspace roots".to_owned(),
            )
        })?;
        server.adopt_workspace(&workspace, true).map_err(|err| {
            FriggError::Internal(format!("failed to adopt workspace for benchmark: {err:?}"))
        })?;
    }
    benchmark_build_symbol_corpora_for_server(&server, repository_id)
}

#[doc(hidden)]
pub fn benchmark_precise_graph_for_server(
    server: &FriggMcpServer,
    repository_id: &str,
) -> crate::domain::FriggResult<PreciseGraphBenchmarkSummary> {
    let mut roots = server
        .roots_for_repository(Some(repository_id))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to resolve repository roots for precise benchmark: {err:?}"
            ))
        })?;
    roots.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    let (_, root) = roots.into_iter().next().ok_or_else(|| {
        FriggError::Internal(
            "no attached workspace roots available for precise benchmark".to_owned(),
        )
    })?;
    let reused_cache = server
        .try_reuse_latest_precise_graph_for_repository(repository_id, &root)
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to check precise graph cache for benchmark: {err:?}"
            ))
        })?
        .is_some();
    let cached = server
        .precise_graph_for_repository_root(
            repository_id,
            &root,
            server.find_references_resource_budgets(),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to build precise graph for benchmark: {err:?}"
            ))
        })?;
    let counts = cached.graph.precise_counts();
    Ok(PreciseGraphBenchmarkSummary {
        artifact_count: cached.ingest_stats.artifacts_discovered,
        artifacts_ingested: cached.ingest_stats.artifacts_ingested,
        artifacts_failed: cached.ingest_stats.artifacts_failed,
        precise_symbol_count: counts.symbols,
        precise_occurrence_count: counts.occurrences,
        precise_relationship_count: counts.relationships,
        reused_cache,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::mcp::server) enum NavigationPhpHelperKind {
    Translation,
    Route,
    Config,
    Env,
}

#[derive(Debug, Clone)]
pub(in crate::mcp::server) struct NavigationLocationTokenHint {
    symbol_query: String,
    relative_path: String,
    resolution_source: &'static str,
    helper_kind: Option<NavigationPhpHelperKind>,
    rust_hint: Option<crate::languages::RustNavigationQueryHint>,
}
/// Concrete streamable HTTP service type used when Frigg is exposed over MCP transport.
pub type FriggMcpService = StreamableHttpService<FriggMcpServer, LocalSessionManager>;

type PendingPreciseDirtyPathSets = (BTreeSet<String>, BTreeSet<String>);
type PendingPreciseDirtyPathMap = Arc<RwLock<BTreeMap<String, PendingPreciseDirtyPathSets>>>;

#[derive(Clone)]
/// Orchestrates Frigg's public MCP tool surface over shared config, caches, session state,
/// provenance, and optional watch-backed refresh state.
pub struct FriggMcpServer {
    config: Arc<FriggConfig>,
    tool_router: ToolRouter<Self>,
    tool_surface_profile: ToolSurfaceProfile,
    runtime_state: FriggMcpRuntimeState,
    session_state: FriggMcpSessionState,
    cache_state: FriggMcpCacheState,
}

#[derive(Clone)]
struct FriggMcpRuntimeState {
    runtime_profile: RuntimeProfile,
    runtime_watch_active: bool,
    semantic_runtime_credentials: Option<SemanticRuntimeCredentials>,
    workspace_registry: Arc<RwLock<WorkspaceRegistry>>,
    watch_runtime: Arc<RwLock<Option<Arc<crate::watch::WatchRuntime>>>>,
    runtime_task_registry: Arc<RwLock<RuntimeTaskRegistry>>,
    validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    searcher_projection_store_service: ProjectionStoreService,
    runtime_cache_registry: Arc<RwLock<RuntimeCacheRegistry>>,
    runtime_cache_telemetry: Arc<RwLock<BTreeMap<RuntimeCacheFamily, RuntimeCacheTelemetry>>>,
    precise_generation_status_cache:
        Arc<RwLock<BTreeMap<String, CachedWorkspacePreciseGeneration>>>,
    precise_generation_pending_dirty_paths: PendingPreciseDirtyPathMap,
    tool_call_display_sink: Arc<RwLock<Option<ToolCallDisplaySink>>>,
}

#[derive(Clone)]
struct FriggMcpSessionState {
    inner: Arc<FriggMcpSessionStateInner>,
}

// Session adoption boundary: per-transport session tracks adopted repository_ids separately
// from the process-wide workspace registry and watch lease refcounts.
struct FriggMcpSessionStateInner {
    display_session_id: String,
    workspace_registry: Arc<RwLock<WorkspaceRegistry>>,
    watch_runtime: Arc<RwLock<Option<Arc<crate::watch::WatchRuntime>>>>,
    adopted_repository_ids: RwLock<BTreeSet<String>>,
    workspace_attach_states: RwLock<BTreeMap<String, WorkspaceAttachSessionState>>,
    session_default_repository_id: RwLock<Option<String>>,
    result_handles: RwLock<SessionResultHandleCache>,
}

#[derive(Debug, Default)]
struct WorkspaceAttachSessionState {
    in_flight: usize,
    completed: bool,
    rollback_requested: bool,
    rollback_previous_default_repository_id: Option<String>,
    default_restore_requested: bool,
    default_restore_previous_default_repository_id: Option<String>,
    default_confirmed: bool,
}

#[derive(Clone)]
struct FriggMcpCacheState {
    symbol_corpus_cache: Arc<RwLock<BTreeMap<SymbolCorpusCacheKey, Arc<RepositorySymbolCorpus>>>>,
    symbol_corpus_cache_epoch: Arc<AtomicU64>,
    precise_graph_cache: Arc<RwLock<BTreeMap<PreciseGraphCacheKey, Arc<CachedPreciseGraph>>>>,
    latest_precise_graph_cache: Arc<RwLock<BTreeMap<String, Arc<CachedPreciseGraph>>>>,
    repository_response_freshness_cache: Arc<
        RwLock<BTreeMap<RepositoryResponseFreshnessCacheKey, CachedRepositoryResponseFreshness>>,
    >,
    repository_response_freshness_cache_epoch: Arc<AtomicU64>,
    precise_generator_probe_cache:
        Arc<RwLock<BTreeMap<PreciseGeneratorProbeCacheKey, CachedPreciseGeneratorProbe>>>,
    heuristic_reference_cache:
        Arc<RwLock<BTreeMap<HeuristicReferenceCacheKey, CachedHeuristicReferences>>>,
    heuristic_reference_cache_epoch: Arc<AtomicU64>,
    compiled_safe_regex_cache: Arc<RwLock<BTreeMap<String, regex::Regex>>>,
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone)]
pub(super) struct ReadOnlyToolExecutionContext {
    pub(super) tool_name: &'static str,
    pub(super) repository_hint: Option<String>,
    pub(super) started_at: Instant,
    display_context_saved_percent: Arc<Mutex<Option<f64>>>,
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
    const WORKSPACE_INDEX_RESPONSE_PATH_LIMIT: usize = 50;
    const SEARCH_STRUCTURAL_MAX_QUERY_CHARS: usize = 4_096;
    const PROVENANCE_MATCH_SAMPLE_LIMIT: usize = 4;
    const REPOSITORY_RESPONSE_FRESHNESS_CACHE_MAX_ENTRIES: usize = 64;
    const PRECISE_GENERATOR_PROBE_CACHE_TTL: Duration = Duration::from_secs(30);
    const PRECISE_GENERATOR_PROBE_CACHE_MAX_ENTRIES: usize = 128;
    const SESSION_RESULT_HANDLE_TTL: Duration = Duration::from_secs(300);
    const SESSION_RESULT_HANDLE_MAX_ENTRIES: usize = 64;
    pub fn new(config: FriggConfig) -> Self {
        let enable_extended_tools =
            active_runtime_tool_surface_profile() == ToolSurfaceProfile::Extended;
        Self::new_with_runtime_options(config, enable_extended_tools)
    }

    pub fn new_with_runtime(
        config: FriggConfig,
        runtime_profile: RuntimeProfile,
        runtime_watch_active: bool,
        runtime_task_registry: Arc<RwLock<RuntimeTaskRegistry>>,
        validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    ) -> Self {
        let enable_extended_tools =
            active_runtime_tool_surface_profile() == ToolSurfaceProfile::Extended;
        Self::new_with_runtime_context(
            config,
            enable_extended_tools,
            runtime_profile,
            runtime_watch_active,
            None,
            runtime_task_registry,
            validated_manifest_candidate_cache,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_runtime_context(
        config: FriggConfig,
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
                semantic_runtime_credentials: None,
                workspace_registry: Arc::clone(&workspace_registry),
                watch_runtime: Arc::clone(&watch_runtime),
                runtime_task_registry,
                validated_manifest_candidate_cache,
                searcher_projection_store_service: ProjectionStoreService::new(),
                runtime_cache_registry: Arc::new(RwLock::new(RuntimeCacheRegistry::default())),
                runtime_cache_telemetry: Arc::new(RwLock::new(BTreeMap::new())),
                precise_generation_status_cache: Arc::new(RwLock::new(BTreeMap::new())),
                precise_generation_pending_dirty_paths: Arc::new(RwLock::new(BTreeMap::new())),
                tool_call_display_sink: Arc::new(RwLock::new(None)),
            },
            session_state: FriggMcpSessionState::new(workspace_registry, watch_runtime),
            cache_state: FriggMcpCacheState {
                symbol_corpus_cache: Arc::new(RwLock::new(BTreeMap::new())),
                symbol_corpus_cache_epoch: Arc::new(AtomicU64::new(0)),
                precise_graph_cache: Arc::new(RwLock::new(BTreeMap::new())),
                latest_precise_graph_cache: Arc::new(RwLock::new(BTreeMap::new())),
                repository_response_freshness_cache: Arc::new(RwLock::new(BTreeMap::new())),
                repository_response_freshness_cache_epoch: Arc::new(AtomicU64::new(0)),
                precise_generator_probe_cache: Arc::new(RwLock::new(BTreeMap::new())),
                heuristic_reference_cache: Arc::new(RwLock::new(BTreeMap::new())),
                heuristic_reference_cache_epoch: Arc::new(AtomicU64::new(0)),
                compiled_safe_regex_cache: Arc::new(RwLock::new(BTreeMap::new())),
            },
        }
    }

    pub fn new_with_runtime_options(config: FriggConfig, enable_extended_tools: bool) -> Self {
        Self::new_with_runtime_context(
            config,
            enable_extended_tools,
            RuntimeProfile::StdioEphemeral,
            false,
            None,
            Arc::new(RwLock::new(RuntimeTaskRegistry::new())),
            Arc::new(RwLock::new(ValidatedManifestCandidateCache::default())),
        )
    }

    #[doc(hidden)]
    pub fn new_with_semantic_runtime_credentials(
        config: FriggConfig,
        credentials: SemanticRuntimeCredentials,
    ) -> Self {
        let mut server = Self::new(config);
        server.runtime_state.semantic_runtime_credentials = Some(credentials);
        server
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
            move || Ok(self.clone_for_new_session()),
            Arc::new(LocalSessionManager::default()),
            config,
        )
    }

    fn workspace_response(&self) -> WorkspaceResponse {
        let current_workspace = self.current_workspace();
        let repository = current_workspace
            .as_ref()
            .map(|workspace| self.public_repository_summary(workspace));
        let repositories = self
            .visible_workspaces()
            .into_iter()
            .map(|workspace| self.public_repository_summary(&workspace))
            .collect::<Vec<_>>();
        let runtime = self.runtime_status_summary();
        let watch_active = runtime.watch_active;
        let (recommended_action, working_tree_dirty, changed_paths_since_snapshot, fresh_enough_for) =
            self.workspace_gate_fields(current_workspace.as_ref(), &repositories, watch_active);
        let routing_stats = crate::mcp::routing_stats::routing_stats_enabled()
            .then(crate::mcp::routing_stats::snapshot);

        WorkspaceResponse {
            repository,
            session_default: current_workspace.is_some(),
            repositories,
            runtime: Some(runtime),
            recommended_action: Some(recommended_action),
            working_tree_dirty: Some(working_tree_dirty),
            changed_paths_since_snapshot,
            watch_active: Some(watch_active),
            fresh_enough_for,
            routing_stats,
        }
    }

    /// Computes Futura workspace gate fields (`FUT-007`).
    fn workspace_gate_fields(
        &self,
        current_workspace: Option<&AttachedWorkspace>,
        repositories: &[RepositorySummary],
        watch_active: bool,
    ) -> (
        WorkspaceGateAction,
        bool,
        Vec<String>,
        Option<Vec<String>>,
    ) {
        if repositories.is_empty() && current_workspace.is_none() {
            return (WorkspaceGateAction::AdoptRepo, false, Vec::new(), None);
        }

        let Some(workspace) = current_workspace else {
            // Visible repos exist but no session default/adoption.
            return (WorkspaceGateAction::AdoptRepo, false, Vec::new(), None);
        };

        let storage = Self::workspace_storage_summary(workspace);
        let dirty = self.workspace_has_dirty_root(workspace);
        let changed_paths = self.changed_paths_since_snapshot_for_gate(&workspace.repository_id);
        let working_tree_dirty = dirty || !changed_paths.is_empty();

        let index_needs_reindex = !matches!(storage.index_state, WorkspaceStorageIndexState::Ready);
        if index_needs_reindex {
            return (
                WorkspaceGateAction::Reindex,
                working_tree_dirty,
                changed_paths,
                None,
            );
        }

        if !watch_active && self.runtime_state.runtime_profile.persistent_state_available() {
            // Persistent runtime expects a watch; without it, prefer waiting over stale trust.
            if working_tree_dirty {
                return (
                    WorkspaceGateAction::UseLiveDiskForTouchedFiles,
                    working_tree_dirty,
                    changed_paths,
                    Some(vec!["read_file".to_owned()]),
                );
            }
            return (
                WorkspaceGateAction::WaitWatch,
                working_tree_dirty,
                changed_paths,
                None,
            );
        }

        if working_tree_dirty {
            return (
                WorkspaceGateAction::UseLiveDiskForTouchedFiles,
                working_tree_dirty,
                changed_paths,
                Some(vec!["read_file".to_owned(), "read_match".to_owned()]),
            );
        }

        (
            WorkspaceGateAction::Ready,
            false,
            Vec::new(),
            Some(vec![
                "search_text".to_owned(),
                "search_symbol".to_owned(),
                "search_hybrid".to_owned(),
                "find_references".to_owned(),
                "go_to_definition".to_owned(),
                "read_file".to_owned(),
                "read_match".to_owned(),
            ]),
        )
    }

    /// Best-effort dirty paths for the workspace gate (`FUT-020`).
    ///
    /// Prefer precise-generation pending dirty paths (watch/hot reindex queue). Paths are
    /// path-scoped only — `use_live_disk_for_touched_files` never licenses repo-wide grep.
    fn changed_paths_since_snapshot_for_gate(&self, repository_id: &str) -> Vec<String> {
        let pending = self
            .runtime_state
            .precise_generation_pending_dirty_paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        pending
            .get(repository_id)
            .map(|(changed, deleted)| {
                let mut paths = changed
                    .iter()
                    .chain(deleted.iter())
                    .take(32)
                    .cloned()
                    .collect::<Vec<_>>();
                paths.sort();
                paths.dedup();
                paths
            })
            .unwrap_or_default()
    }

    /// Test/helper: record dirty paths so workspace gate can surface them without a full watch cycle.
    #[doc(hidden)]
    pub fn test_record_gate_dirty_paths(
        &self,
        repository_id: &str,
        changed_paths: &[String],
        deleted_paths: &[String],
    ) {
        let mut pending = self
            .runtime_state
            .precise_generation_pending_dirty_paths
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::record_pending_precise_dirty_paths_locked(
            &mut pending,
            repository_id,
            changed_paths,
            deleted_paths,
        );
    }

    /// Test/helper: mark validated-manifest dirty root so the gate sees a lagging index.
    #[doc(hidden)]
    pub fn test_mark_workspace_dirty_root(&self, root: &std::path::Path) {
        self.runtime_state
            .validated_manifest_candidate_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .mark_dirty_root(root);
    }

    async fn ensure_workspace_for_status(&self, params: &WorkspaceParams) -> Result<(), ErrorData> {
        let set_default = params.set_default.unwrap_or(true);
        let resolve_mode = params.resolve_mode.unwrap_or(WorkspaceResolveMode::GitRoot);
        let target_path = params.path.as_deref();
        let target_repository_id = params.repository_id.as_deref();

        if target_path.is_some() || target_repository_id.is_some() {
            let outcome = self.attach_workspace_target_internal(
                target_path,
                target_repository_id,
                set_default,
                resolve_mode,
                WorkspaceAttachIndexMode::Ensure,
            )?;
            let repository_id = outcome.response.repository.repository_id.clone();
            if let Some(workspace) = self.workspace_by_repository_id(&repository_id) {
                let (index_lifecycle, index_summary) = self
                    .ensure_workspace_index_for_attach(
                        &workspace,
                        true,
                        Duration::from_millis(30_000),
                    )
                    .await;
                if matches!(index_lifecycle.phase, WorkspaceIndexLifecyclePhase::Ready) {
                    match index_summary.as_ref() {
                        Some(summary) => {
                            self.maybe_spawn_workspace_precise_generation_for_paths(
                                &workspace,
                                &summary.changed_paths,
                                &summary.deleted_paths,
                            );
                        }
                        None => {
                            self.maybe_spawn_workspace_precise_generation_for_paths(
                                &workspace,
                                &[],
                                &[],
                            );
                        }
                    }
                }
            }
            if let Some(guard) = outcome.rollback_guard {
                guard.disarm();
            }
            return Ok(());
        }

        if self.current_repository_id().is_some() || !self.attached_workspaces().is_empty() {
            return Ok(());
        }

        let auto_adoptable_workspaces = self.auto_adoptable_workspaces();
        if let [workspace] = auto_adoptable_workspaces.as_slice() {
            self.adopt_workspace(workspace, true)?;
            return Ok(());
        }

        if let Ok(current_dir) = std::env::current_dir() {
            let current_dir = current_dir.display().to_string();
            let outcome = self.attach_workspace_target_internal(
                Some(&current_dir),
                None,
                true,
                WorkspaceResolveMode::GitRoot,
                WorkspaceAttachIndexMode::Ensure,
            )?;
            let repository_id = outcome.response.repository.repository_id.clone();
            if let Some(workspace) = self.workspace_by_repository_id(&repository_id) {
                let (index_lifecycle, index_summary) = self
                    .ensure_workspace_index_for_attach(
                        &workspace,
                        true,
                        Duration::from_millis(30_000),
                    )
                    .await;
                if matches!(index_lifecycle.phase, WorkspaceIndexLifecyclePhase::Ready) {
                    match index_summary.as_ref() {
                        Some(summary) => {
                            self.maybe_spawn_workspace_precise_generation_for_paths(
                                &workspace,
                                &summary.changed_paths,
                                &summary.deleted_paths,
                            );
                        }
                        None => {
                            self.maybe_spawn_workspace_precise_generation_for_paths(
                                &workspace,
                                &[],
                                &[],
                            );
                        }
                    }
                }
            }
            if let Some(guard) = outcome.rollback_guard {
                guard.disarm();
            }
        }

        Ok(())
    }
}

#[tool_router(router = tool_router)]
impl FriggMcpServer {
    #[tool(
        name = "workspace",
        description = "Show workspace status; adopt path/repository_id or auto-adopt a default when detached.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn workspace(
        &self,
        params: Parameters<WorkspaceParams>,
    ) -> Result<Json<WorkspaceResponse>, ErrorData> {
        let params = params.0;
        let repository_hint = params.repository_id.clone();
        let started_at = Instant::now();
        self.ensure_workspace_for_status(&params).await?;
        let response = self.workspace_response();
        let repository_ids = response
            .repositories
            .iter()
            .map(|repository| repository.repository_id.clone())
            .collect::<Vec<_>>();
        let current_repository_id = response
            .repository
            .as_ref()
            .map(|repository| repository.repository_id.clone());
        let finalization = self.tool_execution_finalization(
            json!({
                "repository_id": current_repository_id,
                "repository_ids": repository_ids.clone(),
                "session_default": response.session_default,
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
            }),
            Some(FriggMcpServer::provenance_normalized_workload_metadata(
                "workspace",
                &repository_ids,
                WorkloadPrecisionMode::Exact,
                None,
                None,
                None,
            )),
        );
        let result = Ok(Json(response));
        let provenance_result = self
            .record_provenance_blocking(
                "workspace",
                repository_hint.as_deref(),
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
        self.finalize_with_provenance_timed(
            "workspace",
            started_at,
            result,
            provenance_result,
            None,
        )
    }

    #[tool(
        name = "list_repositories",
        description = "List session-visible repositories with compact session, watch, and storage state.",
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
                    .visible_workspaces()
                    .into_iter()
                    .map(|workspace| server.public_repository_summary(&workspace))
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
        name = "list_files",
        description = "List repository files with optional path, glob, language, class, hidden-file, and pagination filters. Latency: hot when scoped (path_regex/glob); prefer over shell find/fd on attached repos. Not for content search — use search_text/search_symbol.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn list_files(
        &self,
        params: Parameters<ListFilesParams>,
    ) -> Result<Json<ListFilesResponse>, ErrorData> {
        self.list_files_impl(params.0).await
    }

    #[tool(
        name = "workspace_attach",
        description = "Adopt a repository into this session. Use this before repo-aware tools when detached or when you want a stable default repository.",
        annotations(
            read_only_hint = true,
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
        let wait_for_precise = params.wait_for_precise.unwrap_or(true);
        let index_mode = WorkspaceAttachIndexMode::Ensure;
        let wait_for_index = true;
        let index_timeout = Duration::from_millis(30_000);
        let started_at = Instant::now();
        info!(
            requested_path = params.path.as_deref().unwrap_or(""),
            requested_repository_id = params.repository_id.as_deref().unwrap_or(""),
            set_default,
            resolve_mode = ?resolve_mode,
            "workspace attach started"
        );
        let attach_path = params.path.as_deref();
        let attach_outcome = match self.attach_workspace_target_internal(
            attach_path,
            params.repository_id.as_deref(),
            set_default,
            resolve_mode,
            index_mode,
        ) {
            Ok(response) => response,
            Err(err) => {
                warn!(
                    requested_path = params.path.as_deref().unwrap_or(""),
                    requested_repository_id = params.repository_id.as_deref().unwrap_or(""),
                    set_default,
                    resolve_mode = ?resolve_mode,
                    duration_ms = started_at.elapsed().as_millis() as u64,
                    error = %err.message,
                    "workspace attach failed"
                );
                return Err(err);
            }
        };
        let rollback_guard = attach_outcome.rollback_guard;
        let mut response = attach_outcome.response;
        if matches!(index_mode, WorkspaceAttachIndexMode::Ensure) {
            let repository_id = response.repository.repository_id.clone();
            if let Some(workspace) = self.workspace_by_repository_id(&repository_id) {
                let (index_lifecycle, index_summary) = self
                    .ensure_workspace_index_for_attach(&workspace, wait_for_index, index_timeout)
                    .await;
                response.index_lifecycle = index_lifecycle;
                let precise_generation_action = if matches!(
                    response.index_lifecycle.phase,
                    WorkspaceIndexLifecyclePhase::Ready
                ) {
                    match index_summary.as_ref() {
                        Some(summary) => self.maybe_spawn_workspace_precise_generation_for_paths(
                            &workspace,
                            &summary.changed_paths,
                            &summary.deleted_paths,
                        ),
                        None => self.maybe_spawn_workspace_precise_generation_for_paths(
                            &workspace,
                            &[],
                            &[],
                        ),
                    }
                } else {
                    WorkspacePreciseGenerationAction::NotApplicable
                };
                let mut repository = self.public_repository_summary(&workspace);
                let storage = repository
                    .storage
                    .clone()
                    .unwrap_or_else(|| Self::workspace_storage_summary(&workspace));
                repository.storage = None;
                let precise = self.workspace_precise_summary_for_workspace(
                    &workspace,
                    Some(precise_generation_action),
                );
                response.repository = repository;
                response.storage = storage;
                response.precise = precise.clone();
                response.precise_lifecycle = self.workspace_precise_lifecycle_summary(
                    &workspace,
                    precise_generation_action,
                    &precise,
                    false,
                    false,
                );
            }
        }
        if wait_for_precise {
            let repository_id = response.repository.repository_id.clone();
            let completed = self
                .wait_for_repository_precise_generation(&repository_id, Duration::from_secs(30))
                .await;
            if let Some(workspace) = self.workspace_by_repository_id(&repository_id) {
                let mut repository = self.public_repository_summary(&workspace);
                let storage = repository
                    .storage
                    .clone()
                    .unwrap_or_else(|| Self::workspace_storage_summary(&workspace));
                repository.storage = None;
                let generation_action = response
                    .precise
                    .generation_action
                    .unwrap_or(WorkspacePreciseGenerationAction::NotApplicable);
                let precise = self
                    .workspace_precise_summary_for_workspace(&workspace, Some(generation_action));
                response.repository = repository;
                response.storage = storage;
                response.precise = precise.clone();
                response.precise_lifecycle = self.workspace_precise_lifecycle_summary(
                    &workspace,
                    generation_action,
                    &precise,
                    true,
                    !completed,
                );
            }
        }
        if let Err(err) =
            self.ensure_workspace_attach_still_adopted(&response.repository.repository_id)
        {
            warn!(
                repository_id = %response.repository.repository_id,
                root = %response.repository.root_path,
                duration_ms = started_at.elapsed().as_millis() as u64,
                error = %err.message,
                "workspace attach cancelled before completion"
            );
            return Err(err);
        }
        info!(
            repository_id = %response.repository.repository_id,
            root = %response.repository.root_path,
            action = ?response.action,
            resolution = ?response.resolution,
            session_default = response.session_default,
            precise_state = ?response.precise.state,
            precise_generation_action = ?response.precise.generation_action,
            precise_phase = ?response.precise_lifecycle.phase,
            index_phase = ?response.index_lifecycle.phase,
            index_mode = ?response.index_lifecycle.mode,
            duration_ms = started_at.elapsed().as_millis() as u64,
            "workspace attach completed"
        );
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
                "precise_lifecycle": {
                    "phase": response.precise_lifecycle.phase,
                    "waited_for_completion": response.precise_lifecycle.waited_for_completion,
                    "generation_action": response.precise_lifecycle.generation_action,
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
        let result = Ok(Json(response.clone()));
        let provenance_result = self
            .record_provenance_blocking(
                "workspace_attach",
                None,
                json!({
                    "path": params.path.as_deref().map(Self::bounded_text),
                    "repository_id": params.repository_id,
                    "set_default": params.set_default,
                    "resolve_mode": params.resolve_mode,
                    "wait_for_precise": params.wait_for_precise,
                }),
                finalization.source_refs,
                &result,
            )
            .await;
        if let Some(guard) = rollback_guard {
            guard.disarm();
        }
        self.finalize_with_provenance_timed(
            "workspace_attach",
            started_at,
            result,
            provenance_result,
            None,
        )
    }

    #[tool(
        name = "workspace_detach",
        description = "Remove a repository from this session and release its watch lease when applicable.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn workspace_detach(
        &self,
        params: Parameters<WorkspaceDetachParams>,
    ) -> Result<Json<WorkspaceDetachResponse>, ErrorData> {
        let params = params.0;
        let started_at = Instant::now();
        let repository_id = match params.repository_id.or_else(|| self.current_repository_id()) {
            Some(repository_id) => repository_id,
            None => {
                let attached = self.attached_workspaces();
                if attached.is_empty() {
                    return Err(Self::no_attached_workspaces_error("workspace_detach"));
                }
                return Err(Self::workspace_detach_requires_repository_id_error(&attached));
            }
        };
        let detached = self.detach_workspace(&repository_id)?;
        let Some(workspace) = detached else {
            return Err(Self::resource_not_found(
                "repository_id is not adopted in this session",
                Some(json!({ "repository_id": repository_id })),
            ));
        };
        self.invalidate_repository_symbol_corpus_cache(&workspace.repository_id);
        self.invalidate_repository_response_freshness_cache(&workspace.repository_id);
        self.invalidate_repository_precise_generator_probe_cache(&workspace.repository_id);
        self.scip_invalidate_repository_precise_generation_cache(&workspace.repository_id);
        self.invalidate_repository_precise_graph_caches(&workspace.repository_id);
        let active_session_count = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_session_count(&workspace.repository_id);
        if active_session_count == 0
            && !self.repository_has_active_watch_lease(&workspace.repository_id)
        {
            self.clear_pending_precise_dirty_paths(&workspace.repository_id);
        }
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
        self.finalize_with_provenance_timed(
            "workspace_detach",
            started_at,
            result,
            provenance_result,
            None,
        )
    }

    #[tool(
        name = "workspace_prepare",
        description = "Initialize Frigg repository state and adopt it into this session; requires confirmation.",
        annotations(
            read_only_hint = true,
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
        let started_at = Instant::now();
        let (workspace, resolved_from, resolution, _resolution_guard) = self
            .resolve_workspace_target(
                params.path.as_deref(),
                params.repository_id.as_deref(),
                resolve_mode,
            )?;
        info!(
            repository_id = %workspace.repository_id,
            root = %workspace.root.display(),
            set_default,
            resolve_mode = ?resolve_mode,
            requested_path = params.path.as_deref().unwrap_or(""),
            requested_repository_id = params.repository_id.as_deref().unwrap_or(""),
            "workspace prepare started"
        );
        let mut task_guard = match self.try_start_repository_runtime_task(
            &workspace,
            RuntimeTaskKind::WorkspacePrepare,
            "workspace_prepare",
            Some(format!("prepare {}", workspace.root.display())),
        ) {
            Ok(task_guard) => task_guard,
            Err(active_tasks) => {
                warn!(
                    repository_id = %workspace.repository_id,
                    root = %workspace.root.display(),
                    active_tasks = active_tasks.len(),
                    duration_ms = started_at.elapsed().as_millis() as u64,
                    "workspace prepare rejected because runtime work is active"
                );
                return Err(Self::invalid_params(
                    "repository already has active runtime work",
                    Some(json!({
                        "repository_id": workspace.repository_id,
                        "active_tasks": active_tasks,
                    })),
                ));
            }
        };
        Self::notify_progress(&meta, &client, 0.0, 4.0, "resolve target").await;

        Self::notify_progress(&meta, &client, 1.0, 4.0, "initialize storage").await;
        let prepared_storage_result = match Self::run_blocking_task("workspace_prepare", {
            let workspace = workspace.clone();
            move || -> FriggResult<WorkspaceStorageSummary> {
                let db_path = ensure_provenance_db_parent_dir(&workspace.root)?;
                let storage = Storage::new(&db_path);
                storage.initialize_with_auto_repair()?;
                Ok(FriggMcpServer::workspace_storage_summary(&workspace))
            }
        })
        .await
        {
            Ok(result) => result,
            Err(error) => {
                task_guard.finish(RuntimeTaskStatus::Failed, Some(error.message.to_string()));
                return Err(error);
            }
        };
        let prepared_storage = prepared_storage_result.map_err(|err| {
            let error_message = err.to_string();
            warn!(
                repository_id = %workspace.repository_id,
                root = %workspace.root.display(),
                duration_ms = started_at.elapsed().as_millis() as u64,
                error = %error_message,
                "workspace prepare failed during storage initialization"
            );
            task_guard.finish(RuntimeTaskStatus::Failed, Some(error_message));
            Self::map_frigg_error(err)
        })?;

        Self::notify_progress(&meta, &client, 2.0, 4.0, "validate storage").await;
        self.runtime_state
            .validated_manifest_candidate_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .invalidate_root(&workspace.root);
        self.invalidate_workspace_index_runtime_caches(&workspace, true);

        Self::notify_progress(&meta, &client, 3.0, 4.0, "activate watcher lease").await;
        self.adopt_workspace(&workspace, set_default)
            .inspect_err(|error| {
                warn!(
                    repository_id = %workspace.repository_id,
                    root = %workspace.root.display(),
                    duration_ms = started_at.elapsed().as_millis() as u64,
                    error = %error.message,
                    "workspace prepare failed while adopting workspace"
                );
                task_guard.finish(RuntimeTaskStatus::Failed, Some(error.message.to_string()));
            })?;
        task_guard.finish(RuntimeTaskStatus::Succeeded, None);
        self.maybe_spawn_workspace_runtime_prewarm(&workspace);
        let _ = self.maybe_spawn_workspace_precise_generation_for_paths(&workspace, &[], &[]);

        let mut repository = self.public_repository_summary(&workspace);
        repository.storage = None;
        let response = WorkspacePrepareResponse {
            repository,
            resolved_from,
            resolution,
            session_default: self.current_repository_id().as_deref()
                == Some(workspace.repository_id.as_str()),
            storage: prepared_storage,
        };
        info!(
            repository_id = %response.repository.repository_id,
            root = %response.repository.root_path,
            session_default = response.session_default,
            resolution = ?response.resolution,
            storage_db_path = %response.storage.db_path,
            storage_index_state = ?response.storage.index_state,
            duration_ms = started_at.elapsed().as_millis() as u64,
            "workspace prepare completed"
        );
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
        self.finalize_with_provenance_timed(
            "workspace_prepare",
            started_at,
            result,
            provenance_result,
            None,
        )
    }

    fn capped_workspace_index_paths(paths: &[String]) -> (Vec<String>, bool) {
        let truncated = paths.len() > Self::WORKSPACE_INDEX_RESPONSE_PATH_LIMIT;
        (
            paths
                .iter()
                .take(Self::WORKSPACE_INDEX_RESPONSE_PATH_LIMIT)
                .cloned()
                .collect(),
            truncated,
        )
    }

    #[tool(
        name = "workspace_index",
        description = "Refresh Frigg index state for a repository and adopt it; requires confirmation.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    pub async fn workspace_index(
        &self,
        meta: Meta,
        client: Peer<RoleServer>,
        params: Parameters<WorkspaceIndexParams>,
    ) -> Result<Json<WorkspaceIndexResponse>, ErrorData> {
        let params = params.0;
        Self::require_confirm("workspace_index", params.confirm)?;
        let set_default = params.set_default.unwrap_or(true);
        let resolve_mode = params.resolve_mode.unwrap_or(WorkspaceResolveMode::GitRoot);
        let started_at = Instant::now();
        let (workspace, resolved_from, resolution, _resolution_guard) = self
            .resolve_workspace_target(
                params.path.as_deref(),
                params.repository_id.as_deref(),
                resolve_mode,
            )?;
        info!(
            repository_id = %workspace.repository_id,
            root = %workspace.root.display(),
            set_default,
            resolve_mode = ?resolve_mode,
            requested_path = params.path.as_deref().unwrap_or(""),
            requested_repository_id = params.repository_id.as_deref().unwrap_or(""),
            "workspace index started"
        );
        let mut task_guard = match self.try_start_repository_runtime_task(
            &workspace,
            RuntimeTaskKind::WorkspaceIndex,
            "workspace_index",
            Some(format!("index {}", workspace.root.display())),
        ) {
            Ok(task_guard) => task_guard,
            Err(active_tasks) => {
                warn!(
                    repository_id = %workspace.repository_id,
                    root = %workspace.root.display(),
                    active_tasks = active_tasks.len(),
                    duration_ms = started_at.elapsed().as_millis() as u64,
                    "workspace index rejected because runtime work is active"
                );
                return Err(Self::invalid_params(
                    "repository already has active runtime work",
                    Some(json!({
                        "repository_id": workspace.repository_id,
                        "active_tasks": active_tasks,
                    })),
                ));
            }
        };
        Self::notify_progress(&meta, &client, 0.0, 4.0, "resolve target").await;
        Self::notify_progress(&meta, &client, 1.0, 4.0, "index refresh").await;
        let semantic_runtime = self.config.semantic_runtime.clone();
        let index_result = match Self::run_blocking_task("workspace_index", {
            let workspace = workspace.clone();
            move || -> Result<crate::indexer::IndexSummary, String> {
                let db_path = ensure_provenance_db_parent_dir(&workspace.root)
                    .map_err(|err| err.to_string())?;
                let credentials = SemanticRuntimeCredentials::from_process_env();
                index_repository_with_runtime_config(
                    &workspace.runtime_repository_id,
                    &workspace.root,
                    &db_path,
                    IndexMode::ChangedOnly,
                    &semantic_runtime,
                    &credentials,
                )
                .map_err(|err| err.to_string())
            }
        })
        .await
        {
            Ok(result) => result,
            Err(error) => {
                task_guard.finish(RuntimeTaskStatus::Failed, Some(error.message.to_string()));
                return Err(error);
            }
        };
        self.invalidate_workspace_index_runtime_caches(&workspace, true);
        let index_summary = index_result.map_err(|err| {
            warn!(
                repository_id = %workspace.repository_id,
                root = %workspace.root.display(),
                duration_ms = started_at.elapsed().as_millis() as u64,
                error = %err,
                "workspace index failed during index refresh"
            );
            task_guard.finish(RuntimeTaskStatus::Failed, Some(err.clone()));
            Self::internal(
                err,
                Some(json!({ "repository_id": workspace.repository_id })),
            )
        })?;

        Self::notify_progress(&meta, &client, 2.0, 4.0, "invalidate caches").await;

        Self::notify_progress(&meta, &client, 3.0, 4.0, "finalize").await;
        self.adopt_workspace(&workspace, set_default)
            .inspect_err(|error| {
                warn!(
                    repository_id = %workspace.repository_id,
                    root = %workspace.root.display(),
                    duration_ms = started_at.elapsed().as_millis() as u64,
                    error = %error.message,
                    "workspace index failed while adopting workspace"
                );
                task_guard.finish(RuntimeTaskStatus::Failed, Some(error.message.to_string()));
            })?;
        let precise_generation_action = self.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &index_summary.changed_paths,
            &index_summary.deleted_paths,
        );
        let precise = self
            .workspace_precise_summary_for_workspace(&workspace, Some(precise_generation_action));
        let precise_lifecycle = self.workspace_precise_lifecycle_summary(
            &workspace,
            precise_generation_action,
            &precise,
            false,
            false,
        );
        let mut repository = self.public_repository_summary(&workspace);
        let storage = repository
            .storage
            .clone()
            .unwrap_or_else(|| Self::workspace_storage_summary(&workspace));
        repository.storage = None;
        let (changed_paths, changed_paths_truncated) =
            Self::capped_workspace_index_paths(&index_summary.changed_paths);
        let (deleted_paths, deleted_paths_truncated) =
            Self::capped_workspace_index_paths(&index_summary.deleted_paths);
        let mut response = WorkspaceIndexResponse {
            repository,
            resolved_from,
            resolution,
            session_default: self.current_repository_id().as_deref()
                == Some(workspace.repository_id.as_str()),
            storage,
            snapshot_id: index_summary.snapshot_id.clone(),
            files_scanned: index_summary.files_scanned,
            files_changed: index_summary.files_changed,
            files_deleted: index_summary.files_deleted,
            diagnostics_count: index_summary.diagnostics.total_count(),
            changed_paths,
            deleted_paths,
            paths_truncated: changed_paths_truncated || deleted_paths_truncated,
            precise_lifecycle,
        };
        if params.wait_for_precise.unwrap_or(true) {
            let repository_id = response.repository.repository_id.clone();
            let completed = self
                .wait_for_repository_precise_generation(&repository_id, Duration::from_secs(30))
                .await;
            if let Some(workspace) = self.workspace_by_repository_id(&repository_id) {
                let generation_action = response.precise_lifecycle.generation_action;
                let precise = self
                    .workspace_precise_summary_for_workspace(&workspace, Some(generation_action));
                let mut repository = self.public_repository_summary(&workspace);
                let storage = repository
                    .storage
                    .clone()
                    .unwrap_or_else(|| Self::workspace_storage_summary(&workspace));
                repository.storage = None;
                response.repository = repository;
                response.storage = storage;
                response.precise_lifecycle = self.workspace_precise_lifecycle_summary(
                    &workspace,
                    generation_action,
                    &precise,
                    true,
                    !completed,
                );
            }
        }
        info!(
            repository_id = %response.repository.repository_id,
            root = %response.repository.root_path,
            resolution = ?response.resolution,
            session_default = response.session_default,
            snapshot_id = %response.snapshot_id,
            files_scanned = response.files_scanned,
            files_changed = response.files_changed,
            files_deleted = response.files_deleted,
            diagnostics_count = response.diagnostics_count,
            precise_phase = ?response.precise_lifecycle.phase,
            duration_ms = started_at.elapsed().as_millis() as u64,
            "workspace index completed"
        );
        task_guard.finish(RuntimeTaskStatus::Succeeded, None);
        Self::notify_progress(&meta, &client, 4.0, 4.0, "done").await;

        let finalization = self.tool_execution_finalization(
            json!({
                "repository_id": response.repository.repository_id.clone(),
                "snapshot_id": response.snapshot_id,
                "files_scanned": response.files_scanned,
                "files_changed": response.files_changed,
                "files_deleted": response.files_deleted,
                "diagnostics_count": response.diagnostics_count,
                "changed_paths": response.changed_paths,
                "deleted_paths": response.deleted_paths,
                "paths_truncated": response.paths_truncated,
                "session_default": response.session_default,
                "precise_lifecycle": {
                    "phase": response.precise_lifecycle.phase,
                    "waited_for_completion": response.precise_lifecycle.waited_for_completion,
                    "generation_action": response.precise_lifecycle.generation_action,
                },
            }),
            Some(FriggMcpServer::provenance_normalized_workload_metadata(
                "workspace_index",
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
                "workspace_index",
                None,
                json!({
                    "path": params.path.as_deref().map(Self::bounded_text),
                    "repository_id": params.repository_id,
                    "set_default": params.set_default,
                    "resolve_mode": params.resolve_mode,
                    "confirm": params.confirm,
                    "wait_for_precise": params.wait_for_precise,
                }),
                finalization.source_refs,
                &result,
            )
            .await;
        self.finalize_with_provenance_timed(
            "workspace_index",
            started_at,
            result,
            provenance_result,
            None,
        )
    }

    #[tool(
        name = "workspace_current",
        description = "Inspect the session default, adopted repositories, and runtime tasks without computing detailed index diagnostics.",
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
        let current_repository = current_workspace
            .as_ref()
            .map(|workspace| self.public_repository_summary(workspace));
        let repositories = self
            .attached_workspaces()
            .into_iter()
            .map(|workspace| self.public_repository_summary(&workspace))
            .collect::<Vec<_>>();
        let runtime = self.runtime_status_summary();
        let response = WorkspaceCurrentResponse {
            repository: current_repository,
            session_default: current_workspace.is_some(),
            repositories,
            precise: None,
            precise_ingest: None,
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
        description = "Read a bounded repository-relative source file or line window. Text mode is raw source (no line prefixes). presentation_mode=json returns path/bytes/start_line/end_line metadata; presentation_mode=citation emits LINE|content for user-facing citations.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn read_file(
        &self,
        params: Parameters<ReadFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let response = self.read_file_impl(params.clone()).await?;
        self.present_read_file_result(&params, response)
    }

    #[tool(
        name = "read_match",
        description = "Read a bounded source window for a prior result_handle and match_id from the same call. Prefer for proof after search/nav. Content parity with an equivalent read_file(start_line,end_line) window. presentation_mode=citation emits LINE|content.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn read_match(
        &self,
        params: Parameters<ReadMatchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let response = self.read_match_impl(params.clone()).await?;
        self.present_read_match_result(&params, response)
    }

    #[tool(
        name = "explore",
        description = "File-anchor only: probe/refine/zoom inside one known repository file. Not cross-repo search — use search_text/search_symbol/search_hybrid for that. Call after you already have a path.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn explore(
        &self,
        params: Parameters<ExploreParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let response = self.explore_impl(params.clone()).await?;
        self.present_explore_result(&params, response)
    }

    #[tool(
        name = "search_text",
        description = "rg-shaped exact search (literal default). Latency: hot when scoped (path_regex/glob); warm/cold when unscoped or huge. Prefer over shell rg for indexed source. rg -n 'a|b' → pattern_type=regex; rg -c → count_only=true (read total_matches); rg -g '**/*.rs' → glob='**/*.rs'. Prefer path_regex='^src/' for implementation questions. Do not use for vague discovery (search_hybrid) or known symbol names (search_symbol).",
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
        description = "Discovery pivots for broad code questions without a known string, symbol, or path. Latency: warm/cold (allowed slower than exact; still returns pivots promptly). Not proof: after hybrid run search_text or search_symbol, then read_match. Compact responses include ranking_note, best_pivot_path, suggested_next, latency_class. Do not answer from rank-1 alone or shell-grep as the precision step. Do not use when the string or symbol name is already known.",
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
        description = "Known-name lookup for APIs, types, functions, classes, methods, or identifiers. Latency: hot when runtime-scoped; prefer over hybrid and shell rg when the name is known. Defaults to path_class=runtime (skip tests); pass path_class=support or any for tests. Rows include column/excerpt and latency_class for navigation handoff when indexed. Do not use for free-text error strings (search_text) or vague discovery (search_hybrid).",
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
        name = "search_batch",
        description = "Multi-probe search (2–8 text/symbol/hybrid probes) that replaces parallel grep as a latency strategy. Latency: warm for ≤3 probes, cold for larger batches; still better wall-clock than sequential MCP probes for multi-hypothesis turns. Returns merged, deduped matches with probe_id(s), probe_summary[], suggested_next, latency_class. Prefer over same-turn parallel search_text fan-out or parallel shell greps. Do not use for a single known string (search_text) or single known name (search_symbol).",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn search_batch(
        &self,
        params: Parameters<SearchBatchParams>,
    ) -> Result<Json<SearchBatchResponse>, ErrorData> {
        self.search_batch_impl(params.0).await
    }

    #[tool(
        name = "find_references",
        description = "Find definition and usage rows for a symbol or cursor location. Latency: warm with precise graph; may be colder on heuristic fallback. Prefer over whole-repo shell rg for impact. Pass symbol when known; path+line alone is denser/ambiguous.",
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
        description = "Find likely definitions. Latency: warm/hot when symbol is known; rejects empty {} quickly with recovery. Prefer symbol=<name> (recommended). path+line without symbol can be ambiguous on dense lines — pass symbol or column. Prefer over shell rg for go-to-def on indexed source.",
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
        description = "Find declaration anchors for a symbol or cursor location. Prefer symbol= when the name is known.",
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
        description = "Find implementations for traits/interfaces (not ordinary structs). Prefer symbol= for the trait/interface name.",
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
        description = "Who calls this? Find callers for a callable symbol or cursor location. Prefer for impact/blast-radius after a symbol anchor.",
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
        description = "Find callees for a callable symbol or cursor location. Provisional until confirmed with body proof reads (read_match/read_file).",
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
        description = "File outline for one supported source file. Latency: hot/warm for single-file outline. Defaults to top_level_only=true. Known name → use search_symbol instead. Large outlines paginate via limit/resume_from with total_symbols/returned/truncated. Not for cross-file discovery.",
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
        description = "Return AST focus/ancestors/children around a source location. Requires line AND column together (use column=1 if unknown). Prerequisite for search_structural when node shape is unclear.",
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
        description = "Run a bounded Tree-sitter query and return structural matches.",
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
        name = "impact_bundle",
        description = "Convenience composition: symbol hits + find_references + incoming_calls (+ implementations for traits/interfaces or when include_implementations=true). Individual tools remain the source of truth. suggested_next covers tests pass and proof reads.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn impact_bundle(
        &self,
        params: Parameters<ImpactBundleParams>,
    ) -> Result<Json<ImpactBundleResponse>, ErrorData> {
        self.impact_bundle_impl(params.0).await
    }

    #[cfg_attr(
        feature = "playbook",
        tool(
            name = "playbook_run",
            description = "Run a trace-oriented playbook and return the resulting trace artifact.",
            annotations(
                read_only_hint = true,
                destructive_hint = false,
                idempotent_hint = true
            )
        )
    )]
    #[cfg(feature = "playbook")]
    pub async fn playbook_run(
        &self,
        params: Parameters<PlaybookRunParams>,
    ) -> Result<Json<PlaybookRunResponse>, ErrorData> {
        self.playbook_run_impl(params.0.into()).await
    }

    #[cfg_attr(
        feature = "playbook",
        tool(
            name = "playbook_replay",
            description = "Replay a playbook against an expected trace artifact and report whether it still matches.",
            annotations(
                read_only_hint = true,
                destructive_hint = false,
                idempotent_hint = true
            )
        )
    )]
    #[cfg(feature = "playbook")]
    pub async fn playbook_replay(
        &self,
        params: Parameters<PlaybookReplayParams>,
    ) -> Result<Json<PlaybookReplayResponse>, ErrorData> {
        self.playbook_replay_impl(params.0).await
    }

    #[cfg_attr(
        feature = "playbook",
        tool(
            name = "playbook_compose_citations",
            description = "Compose citation payloads from an existing playbook trace artifact.",
            annotations(
                read_only_hint = true,
                destructive_hint = false,
                idempotent_hint = true
            )
        )
    )]
    #[cfg(feature = "playbook")]
    pub async fn playbook_compose_citations(
        &self,
        params: Parameters<PlaybookComposeCitationsParams>,
    ) -> Result<Json<PlaybookComposeCitationsResponse>, ErrorData> {
        self.playbook_compose_citations_impl(params.0).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for FriggMcpServer {
    fn get_info(&self) -> ServerInfo {
        let tool_surface_profile = self.tool_surface_profile.as_str();
        let runtime_profile = self.runtime_state.runtime_profile.as_str();
        let tool_surface_note = if self.tool_surface_profile == ToolSurfaceProfile::Extended {
            format!(
                "The extended tool surface is enabled by default. Set `{TOOL_SURFACE_PROFILE_ENV}=core` to restrict the runtime to the stable core subset."
            )
        } else if cfg!(feature = "playbook") {
            format!(
                "The runtime is pinned to the restricted core tool surface. Set `{TOOL_SURFACE_PROFILE_ENV}=extended` to expose `explore` and compiled-in playbook tools."
            )
        } else {
            format!(
                "The runtime is pinned to the restricted core tool surface. Set `{TOOL_SURFACE_PROFILE_ENV}=extended` to expose `explore`."
            )
        };
        let playbook_guidance = if cfg!(feature = "playbook") {
            " Use playbook tools only for explicit trace workflows when those tools are present in the active profile."
        } else {
            ""
        };
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_prompts()
                .enable_resources()
                .enable_tools()
                .build(),
        )
            .with_server_info(
                Implementation::new("frigg", env!("CARGO_PKG_VERSION"))
                    .with_title("Frigg MCP")
                    .with_description("Local-first code search + navigation MCP server"),
            )
            .with_instructions(agent_directive::mcp_instructions(&format!(
                "Runtime profile: `{runtime_profile}`. Tool surface: `{tool_surface_profile}`. Omit repository_id in normal single-repo work; call workspace for compact status or to adopt a target path/repository. Repo-aware tools auto-adopt sensible defaults when possible. Detailed routing lives in `{SUPPORT_MATRIX_RESOURCE_URI}`, `{TOOL_SURFACE_RESOURCE_URI}`, `{SHELL_REPLACEMENT_MAP_RESOURCE_URI}`, `{SHELL_GUIDANCE_RESOURCE_URI}`, optional local stats `{ROUTING_STATS_RESOURCE_URI}` (enable FRIGG_ROUTING_STATS=1), and prompt `{ROUTING_GUIDE_PROMPT_NAME}`.{playbook_guidance} {tool_surface_note}"
            )))
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
