//! Workspace lifecycle MCP wire types: attach, prepare, index, runtime tasks, and precise status.

use crate::settings::RuntimeProfile;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::repository::{
    RepositorySummary, WorkspacePreciseIngestSummary, WorkspaceResolveMode, WorkspaceStorageSummary,
};
use super::{ContextEfficiencyMetadata, ReadPresentationMode};

/// Availability of one configured SCIP generator for precise-artifact generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseGeneratorState {
    Available,
    MissingTool,
    Unsupported,
    NotConfigured,
    Error,
}

/// Outcome of one precise-artifact generation attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseGenerationStatus {
    Succeeded,
    Failed,
    Skipped,
    MissingTool,
    Unsupported,
    NotConfigured,
    Timeout,
}

/// Failure category for a precise generator or ingest step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseFailureClass {
    MissingTool,
    ToolPanic,
    ToolTimeout,
    ToolEnvFailure,
    ToolInvalidOutput,
    ToolFailed,
}

/// Suggested next step when precise generation or ingest is degraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRecommendedAction {
    InstallTool,
    RerunIndex,
    CheckEnvironment,
    UpstreamToolFailure,
    UseHeuristicMode,
}

/// Session gate action for `workspace` status responses.
///
/// Distinct from [`WorkspaceRecommendedAction`], which is precise-generation oriented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceGateAction {
    Ready,
    AdoptRepo,
    WaitWatch,
    Reindex,
    UseLiveDiskForTouchedFiles,
    FriggUnavailable,
}

/// Operator/agent-facing copy for non-obvious [`WorkspaceGateAction`] values.
///
/// Public MCP has no reindex/write tool (`PUBLIC_WRITE_TOOL_NAMES` is empty). When the gate
/// returns `reindex`, agents must use CLI `frigg index` / operator lifecycle — not invent an MCP tool.
pub fn workspace_gate_hint(action: WorkspaceGateAction) -> Option<String> {
    match action {
        WorkspaceGateAction::Reindex => Some(
            "Index not ready. Run CLI `frigg index` (or operator lifecycle); there is no public MCP reindex tool."
                .to_owned(),
        ),
        // Ready / adopt / live-disk / wait_watch are self-explanatory or covered by skill cards.
        WorkspaceGateAction::Ready
        | WorkspaceGateAction::AdoptRepo
        | WorkspaceGateAction::WaitWatch
        | WorkspaceGateAction::UseLiveDiskForTouchedFiles
        | WorkspaceGateAction::FriggUnavailable => None,
    }
}

/// Aggregate precise readiness reported by workspace lifecycle tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseState {
    Ok,
    Partial,
    Failed,
    Unavailable,
}

/// Whether attach or index triggered precise-artifact generation work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseGenerationAction {
    Triggered,
    Failed,
    SkippedNoWork,
    SkippedActiveTask,
    NotApplicable,
}

/// Lifecycle phase for background or awaited precise-artifact generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseLifecyclePhase {
    Unavailable,
    NotStarted,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Timeout,
}

/// Result metadata for one precise-artifact generation run.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseGenerationSummary {
    pub status: WorkspacePreciseGenerationStatus,
    pub generated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_sample_paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<WorkspacePreciseFailureClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<WorkspaceRecommendedAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One SCIP generator selected for a planned precise-artifact refresh.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseGenerationPlanItem {
    pub generator_id: String,
    pub language: String,
    pub tool: String,
    pub expected_output_path: String,
}

/// Planned precise-artifact generators for a repository path delta or cold-start refresh.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseGenerationPlanSummary {
    pub action: WorkspacePreciseGenerationAction,
    pub generators: Vec<WorkspacePreciseGenerationPlanItem>,
}

/// Per-generator execution result from a synchronous precise-artifact run.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseGenerationRunItem {
    pub generator_id: String,
    pub language: String,
    pub tool: String,
    pub expected_output_path: String,
    pub summary: WorkspacePreciseGenerationSummary,
}

/// Aggregate outcome from running all selected precise generators for one repository.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseGenerationRunSummary {
    pub action: WorkspacePreciseGenerationAction,
    pub generators: Vec<WorkspacePreciseGenerationRunItem>,
}

/// Repository-local side effects a precise generator may perform outside `.frigg/`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseRepoLocalTouchRisk {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub executes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub may_patch: Vec<String>,
}

