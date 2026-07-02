//! Workspace lifecycle MCP wire types: attach, prepare, index, runtime tasks, and precise status.

use crate::settings::RuntimeProfile;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::repository::{
    RepositorySummary, WorkspacePreciseIngestSummary, WorkspaceResolveMode, WorkspaceStorageSummary,
};
use super::{ContextEfficiencyMetadata, ReadPresentationMode};

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
    RerunIndex,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAttachIndexMode {
    Ensure,
    Defer,
    Skip,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceIndexAction {
    Refreshed,
    Queued,
    SkippedNoWork,
    SkippedActiveTask,
    SkippedByRequest,
    Failed,
    Unavailable,
}

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
    /// Whether to wait for triggered or active precise generation before returning. Omit to default to `true`.
    pub wait_for_precise: Option<bool>,
    /// How attach should handle stale or missing lexical/semantic index state. Omit to default to `ensure`.
    pub index_mode: Option<WorkspaceAttachIndexMode>,
    /// Whether to wait for attach-time index work before returning. Omit to default to `true` for `ensure`, otherwise `false`.
    pub wait_for_index: Option<bool>,
    /// Attach-time index wait timeout in milliseconds. Omit to default to 30000.
    pub index_timeout_ms: Option<u64>,
}

/// Response from `workspace_attach` with storage, index, and precise lifecycle state.
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
    pub index_lifecycle: WorkspaceIndexLifecycleSummary,
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
pub struct WorkspaceIndexParams {
    /// File or directory path to index. Relative paths resolve against the Frigg server process cwd.
    pub path: Option<String>,
    /// Known repository identifier from `list_repositories`.
    pub repository_id: Option<String>,
    /// Whether to make the indexed repository the session default. Omit to default to `true`.
    pub set_default: Option<bool>,
    /// Workspace resolution strategy when using `path`. Omit to prefer the enclosing Git root before falling back to the direct directory.
    pub resolve_mode: Option<WorkspaceResolveMode>,
    /// Explicit confirmation required before Frigg updates storage.
    pub confirm: Option<bool>,
    /// Whether to wait for triggered or active precise generation before returning. Omit to default to `true`.
    pub wait_for_precise: Option<bool>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct WorkspaceCurrentParams {}

/// Response from `workspace_current` summarizing session adoption and runtime health.
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
    pub index_lifecycle: Option<WorkspaceIndexLifecycleSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeStatusSummary>,
}

/// Parameters for `read_file`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileParams {
    pub path: String,
    pub repository_id: Option<String>,
    pub max_bytes: Option<usize>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    /// Omit or set `text` for pure MCP text output; set `json` when callers need structured
    /// metadata or a machine-readable `content` field.
    pub presentation_mode: Option<ReadPresentationMode>,
    /// Include bounded context-efficiency metadata in the response. Defaults to false.
    pub include_context_efficiency: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileResponse {
    pub repository_id: String,
    pub path: String,
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
    /// Omit or set `text` for pure MCP text output; set `json` when callers need structured
    /// metadata or a machine-readable `content` field.
    pub presentation_mode: Option<ReadPresentationMode>,
    /// Include bounded context-efficiency metadata in the response. Defaults to false.
    pub include_context_efficiency: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadMatchResponse {
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    pub line_start: usize,
    pub line_end: usize,
    pub bytes: usize,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_efficiency: Option<ContextEfficiencyMetadata>,
}

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
            bytes: 12,
            content: "hello world\n".to_owned(),
            context_efficiency: None,
        })
        .expect("read_file response should serialize");

        assert!(value.get("context_efficiency").is_none());
    }

    #[test]
    fn read_match_response_omits_context_efficiency_by_default() {
        let value = serde_json::to_value(ReadMatchResponse {
            repository_id: "repo-1".to_owned(),
            path: "src/lib.rs".to_owned(),
            line: 1,
            column: None,
            line_start: 1,
            line_end: 1,
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
}
