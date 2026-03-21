use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::domain::model::{GeneratedStructuralFollowUp, ReferenceMatch, SymbolMatch};
use crate::domain::{ChannelResult, EvidenceChannel, FriggError, WorkloadPrecisionMode};
use crate::graph::{
    PreciseRelationshipKind, RelationKind, ScipIngestError, ScipResourceBudgets, SymbolGraph,
};
use crate::indexer::{
    FileMetadataDigest, HeuristicReference, HeuristicReferenceConfidence,
    HeuristicReferenceEvidence, HeuristicReferenceResolver, ManifestBuilder,
    ManifestDiagnosticKind, ReindexMode, SourceSpan, SymbolDefinition, SymbolExtractionOutput,
    byte_offset_for_line_column, extract_php_source_evidence_from_source,
    extract_symbols_for_paths, extract_symbols_from_source,
    generated_follow_up_structural_at_location_in_source, inspect_syntax_tree_in_source,
    inspect_syntax_tree_with_follow_up_in_source, navigation_symbol_target_rank,
    php_declaration_relation_edges_for_file, php_heuristic_implementation_candidates_for_target,
    register_symbol_definitions, reindex_repository_with_runtime_config,
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
use rmcp::model::{CallToolResult, Content, Implementation, Meta, ServerCapabilities, ServerInfo};
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
    CachedPreciseGeneratorProbe, CachedRepositoryResponseFreshness, CachedRepositorySummary,
    CachedSearchHybridResponse, CachedSearchSymbolResponse, CachedSearchTextResponse,
    CachedWorkspacePreciseGeneration, FileContentSnapshot, FileContentWindowCache,
    FileContentWindowCacheKey, FindDeclarationsResponseCacheKey, GoToDefinitionResponseCacheKey,
    HeuristicReferenceCacheKey, PreciseGeneratorProbeCacheKey, RepositoryFreshnessCacheScope,
    RepositoryResponseCacheFreshness, RepositoryResponseCacheFreshnessMode,
    RepositoryResponseFreshnessCacheKey, RuntimeCacheBudget, RuntimeCacheEvent, RuntimeCacheFamily,
    RuntimeCacheRegistry, RuntimeCacheTelemetry, SearchHybridResponseCacheKey,
    SearchSymbolResponseCacheKey, SearchTextResponseCacheKey, SessionResultHandleCache,
    SessionResultHandleEntry, WorkspaceSemanticRefreshPlan,
    response_cache_scopes_include_repository,
};
use crate::mcp::server_state::{
    CachedPreciseGraph, DeterministicSignatureHasher, DisambiguationRequiredSymbolTarget,
    ExploreExecution, FindReferencesExecution, FindReferencesResourceBudgets,
    NavigationTargetSelection, PreciseArtifactFailureSample, PreciseCoverageMode,
    PreciseGraphCacheKey, PreciseIngestStats, RankedSymbolMatch, ReadFileExecution,
    RepositoryDiagnosticsSummary, RepositorySymbolCorpus, ResolvedNavigationTarget,
    ResolvedSymbolTarget, RuntimeTaskRegistry, ScipArtifactDigest, ScipArtifactDiscovery,
    ScipArtifactFormat, ScipCandidateDirectoryDigest, SearchHybridExecution, SearchSymbolExecution,
    SearchTextExecution, SymbolCandidate, SymbolCorpusCacheKey,
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
    NavigationAvailability, NavigationLocation, NavigationMode, NavigationTargetSelectionStatus,
    NavigationTargetSelectionSummary, OutgoingCallsParams, OutgoingCallsResponse, ReadFileParams,
    ReadFileResponse, ReadMatchParams, ReadMatchResponse, ReadPresentationMode,
    RecentProvenanceSummary, RepositorySummary, ResponseMode, RuntimeStatusSummary,
    RuntimeTaskKind, RuntimeTaskStatus, RuntimeTaskSummary, SearchHybridChannelWeightsParams,
    SearchHybridMatch, SearchHybridParams, SearchHybridResponse, SearchPatternType,
    SearchStructuralParams, SearchStructuralResponse, SearchSymbolParams, SearchSymbolPathClass,
    SearchSymbolResponse, SearchTextParams, SearchTextResponse, SyntaxTreeNodeItem,
    WRITE_CONFIRM_PARAM, WRITE_CONFIRMATION_REQUIRED_ERROR_CODE, WorkspaceAttachAction,
    WorkspaceAttachParams, WorkspaceAttachResponse, WorkspaceCurrentParams,
    WorkspaceCurrentResponse, WorkspaceDetachParams, WorkspaceDetachResponse,
    WorkspaceIndexComponentState, WorkspaceIndexComponentSummary, WorkspaceIndexHealthSummary,
    WorkspacePreciseArtifactFailureSummary, WorkspacePreciseCoverageMode,
    WorkspacePreciseGenerationAction, WorkspacePreciseGenerationStatus,
    WorkspacePreciseGenerationSummary, WorkspacePreciseGeneratorState,
    WorkspacePreciseGeneratorSummary, WorkspacePreciseIngestState, WorkspacePreciseIngestSummary,
    WorkspacePreciseLifecyclePhase, WorkspacePreciseLifecycleSummary, WorkspacePreciseSummary,
    WorkspacePrepareParams, WorkspacePrepareResponse, WorkspaceReindexParams,
    WorkspaceReindexResponse, WorkspaceResolveMode, WorkspaceStorageIndexState,
    WorkspaceStorageSummary,
};
use crate::mcp::workspace_registry::{AttachedWorkspace, WorkspaceRegistry};
use crate::settings::RuntimeProfile;