impl WorkspacePreciseRepoLocalTouchRisk {
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty() && self.executes.is_empty() && self.may_patch.is_empty()
    }
}

/// Per-generator precise status including last generation and repo-local touch risk.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseGeneratorSummary {
    pub state: WorkspacePreciseGeneratorState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_generation: Option<WorkspacePreciseGenerationSummary>,
    #[serde(
        default,
        skip_serializing_if = "WorkspacePreciseRepoLocalTouchRisk::is_empty"
    )]
    pub repo_local_touch_risk: WorkspacePreciseRepoLocalTouchRisk,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Compact precise readiness summary returned by workspace lifecycle tools.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseSummary {
    pub state: WorkspacePreciseState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<WorkspacePreciseFailureClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<WorkspaceRecommendedAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_action: Option<WorkspacePreciseGenerationAction>,
}

/// Detailed precise lifecycle state including active tasks and last generation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseLifecycleSummary {
    pub phase: WorkspacePreciseLifecyclePhase,
    pub waited_for_completion: bool,
    pub generation_action: WorkspacePreciseGenerationAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_generation: Option<WorkspacePreciseGenerationSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task_phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task: Option<RuntimeTaskSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<WorkspacePreciseFailureClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<WorkspaceRecommendedAction>,
}

/// How `workspace_attach` should treat stale or missing lexical and semantic index state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAttachIndexMode {
    Ensure,
    Defer,
}

/// Lifecycle phase for attach-time or background index refresh work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceIndexLifecyclePhase {
    Ready,
    Refreshing,
    RefreshQueued,
    Timeout,
    Failed,
    Skipped,
    Stale,
    Unavailable,
}

/// Action taken for index refresh during workspace attach or index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceIndexAction {
    Refreshed,
    Queued,
    SkippedNoWork,
    SkippedActiveTask,
    TimedOut,
    Failed,
    Unavailable,
}

/// Index lifecycle state returned while attach or index waits on lexical and semantic readiness.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceIndexLifecycleSummary {
    pub phase: WorkspaceIndexLifecyclePhase,
    pub mode: WorkspaceAttachIndexMode,
    pub waited_for_completion: bool,
    pub timed_out: bool,
    pub action_taken: WorkspaceIndexAction,
    pub lexical_ready: bool,
    pub semantic_ready: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_tasks: Vec<RuntimeTaskSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<WorkspaceRecommendedAction>,
}

impl Default for WorkspaceIndexLifecycleSummary {
    fn default() -> Self {
        Self {
            phase: WorkspaceIndexLifecyclePhase::Ready,
            mode: WorkspaceAttachIndexMode::Ensure,
            waited_for_completion: false,
            timed_out: false,
            action_taken: WorkspaceIndexAction::SkippedNoWork,
            lexical_ready: true,
            semantic_ready: true,
            active_tasks: Vec::new(),
            failure_summary: None,
            recommended_action: None,
        }
    }
}

/// Whether attach created a fresh session adoption or reused an existing workspace entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAttachAction {
    AttachedFresh,
    ReusedWorkspace,
}

/// Parameters for explicit workspace attach that may wait for precise generation readiness.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceAttachParams {
    /// File or directory path to attach.
    pub path: Option<String>,
    /// Visible repository identifier from `list_repositories`.
    pub repository_id: Option<String>,
    /// Whether to make the attached repository the session default. Omit to default to `true`.
    pub set_default: Option<bool>,
    /// Workspace resolution strategy.
    pub resolve_mode: Option<WorkspaceResolveMode>,
    /// Whether to wait for triggered or active precise generation before returning. Omit to default to `true`.
    pub wait_for_precise: Option<bool>,
}

/// Parameters for `workspace`, Frigg's compact workspace status and auto-adoption entrypoint.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct WorkspaceParams {
    /// File or directory path to adopt before returning status.
    pub path: Option<String>,
    /// Visible repository id to adopt before returning status.
    pub repository_id: Option<String>,
    /// When adopting, make the repository the session default. Defaults true.
    pub set_default: Option<bool>,
    /// Path resolution for `path`. Defaults to the enclosing Git root.
    pub resolve_mode: Option<WorkspaceResolveMode>,
}

