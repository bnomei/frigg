use std::collections::BTreeMap;

use crate::domain::{
    ChannelHealthStatus, EvidenceAnchor, PathClass, SourceClass,
    model::{ReferenceMatch, SymbolMatch, TextMatch},
};
use crate::settings::RuntimeProfile;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PUBLIC_TOOL_NAMES: [&str; 22] = [
    "list_repositories",
    "workspace_attach",
    "workspace_detach",
    "workspace_prepare",
    "workspace_reindex",
    "workspace_current",
    "read_file",
    "explore",
    "search_text",
    "search_hybrid",
    "search_symbol",
    "find_references",
    "go_to_definition",
    "find_declarations",
    "find_implementations",
    "incoming_calls",
    "outgoing_calls",
    "document_symbols",
    "search_structural",
    "deep_search_run",
    "deep_search_replay",
    "deep_search_compose_citations",
];
pub const PUBLIC_READ_ONLY_TOOL_NAMES: [&str; 18] = [
    "list_repositories",
    "workspace_current",
    "read_file",
    "explore",
    "search_text",
    "search_hybrid",
    "search_symbol",
    "find_references",
    "go_to_definition",
    "find_declarations",
    "find_implementations",
    "incoming_calls",
    "outgoing_calls",
    "document_symbols",
    "search_structural",
    "deep_search_run",
    "deep_search_replay",
    "deep_search_compose_citations",
];
pub const PUBLIC_SESSION_STATEFUL_TOOL_NAMES: [&str; 2] = ["workspace_attach", "workspace_detach"];
pub const PUBLIC_WRITE_TOOL_NAMES: [&str; 2] = ["workspace_prepare", "workspace_reindex"];
pub const WRITE_CONFIRM_PARAM: &str = "confirm";
pub const WRITE_CONFIRMATION_REQUIRED_ERROR_CODE: &str = "confirmation_required";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RepositorySessionSummary {
    pub adopted: bool,
    pub active_session_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RepositoryWatchSummary {
    pub active: bool,
    pub lease_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RepositorySummary {
    pub repository_id: String,
    pub display_name: String,
    pub root_path: String,
    pub session: RepositorySessionSummary,
    pub watch: RepositoryWatchSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<WorkspaceStorageSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<WorkspaceIndexHealthSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListRepositoriesResponse {
    pub repositories: Vec<RepositorySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ListRepositoriesParams {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceResolveMode {
    GitRoot,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStorageIndexState {
    MissingDb,
    Uninitialized,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceStorageSummary {
    pub db_path: String,
    pub exists: bool,
    pub initialized: bool,
    pub index_state: WorkspaceStorageIndexState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceIndexComponentState {
    Missing,
    Ready,
    Stale,
    Disabled,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceIndexComponentSummary {
    pub state: WorkspaceIndexComponentState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatible_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceIndexHealthSummary {
    pub lexical: WorkspaceIndexComponentSummary,
    pub semantic: WorkspaceIndexComponentSummary,
    pub scip: WorkspaceIndexComponentSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceAttachParams {
    /// File or directory path to attach. Relative paths resolve against the Frigg server process cwd.
    pub path: Option<String>,
    /// Known repository identifier from `list_repositories`.
    pub repository_id: Option<String>,
    /// Whether to make the attached repository the session default. Omit to default to `true`.
    pub set_default: Option<bool>,
    /// Workspace resolution strategy. Omit to prefer the enclosing Git root before falling back to the direct directory.
    pub resolve_mode: Option<WorkspaceResolveMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceAttachResponse {
    pub repository: RepositorySummary,
    pub resolved_from: String,
    pub resolution: WorkspaceResolveMode,
    pub session_default: bool,
    pub storage: WorkspaceStorageSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceDetachParams {
    /// Repository identifier to detach. Omit to detach the current session-default repository.
    pub repository_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceDetachResponse {
    pub repository_id: String,
    pub session_default: bool,
    pub detached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePrepareParams {
    /// File or directory path to prepare. Relative paths resolve against the Frigg server process cwd.
    pub path: Option<String>,
    /// Known repository identifier from `list_repositories`.
    pub repository_id: Option<String>,
    /// Whether to make the prepared repository the session default. Omit to default to `true`.
    pub set_default: Option<bool>,
    /// Workspace resolution strategy when using `path`. Omit to prefer the enclosing Git root before falling back to the direct directory.
    pub resolve_mode: Option<WorkspaceResolveMode>,
    /// Explicit confirmation required before Frigg writes `.frigg/` state or updates storage.
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePrepareResponse {
    pub repository: RepositorySummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<WorkspaceResolveMode>,
    pub session_default: bool,
    pub storage: WorkspaceStorageSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceReindexParams {
    /// File or directory path to reindex. Relative paths resolve against the Frigg server process cwd.
    pub path: Option<String>,
    /// Known repository identifier from `list_repositories`.
    pub repository_id: Option<String>,
    /// Whether to make the reindexed repository the session default. Omit to default to `true`.
    pub set_default: Option<bool>,
    /// Workspace resolution strategy when using `path`. Omit to prefer the enclosing Git root before falling back to the direct directory.
    pub resolve_mode: Option<WorkspaceResolveMode>,
    /// Explicit confirmation required before Frigg updates storage.
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceReindexResponse {
    pub repository: RepositorySummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<WorkspaceResolveMode>,
    pub session_default: bool,
    pub storage: WorkspaceStorageSummary,
    pub snapshot_id: String,
    pub files_scanned: usize,
    pub files_changed: usize,
    pub files_deleted: usize,
    pub diagnostics_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct WorkspaceCurrentParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceCurrentResponse {
    pub repository: Option<RepositorySummary>,
    pub session_default: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<RepositorySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeStatusSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTaskKind {
    ChangedReindex,
    SemanticRefresh,
    PrecisePrewarm,
    WorkspacePrepare,
    WorkspaceReindex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTaskStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeTaskSummary {
    pub task_id: String,
    pub kind: RuntimeTaskKind,
    pub status: RuntimeTaskStatus,
    pub repository_id: String,
    pub phase: String,
    pub created_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecentProvenanceSummary {
    pub trace_id: String,
    pub tool_name: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeStatusSummary {
    pub profile: RuntimeProfile,
    pub persistent_state_available: bool,
    pub watch_active: bool,
    pub tool_surface_profile: String,
    pub status_tool: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_tasks: Vec<RuntimeTaskSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_tasks: Vec<RuntimeTaskSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_provenance: Vec<RecentProvenanceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileParams {
    pub path: String,
    pub repository_id: Option<String>,
    pub max_bytes: Option<usize>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileResponse {
    pub repository_id: String,
    pub path: String,
    pub bytes: usize,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchPatternType {
    Literal,
    Regex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExploreOperation {
    Probe,
    Zoom,
    Refine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreAnchor {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreCursor {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreLineWindow {
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreWindow {
    pub start_line: usize,
    pub end_line: usize,
    pub bytes: usize,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreMatch {
    pub match_id: String,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub excerpt: String,
    pub window: ExploreWindow,
    pub anchor: ExploreAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreMetadata {
    pub lossy_utf8: bool,
    pub effective_context_lines: usize,
    pub effective_max_matches: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreParams {
    /// Artifact path using the same canonical repository-relative semantics as `read_file`.
    pub path: String,
    /// Optional repository scope from `list_repositories`.
    pub repository_id: Option<String>,
    /// Explorer mode: `probe` scans an artifact, `zoom` returns a bounded window, and `refine` searches only inside an anchor-derived window.
    pub operation: ExploreOperation,
    /// Search query for `probe` or `refine`. Leading and trailing whitespace is trimmed.
    pub query: Option<String>,
    /// Match mode for `query`. Omit for exact literal search or set `regex` for safe-regex search.
    pub pattern_type: Option<SearchPatternType>,
    /// Explicit anchor used by `zoom` and `refine`.
    pub anchor: Option<ExploreAnchor>,
    /// Context lines to include around anchors and match windows. Omit to use the explorer default.
    pub context_lines: Option<usize>,
    /// Max match rows to return. Omit to use the explorer default.
    pub max_matches: Option<usize>,
    /// Explicit continuation cursor for `probe` or `refine`.
    pub resume_from: Option<ExploreCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreResponse {
    pub repository_id: String,
    pub path: String,
    pub operation: ExploreOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern_type: Option<SearchPatternType>,
    pub total_lines: usize,
    pub scan_scope: ExploreLineWindow,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<ExploreWindow>,
    pub total_matches: usize,
    pub matches: Vec<ExploreMatch>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_from: Option<ExploreCursor>,
    pub metadata: ExploreMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextParams {
    /// Text or regex pattern to search for. Leading and trailing whitespace is trimmed.
    pub query: String,
    /// Match mode for `query`. Omit for exact literal search or set `regex` for safe-regex search.
    pub pattern_type: Option<SearchPatternType>,
    /// Optional repository scope from `list_repositories`.
    pub repository_id: Option<String>,
    /// Optional safe regex over canonical repository-relative paths.
    /// Use this to narrow code, docs, or runtime slices.
    pub path_regex: Option<String>,
    /// Optional max matches. Frigg clamps the effective limit to the server search budget.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextResponse {
    pub total_matches: usize,
    pub matches: Vec<TextMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridChannelWeightsParams {
    pub lexical: Option<f32>,
    pub graph: Option<f32>,
    pub semantic: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridParams {
    /// Broad natural-language or exact-phrase query for doc/runtime retrieval.
    /// Pivot to `search_symbol` or scoped `search_text.path_regex` for concrete anchors.
    pub query: String,
    /// Optional repository scope from `list_repositories`. Omit to search every configured repository.
    pub repository_id: Option<String>,
    /// Optional language filter for source-backed follow-up, for example `rust` when runtime files should outrank docs-only evidence.
    pub language: Option<String>,
    /// Optional max matches. Frigg clamps the effective limit to the server search budget.
    pub limit: Option<usize>,
    /// Optional channel-weight overrides when a client needs deterministic lexical/graph/semantic tradeoffs.
    pub weights: Option<SearchHybridChannelWeightsParams>,
    /// Optional semantic-channel toggle. Omit to use the active runtime configuration.
    pub semantic: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridMatch {
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub excerpt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<EvidenceAnchor>,
    pub blended_score: f32,
    pub lexical_score: f32,
    pub graph_score: f32,
    pub semantic_score: f32,
    pub lexical_sources: Vec<String>,
    pub graph_sources: Vec<String>,
    pub semantic_sources: Vec<String>,
    /// Additive generic path-class hint for clients choosing a first navigation pivot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_class: Option<PathClass>,
    /// Additive generic source-class hint derived from shared runtime/support/project classification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_class: Option<SourceClass>,
    /// Additive generic surface-family hints such as `runtime`, `tests`, or `entrypoint`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_families: Vec<String>,
    /// Additive live-navigation hint describing whether this match is a good pivot for follow-up tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub navigation_hint: Option<SearchHybridNavigationHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridNavigationHint {
    /// True when the match is a reasonable first pivot for `read_file` or symbol follow-up.
    pub pivotable: bool,
    /// True when `document_symbols` is expected to be useful on this path.
    pub document_symbols: bool,
    /// True when symbol/anchor follow-up is likely to support `go_to_definition`.
    pub go_to_definition: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridUtilitySummary {
    /// Count of returned matches that look like useful live-navigation pivots.
    pub pivotable_match_count: usize,
    /// One-based rank of the best generic pivot inside the returned result set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pivot_rank: Option<usize>,
    /// Canonical path of the best generic pivot inside the returned result set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pivot_path: Option<String>,
    /// Repository id for the best generic pivot when cross-repository search is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pivot_repository_id: Option<String>,
    /// True when the returned set contains at least one pivot that likely supports symbol follow-up.
    pub symbol_navigation_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridChannelDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridChannelMetadata {
    pub status: ChannelHealthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub candidate_count: usize,
    pub hit_count: usize,
    pub match_count: usize,
    pub diagnostic_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<SearchHybridChannelDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridDiagnosticsSummary {
    pub walk: usize,
    pub read: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridStageSample {
    pub elapsed_us: u64,
    pub input_count: usize,
    pub output_count: usize,
}

impl From<&crate::searcher::SearchStageSample> for SearchHybridStageSample {
    fn from(value: &crate::searcher::SearchStageSample) -> Self {
        Self {
            elapsed_us: value.elapsed_us,
            input_count: value.input_count,
            output_count: value.output_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridStageAttribution {
    pub candidate_intake: SearchHybridStageSample,
    pub freshness_validation: SearchHybridStageSample,
    pub scan: SearchHybridStageSample,
    pub witness_scoring: SearchHybridStageSample,
    pub graph_expansion: SearchHybridStageSample,
    pub semantic_retrieval: SearchHybridStageSample,
    pub anchor_blending: SearchHybridStageSample,
    pub document_aggregation: SearchHybridStageSample,
    pub final_diversification: SearchHybridStageSample,
}

impl From<&crate::searcher::SearchStageAttribution> for SearchHybridStageAttribution {
    fn from(value: &crate::searcher::SearchStageAttribution) -> Self {
        Self {
            candidate_intake: SearchHybridStageSample::from(&value.candidate_intake),
            freshness_validation: SearchHybridStageSample::from(&value.freshness_validation),
            scan: SearchHybridStageSample::from(&value.scan),
            witness_scoring: SearchHybridStageSample::from(&value.witness_scoring),
            graph_expansion: SearchHybridStageSample::from(&value.graph_expansion),
            semantic_retrieval: SearchHybridStageSample::from(&value.semantic_retrieval),
            anchor_blending: SearchHybridStageSample::from(&value.anchor_blending),
            document_aggregation: SearchHybridStageSample::from(&value.document_aggregation),
            final_diversification: SearchHybridStageSample::from(&value.final_diversification),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResponseFreshnessRepositoryMetadata {
    pub repository_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    pub manifest: String,
    pub semantic: String,
    pub dirty_root: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResponseFreshnessBasisMetadata {
    pub mode: String,
    pub cacheable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<ResponseFreshnessRepositoryMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridSemanticAcceleratorMetadata {
    pub tier: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ChannelHealthStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridLanguageCapabilityMetadata {
    pub requested_language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub semantic_chunking: String,
    pub semantic_accelerator: SearchHybridSemanticAcceleratorMetadata,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub capabilities: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridMetadata {
    pub channels: BTreeMap<String, SearchHybridChannelMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_requested: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_status: Option<ChannelHealthStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_candidate_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_hit_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_match_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    pub diagnostics_count: usize,
    pub diagnostics: SearchHybridDiagnosticsSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_attribution: Option<SearchHybridStageAttribution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_capability: Option<SearchHybridLanguageCapabilityMetadata>,
    /// Additive utility summary for discovery-to-navigation workflows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utility: Option<SearchHybridUtilitySummary>,
    pub freshness_basis: ResponseFreshnessBasisMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridResponse {
    pub matches: Vec<SearchHybridMatch>,
    /// Legacy top-level compatibility mirror of `metadata.semantic_requested`; live responses may omit this when structured metadata is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_requested: Option<bool>,
    /// Legacy top-level compatibility mirror of `metadata.semantic_enabled`; live responses may omit this when structured metadata is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_enabled: Option<bool>,
    /// Legacy top-level compatibility mirror of `metadata.semantic_status`; live responses may omit this when structured metadata is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_status: Option<ChannelHealthStatus>,
    /// Legacy top-level compatibility mirror of `metadata.semantic_reason`; live responses may omit this when structured metadata is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_reason: Option<String>,
    /// Legacy top-level compatibility mirror of `metadata.semantic_hit_count`; live responses may omit this when structured metadata is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_hit_count: Option<usize>,
    /// Legacy top-level compatibility mirror of `metadata.semantic_match_count`; live responses may omit this when structured metadata is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_match_count: Option<usize>,
    /// Legacy top-level compatibility mirror of `metadata.warning`; live responses may omit this when structured metadata is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Canonical structured multi-channel diagnostics payload for live responses; includes `channels` plus flat semantic compatibility mirrors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SearchHybridMetadata>,
    /// Legacy derived human-readable note; machine clients should read `metadata` for structured meaning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchSymbolParams {
    /// API/type/function name to search in indexed symbols.
    /// Use this after `search_hybrid` or `search_text` when you know the runtime anchor.
    pub query: String,
    /// Optional repository scope from `list_repositories`. Omit to search every configured repository.
    pub repository_id: Option<String>,
    /// Optional path class filter. Use `runtime` for `src/`, `support`
    /// for `tests/`, `benches/`, or `examples/`, or `project` for everything else.
    pub path_class: Option<SearchSymbolPathClass>,
    /// Optional safe regex over canonical repository-relative symbol paths.
    /// Use this to constrain overloaded names to a file or slice.
    pub path_regex: Option<String>,
    /// Optional max matches. Frigg clamps the effective limit to the server search budget.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchSymbolResponse {
    pub matches: Vec<SymbolMatch>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchSymbolPathClass {
    Runtime,
    Project,
    Support,
}

impl SearchSymbolPathClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Project => "project",
            Self::Support => "support",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindReferencesParams {
    /// Optional symbol query. Omit when resolving the target by source location.
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    /// Optional source path used for deterministic location-aware target resolution.
    pub path: Option<String>,
    /// Optional 1-based line used for deterministic location-aware target resolution.
    pub line: Option<usize>,
    /// Optional 1-based column used for deterministic location-aware target resolution.
    pub column: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindReferencesResponse {
    pub total_matches: usize,
    pub matches: Vec<ReferenceMatch>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoToDefinitionParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigationLocation {
    pub symbol: String,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub kind: Option<String>,
    pub precision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoToDefinitionResponse {
    pub matches: Vec<NavigationLocation>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindDeclarationsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindDeclarationsResponse {
    pub matches: Vec<NavigationLocation>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindImplementationsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImplementationMatch {
    pub symbol: String,
    pub kind: Option<String>,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub relation: Option<String>,
    pub precision: Option<String>,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindImplementationsResponse {
    pub matches: Vec<ImplementationMatch>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IncomingCallsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutgoingCallsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CallHierarchyMatch {
    pub source_symbol: String,
    pub target_symbol: String,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub relation: String,
    pub precision: Option<String>,
    pub call_path: Option<String>,
    pub call_line: Option<usize>,
    pub call_column: Option<usize>,
    pub call_end_line: Option<usize>,
    pub call_end_column: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IncomingCallsResponse {
    pub matches: Vec<CallHierarchyMatch>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutgoingCallsResponse {
    pub matches: Vec<CallHierarchyMatch>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolsParams {
    pub path: String,
    pub repository_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolItem {
    pub symbol: String,
    pub kind: String,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub container: Option<String>,
    pub children: Vec<DocumentSymbolItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolsResponse {
    pub symbols: Vec<DocumentSymbolItem>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchStructuralParams {
    pub query: String,
    pub language: Option<String>,
    pub repository_id: Option<String>,
    pub path_regex: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StructuralMatch {
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchStructuralResponse {
    pub matches: Vec<StructuralMatch>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[path = "types/deep_search.rs"]
mod deep_search;
pub use deep_search::*;

#[cfg(test)]
#[path = "types/schema_tests.rs"]
mod schema_tests;