mod content;
mod deep_search;
mod errors;
mod execution;
mod navigation_cache;
mod navigation_metadata;
mod navigation_precise;
mod navigation_resolution;
mod navigation_tools;
mod precise_graph;
mod presentation;
mod provenance;
mod runtime_cache;
mod runtime_status;
mod search_tools;
mod symbol_index;
mod workspace;
mod workspace_session;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SymbolCorpusBenchmarkSummary {
    pub repository_count: usize,
    pub source_file_count: usize,
    pub symbol_count: usize,
    pub php_evidence_files: usize,
    pub blade_evidence_files: usize,
}

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

#[derive(Clone)]
/// Orchestrates Frigg's public MCP tool surface over shared config, caches, session state,
/// provenance, and optional watch-backed freshness.
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
    result_handles: RwLock<SessionResultHandleCache>,
}

#[derive(Clone)]
struct FriggMcpCacheState {
    symbol_corpus_cache: Arc<RwLock<BTreeMap<SymbolCorpusCacheKey, Arc<RepositorySymbolCorpus>>>>,
    precise_graph_cache: Arc<RwLock<BTreeMap<PreciseGraphCacheKey, Arc<CachedPreciseGraph>>>>,
    latest_precise_graph_cache: Arc<RwLock<BTreeMap<String, Arc<CachedPreciseGraph>>>>,
    provenance_storage_cache: Arc<RwLock<BTreeMap<ProvenanceStorageCacheKey, Arc<Storage>>>>,
    repository_response_freshness_cache: Arc<
        RwLock<BTreeMap<RepositoryResponseFreshnessCacheKey, CachedRepositoryResponseFreshness>>,
    >,
    precise_generator_probe_cache:
        Arc<RwLock<BTreeMap<PreciseGeneratorProbeCacheKey, CachedPreciseGeneratorProbe>>>,
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
    const REPOSITORY_RESPONSE_FRESHNESS_CACHE_TTL: Duration = Duration::from_secs(2);
    const REPOSITORY_RESPONSE_FRESHNESS_CACHE_MAX_ENTRIES: usize = 64;
    const PRECISE_GENERATOR_PROBE_CACHE_TTL: Duration = Duration::from_secs(30);
    const PRECISE_GENERATOR_PROBE_CACHE_MAX_ENTRIES: usize = 128;
    const PROVENANCE_STORAGE_CACHE_MAX_ENTRIES: usize = 32;
    const SESSION_RESULT_HANDLE_TTL: Duration = Duration::from_secs(300);
    const SESSION_RESULT_HANDLE_MAX_ENTRIES: usize = 64;
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