/// Response from `workspace` with current session status and the known repository list.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceResponse {
    pub repository: Option<RepositorySummary>,
    pub session_default: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<RepositorySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeStatusSummary>,
    /// Session gate: what the agent should do next.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<WorkspaceGateAction>,
    /// Short operator/agent-facing explanation of `recommended_action` when non-obvious.
    ///
    /// Especially for `reindex`: public MCP has no reindex tool — use CLI `frigg index` / operator
    /// maintenance, not an invented MCP tool name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_hint: Option<String>,
    /// Working tree may have paths that differ from the last snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_tree_dirty: Option<bool>,
    /// Paths known to have changed since the last successful snapshot/index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_paths_since_snapshot: Vec<String>,
    /// Echo of runtime watch activity for compact agent consumption.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch_active: Option<bool>,
    /// Tool classes that are fresh enough to trust without live-disk fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fresh_enough_for: Option<Vec<String>>,
    /// Lexical index substrate ready for search (optional health vocab; gate action still primary).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical_ready: Option<bool>,
    /// Semantic index substrate ready or intentionally disabled (not a full health scorecard).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_ready: Option<bool>,
    /// Opt-in local routing stats when `FRIGG_ROUTING_STATS=1`.
    /// Process-local only; never cloud telemetry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_stats: Option<crate::mcp::routing_stats::RoutingStatsSnapshot>,
}

/// Response from `workspace_attach` with storage and precise lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceAttachResponse {
    pub repository: RepositorySummary,
    pub resolved_from: String,
    pub resolution: WorkspaceResolveMode,
    pub session_default: bool,
    pub storage: WorkspaceStorageSummary,
    pub action: WorkspaceAttachAction,
    pub precise: WorkspacePreciseSummary,
    pub precise_lifecycle: WorkspacePreciseLifecycleSummary,
    #[serde(skip)]
    #[schemars(skip)]
    pub index_lifecycle: WorkspaceIndexLifecycleSummary,
}

/// Parameters for `workspace_detach`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceDetachParams {
    /// Repository identifier to detach. Omit to detach the current session-default repository.
    pub repository_id: Option<String>,
}

/// Response from `workspace_detach`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceDetachResponse {
    pub repository_id: String,
    pub session_default: bool,
    pub detached: bool,
}

/// Parameters for `workspace_prepare`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePrepareParams {
    /// File or directory path to prepare.
    pub path: Option<String>,
    /// Visible repository identifier from `list_repositories`.
    pub repository_id: Option<String>,
    /// Whether to make the prepared repository the session default. Omit to default to `true`.
    pub set_default: Option<bool>,
    /// Workspace resolution strategy.
    pub resolve_mode: Option<WorkspaceResolveMode>,
    /// Explicit confirmation required before Frigg writes `.frigg/` state or updates storage.
    pub confirm: Option<bool>,
}

/// Response from `workspace_prepare`.
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

/// Parameters for `workspace_index`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceIndexParams {
    /// File or directory path to index.
    pub path: Option<String>,
    /// Visible repository identifier from `list_repositories`.
    pub repository_id: Option<String>,
    /// Whether to make the indexed repository the session default. Omit to default to `true`.
    pub set_default: Option<bool>,
    /// Workspace resolution strategy.
    pub resolve_mode: Option<WorkspaceResolveMode>,
    /// Explicit confirmation required before Frigg updates storage.
    pub confirm: Option<bool>,
    /// Whether to wait for triggered or active precise generation before returning. Omit to default to `true`.
    pub wait_for_precise: Option<bool>,
}

/// Response from `workspace_index` including snapshot deltas and precise lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceIndexResponse {
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub paths_truncated: bool,
    pub precise_lifecycle: WorkspacePreciseLifecycleSummary,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Empty parameter object for `workspace_current`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct WorkspaceCurrentParams {}

/// Compact response from `workspace_current` summarizing session adoption and runtime tasks.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceCurrentResponse {
    pub repository: Option<RepositorySummary>,
    pub session_default: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<RepositorySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precise: Option<WorkspacePreciseSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precise_ingest: Option<WorkspacePreciseIngestSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeStatusSummary>,
}

/// Parameters for `read_file`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileParams {
    /// Canonical repository-relative path.
    pub path: String,
    /// Optional repository scope.
    pub repository_id: Option<String>,
    pub max_bytes: Option<usize>,
    /// First 1-based line to return.
    pub start_line: Option<usize>,
    /// Last 1-based line to return.
    pub end_line: Option<usize>,
    /// Number of lines to return from `start_line`.
    pub line_count: Option<usize>,
    /// Use `text` for source bytes only, `json` for metadata and content fields, or `citation`
    /// for `LINE|content` text prefixes suitable for user-facing line citations.
    pub presentation_mode: Option<ReadPresentationMode>,
    /// Include context-efficiency metadata; requires `presentation_mode=json`.
    pub include_context_efficiency: Option<bool>,
}

