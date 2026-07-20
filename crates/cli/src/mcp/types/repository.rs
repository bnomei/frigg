//! Repository discovery MCP wire types returned by `list_repositories` and workspace summaries.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::search::SearchSymbolPathClass;
use super::workspace::{RuntimeStatusSummary, WorkspacePreciseGeneratorSummary};

/// Per-session workspace adoption state for one known repository.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RepositorySessionSummary {
    pub adopted: bool,
    pub active_session_count: usize,
}

/// Filesystem watch lease state for a repository root.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RepositoryWatchSummary {
    pub active: bool,
    pub lease_count: usize,
}

/// Repository-level status view returned by discovery and workspace lifecycle tools before clients
/// ask deeper search or navigation questions.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RepositorySummary {
    pub repository_id: String,
    pub display_name: String,
    pub root_path: String,
    pub session: RepositorySessionSummary,
    pub watch: RepositoryWatchSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<WorkspaceStorageSummary>,
    #[serde(skip)]
    #[schemars(skip)]
    pub health: Option<WorkspaceIndexHealthSummary>,
}

/// Response from `list_repositories`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListRepositoriesResponse {
    pub repositories: Vec<RepositorySummary>,
}

/// Side-effect-free process status returned by the HTTP `/status` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ServiceStatusResponse {
    pub schema_version: u32,
    pub frigg_version: String,
    pub repositories: Vec<RepositorySummary>,
    pub runtime: RuntimeStatusSummary,
}

/// Empty parameter object for `list_repositories`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ListRepositoriesParams {}

/// Parameters for `list_files`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ListFilesParams {
    /// Optional. Omit to use the current session default repository, auto-adopt the only visible
    /// startup repository, or list across adopted repositories. Set only for multi-repo searches.
    pub repository_id: Option<String>,
    /// Repository-relative path filter replacing `rg --files path/`, `find path/`, or `fd`.
    /// Example: `^crates/cli/src/`.
    pub path_regex: Option<String>,
    /// Repository-relative glob replacing `rg --files -g`.
    pub glob: Option<String>,
    /// Optional source-language filter such as `rust`, `php`, `typescript`, or `python`.
    pub language: Option<String>,
    /// Optional path class filter: `runtime`, `support`, or `project`.
    pub path_class: Option<SearchSymbolPathClass>,
    /// Equivalent to `rg --hidden` when true. Defaults to false for rg-shaped behavior.
    pub include_hidden: Option<bool>,
    /// Optional max returned files. Omit for the default bounded listing.
    pub limit: Option<usize>,
    /// Continuation cursor returned as `resume_from` when a file listing is truncated.
    pub resume_from: Option<String>,
    /// Opaque v2 continuation returned by an earlier page. Cannot be combined with `resume_from`.
    pub continuation: Option<String>,
}

/// One repository-relative file row returned by `list_files`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListFilesEntry {
    pub repository_id: String,
    pub path: String,
    pub size_bytes: u64,
}

/// Response from `list_files`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListFilesResponse {
    pub total_files: usize,
    pub files: Vec<ListFilesEntry>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_from: Option<String>,
    /// Canonical cardinality and paging truth for the file rows.
    pub completeness: super::ResultCompleteness,
}

/// How workspace attach resolves an input path to a repository root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceResolveMode {
    #[serde(alias = "git", alias = "repo_root", alias = "repo")]
    GitRoot,
    #[serde(alias = "dir", alias = "directory")]
    Direct,
}

/// Persisted `.frigg/` storage readiness for workspace indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStorageIndexState {
    MissingDb,
    Uninitialized,
    Ready,
    Error,
}

/// `.frigg/` storage location and initialization state for one workspace.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceStorageSummary {
    pub db_path: String,
    pub exists: bool,
    pub initialized: bool,
    pub index_state: WorkspaceStorageIndexState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Readiness of one indexed retrieval substrate such as lexical, semantic, or SCIP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceIndexComponentState {
    Missing,
    Ready,
    Stale,
    Disabled,
    Error,
}

/// Status snapshot for one workspace index component and its backing snapshot metadata.
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

/// How completely ingested precise artifacts cover a repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseCoverageMode {
    Full,
    Partial,
    None,
}

/// Runtime precise-artifact ingest outcome for a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePreciseIngestState {
    Missing,
    Ready,
    Partial,
    Failed,
    Error,
}

/// One failed SCIP artifact sample from precise ingest.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseArtifactFailureSummary {
    pub artifact_label: String,
    pub stage: String,
    pub detail: String,
}

/// Precise-artifact discovery and ingest totals for a workspace.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePreciseIngestSummary {
    pub state: WorkspacePreciseIngestState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_mode: Option<WorkspacePreciseCoverageMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub artifacts_discovered: usize,
    pub artifacts_discovered_bytes: u64,
    pub artifacts_ingested: usize,
    pub artifacts_ingested_bytes: u64,
    pub artifacts_failed: usize,
    pub artifacts_failed_bytes: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_artifacts: Vec<WorkspacePreciseArtifactFailureSummary>,
}

/// High-level view of which retrieval substrates are ready for a workspace and therefore how rich
/// downstream search or navigation responses can be.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceIndexHealthSummary {
    pub lexical: WorkspaceIndexComponentSummary,
    pub semantic: WorkspaceIndexComponentSummary,
    pub scip: WorkspaceIndexComponentSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precise_ingest: Option<WorkspacePreciseIngestSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub precise_generators: Vec<WorkspacePreciseGeneratorSummary>,
}