    #[allow(clippy::too_many_arguments)]
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
                repository_response_freshness_cache: Arc::new(RwLock::new(BTreeMap::new())),
                precise_generator_probe_cache: Arc::new(RwLock::new(BTreeMap::new())),
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
}

#[tool_router(router = tool_router)]
impl FriggMcpServer {
    #[tool(
        name = "list_repositories",
        description = "List globally known repositories and their session, watch, and index state.",
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
        description = "Adopt a repository into this session. Use this before repo-aware tools when detached or when you want a stable default repository.",
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
        let wait_for_precise = params.wait_for_precise.unwrap_or(false);
        let started_at = Instant::now();
        info!(
            requested_path = params.path.as_deref().unwrap_or(""),
            requested_repository_id = params.repository_id.as_deref().unwrap_or(""),
            set_default,
            resolve_mode = ?resolve_mode,
            "workspace attach started"
        );
        let mut response = match self.attach_workspace_target_internal(
            params.path.as_deref(),
            params.repository_id.as_deref(),
            set_default,
            resolve_mode,
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
        if wait_for_precise {
            let repository_id = response.repository.repository_id.clone();
            let completed = self
                .wait_for_repository_precise_generation(&repository_id, Duration::from_secs(30))
                .await;
            if let Some(workspace) = self.workspace_by_repository_id(&repository_id) {
                let mut repository = self.repository_summary(&workspace);
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
        info!(
            repository_id = %response.repository.repository_id,
            root = %response.repository.root_path,
            action = ?response.action,
            resolution = ?response.resolution,
            session_default = response.session_default,
            precise_state = ?response.precise.state,
            precise_generation_action = ?response.precise.generation_action,
            precise_phase = ?response.precise_lifecycle.phase,
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
                    "wait_for_precise": params.wait_for_precise,
                }),
                finalization.source_refs,
                &result,
            )
            .await;
        self.finalize_with_provenance("workspace_attach", result, provenance_result)
    }

