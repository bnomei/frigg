use crate::domain::{
    EvidenceAnchor,
    model::{ReferenceMatch, SymbolMatch, TextMatch},
};
use crate::settings::RuntimeProfile;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PUBLIC_READ_ONLY_TOOL_NAMES: [&str; 19] = [
    "list_repositories",
    "workspace_attach",
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
pub const WRITE_CONFIRM_PARAM: &str = "confirm";
pub const WRITE_CONFIRMATION_REQUIRED_ERROR_CODE: &str = "confirmation_required";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RepositorySummary {
    pub repository_id: String,
    pub display_name: String,
    pub root_path: String,
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
    pub path: String,
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
    pub semantic_status: Option<String>,
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
    pub metadata: Option<Value>,
    /// Legacy JSON-encoded compatibility metadata; live responses may omit this when structured metadata is present.
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