/// Response from `read_file` with selected bytes and optional context-efficiency metadata.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileResponse {
    pub repository_id: String,
    pub path: String,
    /// First 1-based line of the returned window when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<usize>,
    /// Last 1-based line of the returned window when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    pub bytes: usize,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_efficiency: Option<ContextEfficiencyMetadata>,
}

/// Parameters for `read_match` using a prior search or navigation `result_handle`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadMatchParams {
    pub result_handle: String,
    pub match_id: String,
    pub before: Option<usize>,
    pub after: Option<usize>,
    /// Use `text` for source bytes only, `json` for metadata and content fields, or `citation`
    /// for `LINE|content` text prefixes suitable for user-facing line citations.
    pub presentation_mode: Option<ReadPresentationMode>,
    /// Include context-efficiency metadata; requires `presentation_mode=json`.
    pub include_context_efficiency: Option<bool>,
}

/// Response from `read_match` expanding a prior search or navigation hit with local context.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadMatchResponse {
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    pub start_line: usize,
    pub end_line: usize,
    pub bytes: usize,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_efficiency: Option<ContextEfficiencyMetadata>,
}

/// Background runtime work kind tracked by the MCP server process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTaskKind {
    ChangedIndex,
    SemanticRefresh,
    PrecisePrewarm,
    PreciseGenerate,
    WorkspacePrepare,
    WorkspaceIndex,
}

/// Completion state for one tracked runtime task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTaskStatus {
    Running,
    Succeeded,
    Failed,
}

/// One active or recently finished runtime task surfaced in workspace status responses.
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

/// Closed reason set for compact agent-facing watch status.
///
/// Explains `wait_watch` / freshness waits. Gate `recommended_action` remains the decision;
/// do not micro-manage retries from these reasons alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WatchStatusReason {
    /// Watch mode disabled for this transport/profile.
    ModeOff,
    /// Watch runtime enabled but no active lease on the session default repository.
    NoLease,
    /// Incremental refresh task is running (changed-index / semantic refresh).
    Refreshing,
    /// Lease held and no refresh task currently running.
    Active,
    /// Dual-class queue has pending work (debounce/queued), no in-flight refresh task.
    Debouncing,
    /// Reserved for future retry-backoff observability.
    RetryBackoff,
    /// Reserved for future blocked-refresh observability.
    Blocked,
    /// Reserved for future notify-degraded observability.
    NotifyDegraded,
}

/// Compact watch lease / refresh projection for agents (not a raw event log).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WatchStatusSummary {
    pub reason: WatchStatusReason,
    /// Active lease holders for the scoped repository (0 when mode off / no runtime).
    pub lease_count: usize,
    /// Optional repository scope for the lease snapshot (session default when known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    /// Optional human-readable detail (stable short strings only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Dual-class queue depth (pending + in-flight units, 0..=4). No third hot-path class.
    ///
    /// Helps choose `wait_watch` vs path-scoped live-disk without inventing agent-priority queues.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_queue_depth: Option<usize>,
    /// Known dirty paths in the hot-path oracle / scheduler (best-effort count).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_dirty_path_count: Option<usize>,
    /// Age of oldest pending debounce/retry signal in ms (not a hard ETA).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_pending_age_ms: Option<u64>,
}