    #[tool(
        name = "workspace_detach",
        description = "Remove a repository from this session and release its watch lease when applicable.",
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
        self.invalidate_repository_symbol_corpus_cache(&workspace.repository_id);
        self.invalidate_repository_summary_cache(&workspace.repository_id);
        self.invalidate_repository_response_freshness_cache(&workspace.repository_id);
        self.invalidate_repository_file_content_cache(&workspace.repository_id);
        self.invalidate_repository_precise_generator_probe_cache(&workspace.repository_id);
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
        description = "Initialize or verify Frigg state for a repository, then adopt it into this session. Requires confirmation.",
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
        let started_at = Instant::now();
        let (workspace, resolved_from, resolution) = self.resolve_workspace_target(
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
        if self.repository_has_active_runtime_work(&workspace.repository_id) {
            warn!(
                repository_id = %workspace.repository_id,
                root = %workspace.root.display(),
                duration_ms = started_at.elapsed().as_millis() as u64,
                "workspace prepare rejected because runtime work is active"
            );
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
            warn!(
                repository_id = %workspace.repository_id,
                root = %workspace.root.display(),
                duration_ms = started_at.elapsed().as_millis() as u64,
                error = %err,
                "workspace prepare failed during storage initialization"
            );
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
        self.invalidate_repository_symbol_corpus_cache(&workspace.repository_id);
        self.invalidate_repository_summary_cache(&workspace.repository_id);
        self.invalidate_repository_response_freshness_cache(&workspace.repository_id);
        self.invalidate_repository_file_content_cache(&workspace.repository_id);
        self.invalidate_repository_precise_generator_probe_cache(&workspace.repository_id);
        self.invalidate_repository_precise_graph_caches(&workspace.repository_id);
        self.invalidate_repository_search_response_caches(&workspace.repository_id);
        self.invalidate_repository_navigation_response_caches(&workspace.repository_id);

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
        description = "Refresh indexed state for a repository, then adopt it into this session. Requires confirmation.",
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
        let started_at = Instant::now();
        let (workspace, resolved_from, resolution) = self.resolve_workspace_target(
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
            "workspace reindex started"
        );
        if self.repository_has_active_runtime_work(&workspace.repository_id) {
            warn!(
                repository_id = %workspace.repository_id,
                root = %workspace.root.display(),
                duration_ms = started_at.elapsed().as_millis() as u64,
                "workspace reindex rejected because runtime work is active"
            );
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
                    &workspace.runtime_repository_id,
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
            warn!(
                repository_id = %workspace.repository_id,
                root = %workspace.root.display(),
                duration_ms = started_at.elapsed().as_millis() as u64,
                error = %err,
                "workspace reindex failed during lexical refresh"
            );
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
        self.invalidate_repository_symbol_corpus_cache(&workspace.repository_id);
        self.invalidate_repository_summary_cache(&workspace.repository_id);
        self.invalidate_repository_response_freshness_cache(&workspace.repository_id);
        self.invalidate_repository_file_content_cache(&workspace.repository_id);
        self.invalidate_repository_precise_generator_probe_cache(&workspace.repository_id);
        self.scip_invalidate_repository_precise_generation_cache(&workspace.repository_id);
        self.invalidate_repository_precise_graph_caches(&workspace.repository_id);
        self.invalidate_repository_search_response_caches(&workspace.repository_id);
        self.invalidate_repository_navigation_response_caches(&workspace.repository_id);

        Self::notify_progress(&meta, &client, 3.0, 4.0, "finalize").await;
        self.adopt_workspace(&workspace, set_default)
            .inspect_err(|error| {
                warn!(
                    repository_id = %workspace.repository_id,
                    root = %workspace.root.display(),
                    duration_ms = started_at.elapsed().as_millis() as u64,
                    error = %error.message,
                    "workspace reindex failed while adopting workspace"
                );
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
        let precise_generation_action = self.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &reindex_summary.changed_paths,
            &reindex_summary.deleted_paths,
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
        let mut repository = self.repository_summary(&workspace);
        let storage = repository
            .storage
            .clone()
            .unwrap_or_else(|| Self::workspace_storage_summary(&workspace));
        repository.storage = None;
        let mut response = WorkspaceReindexResponse {
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
            precise_lifecycle,
        };
        if params.wait_for_precise.unwrap_or(false) {
            let repository_id = response.repository.repository_id.clone();
            let completed = self
                .wait_for_repository_precise_generation(&repository_id, Duration::from_secs(30))
                .await;
            if let Some(workspace) = self.workspace_by_repository_id(&repository_id) {
                let generation_action = response.precise_lifecycle.generation_action;
                let precise = self
                    .workspace_precise_summary_for_workspace(&workspace, Some(generation_action));
                let mut repository = self.repository_summary(&workspace);
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
            "workspace reindex completed"
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
                "precise_lifecycle": {
                    "phase": response.precise_lifecycle.phase,
                    "waited_for_completion": response.precise_lifecycle.waited_for_completion,
                    "generation_action": response.precise_lifecycle.generation_action,
                },
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
                    "wait_for_precise": params.wait_for_precise,
                }),
                finalization.source_refs,
                &result,
            )
            .await;
        self.finalize_with_provenance("workspace_reindex", result, provenance_result)
    }

    #[tool(
        name = "workspace_current",
        description = "Inspect the session default, adopted repositories, runtime tasks, and compact precise/index status.",
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
            .map(|workspace| self.repository_summary(workspace));
        let repositories = self
            .attached_workspaces()
            .into_iter()
            .map(|workspace| self.repository_summary(&workspace))
            .collect::<Vec<_>>();
        let runtime = self.runtime_status_summary();
        let precise = current_workspace
            .as_ref()
            .map(|workspace| self.workspace_precise_summary_for_workspace(workspace, None));
        let precise_ingest = current_repository
            .as_ref()
            .and_then(|repository| repository.health.as_ref())
            .and_then(|health| health.precise_ingest.clone());
        let response = WorkspaceCurrentResponse {
            repository: current_repository,
            session_default: current_workspace.is_some(),
            repositories,
            precise,
            precise_ingest,
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
        description = "Read a bounded slice of a repository file when you already know the canonical path. Use read_match to reopen a prior search or navigation hit by handle.",
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
        description = "Open a bounded source window around a prior search or navigation hit using its session result_handle and match_id.",
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
        description = "Probe, zoom, or refine within one repository file after discovery.",
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
        description = "Use exact literal or regex search when you know the text and need repository scoping or path_regex narrowing. Use context_lines, max_matches_per_file, or collapse_by_file to keep first-pass review compact.",
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
        description = "Use for broad repository discovery. If semantic is unavailable, treat broad natural-language ranking as weaker and pivot to exact tools sooner.",
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
        description = "Use when you know the symbol name and need repository-aware lookup. Add path_class or path_regex to reduce overload noise.",
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
        description = "Find usage sites for a symbol or cursor location. Check mode and match_kind; set include_definition=false to hide the defining row, or include_follow_up_structural=true for replayable structural follow-ups on anchored matches.",
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
        description = "Jump from a symbol or cursor location to likely definitions. Check mode before treating the result as precise; set include_follow_up_structural=true for replayable structural follow-ups on anchored matches.",
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
        description = "Find declaration anchors for a symbol or cursor location. Check mode before treating the result as precise; set include_follow_up_structural=true for replayable structural follow-ups on anchored matches.",
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
        description = "Find implementing types or members for a symbol or cursor location. Check mode and per-match precision hints when results underfill; set include_follow_up_structural=true for replayable structural follow-ups on anchored matches.",
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
        description = "Find callers for a callable symbol or cursor location. Check availability before treating empty results as meaningful; set include_follow_up_structural=true for replayable structural follow-ups on anchored matches.",
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
        description = "Find callees for a callable symbol or cursor location. Check availability before treating empty results as meaningful; set include_follow_up_structural=true for replayable structural follow-ups on anchored matches.",
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
        description = "Return a symbol outline for one supported source file. Use top_level_only=true for a cheap first pass, or include_follow_up_structural=true for replayable structural follow-ups on anchored symbols.",
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
        description = "Inspect the AST around a source location before writing or debugging a search_structural query. Set include_follow_up_structural=true for replayable structural follow-ups derived from the resolved AST focus.",
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
        description = "Run Tree-sitter queries when syntax shape matters more than text or symbols. Grouped match rows are the default; use inspect_syntax_tree first when the node shape is unclear, use primary_capture to choose the visible anchor, and use result_mode=captures for raw capture debugging.",
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
        description = "Run a trace-oriented deep-search playbook and return the resulting trace artifact.",
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
        description = "Replay a deep-search playbook against an expected trace artifact and report whether it still matches.",
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
        description = "Compose citation payloads from an existing deep-search trace artifact.",
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
                    "Start with list_repositories. If the session is detached, call workspace_attach explicitly. Use workspace_current for repository health, precise status, and runtime task status. Prefer shell tools for cheap local reads and literal scans. Read-only MCP tools default to compact responses; request response_mode=full only when you need diagnostics, freshness detail, or selection notes. Search and navigation results now return result_handle plus per-row match_id values, and read_match reopens a bounded source window around one prior hit. `read_file`, `read_match`, and `explore(operation=zoom)` are text-first by default; request presentation_mode=json only when a downstream consumer needs the structured compatibility payload. `explore(operation=probe|refine)` stays structured by default. Use search_hybrid for broad discovery, then pivot to search_symbol, search_text, navigation tools, read_match, or read_file once you have a concrete anchor. If search_hybrid reports lexical_only_mode or non-ok semantic status, treat broad natural-language ranking as weaker evidence and use exact tools sooner. Use top_level_only=true on document_symbols for a cheap first outline, and use include_follow_up_structural=true on inspect_syntax_tree, search_structural, or anchored navigation and outline tools when you want replayable search_structural follow-ups derived from the resolved AST focus. If the extended profile is enabled, use explore for bounded follow-up inside one file and deep-search tools only for explicit trace workflows. Runtime tool-surface profile is `{tool_surface_profile}`; set `{TOOL_SURFACE_PROFILE_ENV}=extended` to expose explore and deep-search tools. Runtime profile is `{runtime_profile}`. Policy resources remain available at `{SUPPORT_MATRIX_RESOURCE_URI}`, `{TOOL_SURFACE_RESOURCE_URI}`, and `{SHELL_GUIDANCE_RESOURCE_URI}`. Prompt guidance is available via `{ROUTING_GUIDE_PROMPT_NAME}`."
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
