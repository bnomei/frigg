use crate::settings::RuntimeProfile;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::repository::{RepositorySummary, WorkspaceResolveMode, WorkspaceStorageSummary};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseGeneratorState {
    Available,
    MissingTool,
    Unsupported,
    NotConfigured,
    Error,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRecommendedAction {
    InstallTool,
    RerunReindex,
    CheckEnvironment,
    UpstreamToolFailure,
    UseHeuristicMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseState {
    Ok,
    Partial,
    Failed,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseGenerationAction {
    Triggered,
    SkippedNoWork,
    SkippedActiveTask,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseGenerationSummary {
    pub status: WorkspacePreciseGenerationStatus,
    pub generated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<WorkspacePreciseFailureClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<WorkspaceRecommendedAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAttachAction {
    AttachedFresh,
    ReusedWorkspace,
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
    pub action: WorkspaceAttachAction,
    pub precise: WorkspacePreciseSummary,
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
    pub precise: Option<WorkspacePreciseSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeStatusSummary>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTaskKind {
    ChangedReindex,
    SemanticRefresh,
    PrecisePrewarm,
    PreciseGenerate,
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