/// Process-level runtime summary returned by `workspace` / `workspace_current`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeStatusSummary {
    pub profile: RuntimeProfile,
    pub persistent_state_available: bool,
    pub watch_active: bool,
    /// Compact watch lease/refresh status for agents (explains wait_watch; gate action decides).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch_status: Option<WatchStatusSummary>,
    /// Active tool-surface profile (`core` or `extended`).
    pub tool_surface_profile: String,
    /// Tool names registered on **this process** after profile filtering.
    ///
    /// Authoritative for agent routing: prefer this list (or live `tools/list`) over host
    /// schema caches, source `#[tool]` attributes, or inventory freezes. Non-public lifecycle
    /// handlers (e.g. `workspace_index`) are never listed here.
    ///
    /// Always serialized (including empty) so clients can distinguish “field present” from an
    /// older server that never emitted the key.
    #[serde(default)]
    pub tools_exposed: Vec<String>,
    pub status_tool: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_tasks: Vec<RuntimeTaskSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_tasks: Vec<RuntimeTaskSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::types::repository::{
        RepositorySessionSummary, RepositoryWatchSummary, WorkspaceStorageIndexState,
    };
    use serde_json::json;

    #[test]
    fn read_file_params_accept_context_efficiency_opt_in() {
        let params: ReadFileParams = serde_json::from_value(json!({
            "path": "src/lib.rs",
            "include_context_efficiency": true
        }))
        .expect("read_file params should accept include_context_efficiency");

        assert_eq!(params.include_context_efficiency, Some(true));
    }

    #[test]
    fn read_match_params_accept_context_efficiency_opt_in() {
        let params: ReadMatchParams = serde_json::from_value(json!({
            "result_handle": "result-1",
            "match_id": "match-0001",
            "include_context_efficiency": true
        }))
        .expect("read_match params should accept include_context_efficiency");

        assert_eq!(params.include_context_efficiency, Some(true));
    }

    #[test]
    fn read_file_response_omits_context_efficiency_by_default() {
        let value = serde_json::to_value(ReadFileResponse {
            repository_id: "repo-1".to_owned(),
            path: "src/lib.rs".to_owned(),
            start_line: Some(1),
            end_line: Some(1),
            bytes: 12,
            content: "hello world\n".to_owned(),
            context_efficiency: None,
        })
        .expect("read_file response should serialize");

        assert!(value.get("context_efficiency").is_none());
        assert_eq!(value.get("start_line").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(value.get("end_line").and_then(|v| v.as_u64()), Some(1));
    }

    #[test]
    fn read_match_response_omits_context_efficiency_by_default() {
        let value = serde_json::to_value(ReadMatchResponse {
            repository_id: "repo-1".to_owned(),
            path: "src/lib.rs".to_owned(),
            line: 1,
            column: None,
            start_line: 1,
            end_line: 1,
            bytes: 12,
            content: "hello world\n".to_owned(),
            context_efficiency: None,
        })
        .expect("read_match response should serialize");

        assert!(value.get("context_efficiency").is_none());
    }

    #[test]
    fn workspace_index_response_serializes_changed_and_deleted_path_metadata() {
        let response = WorkspaceIndexResponse {
            repository: RepositorySummary {
                repository_id: "repo-1".to_owned(),
                display_name: "fixture".to_owned(),
                root_path: "/tmp/fixture".to_owned(),
                session: RepositorySessionSummary {
                    adopted: true,
                    active_session_count: 1,
                },
                watch: RepositoryWatchSummary {
                    active: false,
                    lease_count: 0,
                },
                storage: None,
                health: None,
            },
            resolved_from: Some("/tmp/fixture/src/lib.rs".to_owned()),
            resolution: Some(WorkspaceResolveMode::GitRoot),
            session_default: true,
            storage: WorkspaceStorageSummary {
                db_path: "/tmp/fixture/.frigg/storage.sqlite3".to_owned(),
                exists: true,
                initialized: true,
                index_state: WorkspaceStorageIndexState::Ready,
                error: None,
            },
            snapshot_id: "snapshot-1".to_owned(),
            files_scanned: 2,
            files_changed: 1,
            files_deleted: 1,
            diagnostics_count: 0,
            changed_paths: vec!["src/lib.rs".to_owned()],
            deleted_paths: vec!["old.rs".to_owned()],
            paths_truncated: true,
            precise_lifecycle: WorkspacePreciseLifecycleSummary {
                phase: WorkspacePreciseLifecyclePhase::Skipped,
                waited_for_completion: false,
                generation_action: WorkspacePreciseGenerationAction::SkippedNoWork,
                last_generation: None,
                active_task_phase: None,
                active_task: None,
                failure_class: None,
                failure_summary: None,
                recommended_action: None,
            },
        };

        let value =
            serde_json::to_value(&response).expect("workspace_index response should serialize");
        assert_eq!(value["changed_paths"], json!(["src/lib.rs"]));
        assert_eq!(value["deleted_paths"], json!(["old.rs"]));
        assert_eq!(value["paths_truncated"], json!(true));

        let mut omitted = response;
        omitted.changed_paths.clear();
        omitted.deleted_paths.clear();
        omitted.paths_truncated = false;
        let value = serde_json::to_value(omitted)
            .expect("workspace_index response should serialize empty path metadata");
        assert!(value.get("changed_paths").is_none());
        assert!(value.get("deleted_paths").is_none());
        assert!(value.get("paths_truncated").is_none());
    }

    #[test]
    fn workspace_gate_action_serde_snake_case_is_stable() {
        let value = serde_json::to_value(WorkspaceGateAction::UseLiveDiskForTouchedFiles)
            .expect("serialize gate action");
        assert_eq!(value, json!("use_live_disk_for_touched_files"));
        let parsed: WorkspaceGateAction =
            serde_json::from_value(json!("adopt_repo")).expect("parse gate action");
        assert_eq!(parsed, WorkspaceGateAction::AdoptRepo);
        assert_eq!(
            serde_json::to_value(WorkspaceGateAction::Reindex).expect("serialize reindex"),
            json!("reindex")
        );
    }

    #[test]
    fn reindex_gate_hint_points_at_cli_not_mcp_tool() {
        let hint = workspace_gate_hint(WorkspaceGateAction::Reindex)
            .expect("reindex must carry a gate_hint");
        assert!(
            hint.contains("frigg index") && hint.contains("no public MCP"),
            "reindex gate_hint should point at CLI, not an MCP tool: {hint}"
        );
        assert!(workspace_gate_hint(WorkspaceGateAction::Ready).is_none());
        assert!(workspace_gate_hint(WorkspaceGateAction::AdoptRepo).is_none());
        assert!(workspace_gate_hint(WorkspaceGateAction::WaitWatch).is_none());
        assert!(workspace_gate_hint(WorkspaceGateAction::UseLiveDiskForTouchedFiles).is_none());

        let response = WorkspaceResponse {
            repository: None,
            session_default: false,
            repositories: Vec::new(),
            runtime: None,
            recommended_action: Some(WorkspaceGateAction::Reindex),
            gate_hint: workspace_gate_hint(WorkspaceGateAction::Reindex),
            working_tree_dirty: None,
            changed_paths_since_snapshot: Vec::new(),
            watch_active: None,
            fresh_enough_for: None,
            lexical_ready: None,
            semantic_ready: None,
            routing_stats: None,
        };
        let value = serde_json::to_value(&response).expect("workspace response should serialize");
        assert_eq!(value["recommended_action"], "reindex");
        assert_eq!(value["gate_hint"], hint);
    }

    #[test]
    fn workspace_response_serializes_gate_fields() {
        let response = WorkspaceResponse {
            repository: None,
            session_default: false,
            repositories: Vec::new(),
            runtime: None,
            recommended_action: Some(WorkspaceGateAction::AdoptRepo),
            gate_hint: None,
            working_tree_dirty: Some(false),
            changed_paths_since_snapshot: Vec::new(),
            watch_active: Some(false),
            fresh_enough_for: None,
            lexical_ready: Some(true),
            semantic_ready: Some(false),
            routing_stats: None,
        };
        let value = serde_json::to_value(&response).expect("serialize workspace response");
        assert_eq!(value["recommended_action"], "adopt_repo");
        assert_eq!(value["working_tree_dirty"], false);
        assert_eq!(value["watch_active"], false);
        assert_eq!(value["lexical_ready"], true);
        assert_eq!(value["semantic_ready"], false);
        assert!(value.get("changed_paths_since_snapshot").is_none());
        assert!(value.get("fresh_enough_for").is_none());
        assert!(value.get("routing_stats").is_none());
        assert!(value.get("gate_hint").is_none());
    }

    #[test]
    fn watch_status_reason_serde_snake_case_is_stable() {
        assert_eq!(
            serde_json::to_value(WatchStatusReason::ModeOff).expect("serialize"),
            json!("mode_off")
        );
        assert_eq!(
            serde_json::to_value(WatchStatusReason::NoLease).expect("serialize"),
            json!("no_lease")
        );
        assert_eq!(
            serde_json::to_value(WatchStatusReason::Refreshing).expect("serialize"),
            json!("refreshing")
        );
        assert_eq!(
            serde_json::to_value(WatchStatusReason::Active).expect("serialize"),
            json!("active")
        );
        assert_eq!(
            serde_json::to_value(WatchStatusReason::Debouncing).expect("serialize"),
            json!("debouncing")
        );
    }
}
