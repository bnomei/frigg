use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::workspace::WorkspacePreciseGeneratorSummary;

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
/// Repository-level status view returned by discovery and workspace lifecycle tools before clients
/// ask deeper search or navigation questions.
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
    #[serde(alias = "git", alias = "repo_root", alias = "repo")]
    GitRoot,
    #[serde(alias = "dir", alias = "directory")]
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
/// High-level view of which retrieval substrates are ready for a workspace and therefore how rich
/// downstream search or navigation responses can be.
pub struct WorkspaceIndexHealthSummary {
    pub lexical: WorkspaceIndexComponentSummary,
    pub semantic: WorkspaceIndexComponentSummary,
    pub scip: WorkspaceIndexComponentSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub precise_generators: Vec<WorkspacePreciseGeneratorSummary>,
}
