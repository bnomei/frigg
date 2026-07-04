//! Index planning: manifest diffing, snapshot selection, and semantic refresh assembly.
//!
//! Plans record which repository paths changed, which manifest snapshot to advance from,
//! and whether semantic embedding work stays incremental or must run as a full refresh.

use std::path::{Path, PathBuf};

use crate::domain::{FriggError, FriggResult};
use crate::settings::SemanticRuntimeConfig;
use crate::storage::DEFAULT_RETAINED_MANIFEST_SNAPSHOTS;
#[cfg(test)]
use crate::storage::Storage;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::super::ManifestBuilder;
#[cfg(test)]
use super::super::manifest::diff;
use super::super::manifest::{
    deterministic_snapshot_id, normalize_deleted_repository_relative_path,
    normalize_repository_relative_path,
};
use super::super::semantic::SemanticIndexStorage;
use super::super::{FileDigest, ManifestBuildDiagnostic, ManifestDiagnosticKind, ManifestDiff};
use super::semantic::build_semantic_refresh_plan;
#[cfg(test)]
use super::store::ManifestStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Selects whether a refresh should rebuild the whole repository view or advance it from changed
/// paths only.
pub enum IndexMode {
    Full,
    ChangedOnly,
}

impl IndexMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::ChangedOnly => "changed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Outcome of a completed refresh, used by watch runtime and user-facing tooling to describe how
/// repository state advanced.
pub struct IndexSummary {
    pub repository_id: String,
    pub snapshot_id: String,
    pub previous_snapshot_id: Option<String>,
    pub snapshot_plan: String,
    pub files_scanned: usize,
    pub files_changed: usize,
    pub files_deleted: usize,
    pub changed_paths: Vec<String>,
    pub deleted_paths: Vec<String>,
    pub semantic_refresh_mode: SemanticRefreshMode,
    pub semantic_provider: Option<String>,
    pub semantic_model: Option<String>,
    pub semantic_records: usize,
    pub diagnostics: IndexDiagnostics,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Planning-time decision about how the manifest snapshot should advance and whether a later
/// semantic failure must roll it back.
pub enum ManifestSnapshotPlan {
    ReuseExisting {
        snapshot_id: String,
    },
    PersistNew {
        snapshot_id: String,
        rollback_on_semantic_failure: bool,
    },
}

impl ManifestSnapshotPlan {
    pub fn snapshot_id(&self) -> &str {
        match self {
            Self::ReuseExisting { snapshot_id } | Self::PersistNew { snapshot_id, .. } => {
                snapshot_id
            }
        }
    }

    pub fn rollback_on_semantic_failure(&self) -> bool {
        match self {
            Self::ReuseExisting { .. } => false,
            Self::PersistNew {
                rollback_on_semantic_failure,
                ..
            } => *rollback_on_semantic_failure,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReuseExisting { .. } => "reuse_existing",
            Self::PersistNew { .. } => "persist_new",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Policy for how semantic artifacts move relative to the manifest refresh.
pub enum SemanticRefreshMode {
    Disabled,
    FullRebuild,
    FullRebuildFromChangedOnly,
    IncrementalAdvance,
    ReuseExisting,
}

impl SemanticRefreshMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::FullRebuild => "full_rebuild",
            Self::FullRebuildFromChangedOnly => "full_rebuild_from_changed_only",
            Self::IncrementalAdvance => "incremental_advance",
            Self::ReuseExisting => "reuse_existing",
        }
    }

    pub fn is_noop(self) -> bool {
        matches!(self, Self::Disabled | Self::ReuseExisting)
    }

    pub fn is_rebuild_like(self) -> bool {
        matches!(self, Self::FullRebuild | Self::FullRebuildFromChangedOnly)
    }

    pub fn shows_path_delta(self) -> bool {
        matches!(self, Self::IncrementalAdvance)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Coarse domain phases emitted while a repository index is actively doing work.
pub enum IndexProgressPhase {
    InitializeStorage,
    LoadManifest,
    BuildManifest,
    BuildPlan,
    PersistManifestSnapshot,
    RefreshRetrievalProjections,
    SemanticRefresh,
    PruneManifestSnapshots,
    CheckpointWal,
}

impl IndexProgressPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InitializeStorage => "initialize_storage",
            Self::LoadManifest => "load_manifest",
            Self::BuildManifest => "build_manifest",
            Self::BuildPlan => "build_plan",
            Self::PersistManifestSnapshot => "persist_manifest_snapshot",
            Self::RefreshRetrievalProjections => "refresh_retrieval_projections",
            Self::SemanticRefresh => "semantic_refresh",
            Self::PruneManifestSnapshots => "prune_manifest_snapshots",
            Self::CheckpointWal => "checkpoint_wal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Status for an [`IndexProgressEvent`] phase transition.
pub enum IndexProgressStatus {
    Starting,
    Ok,
    Skipped,
    Warning,
}

impl IndexProgressStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ok => "ok",
            Self::Skipped => "skipped",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// UI-agnostic progress snapshot for index work; renderers decide how to display it.
pub struct IndexProgressEvent {
    pub repository_id: String,
    pub mode: IndexMode,
    pub phase: IndexProgressPhase,
    pub status: IndexProgressStatus,
    pub semantic_refresh_mode: Option<SemanticRefreshMode>,
    pub snapshot_id: Option<String>,
    pub previous_snapshot_id: Option<String>,
    pub files_scanned: Option<usize>,
    pub files_changed: Option<usize>,
    pub files_deleted: Option<usize>,
    pub diagnostics: Option<usize>,
    pub records: Option<usize>,
    pub changed_paths: Option<usize>,
    pub deleted_paths: Option<usize>,
    pub pruned_snapshots: Option<usize>,
    pub duration_ms: Option<u128>,
}

impl IndexProgressEvent {
    pub fn new(
        repository_id: impl Into<String>,
        mode: IndexMode,
        phase: IndexProgressPhase,
        status: IndexProgressStatus,
    ) -> Self {
        Self {
            repository_id: repository_id.into(),
            mode,
            phase,
            status,
            semantic_refresh_mode: None,
            snapshot_id: None,
            previous_snapshot_id: None,
            files_scanned: None,
            files_changed: None,
            files_deleted: None,
            diagnostics: None,
            records: None,
            changed_paths: None,
            deleted_paths: None,
            pruned_snapshots: None,
            duration_ms: None,
        }
    }

    pub fn with_snapshot(mut self, snapshot_id: impl Into<String>) -> Self {
        self.snapshot_id = Some(snapshot_id.into());
        self
    }

    pub fn with_previous_snapshot(mut self, previous_snapshot_id: Option<&str>) -> Self {
        self.previous_snapshot_id = previous_snapshot_id.map(ToOwned::to_owned);
        self
    }

    pub fn with_file_counts(
        mut self,
        files_scanned: usize,
        files_changed: usize,
        files_deleted: usize,
    ) -> Self {
        self.files_scanned = Some(files_scanned);
        self.files_changed = Some(files_changed);
        self.files_deleted = Some(files_deleted);
        self
    }

    pub fn with_diagnostics(mut self, diagnostics: usize) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    pub fn with_records(mut self, records: usize) -> Self {
        self.records = Some(records);
        self
    }

    pub fn with_semantic_refresh_mode(mut self, mode: SemanticRefreshMode) -> Self {
        self.semantic_refresh_mode = Some(mode);
        self
    }

    pub fn with_path_counts(mut self, changed_paths: usize, deleted_paths: usize) -> Self {
        self.changed_paths = Some(changed_paths);
        self.deleted_paths = Some(deleted_paths);
        self
    }

    pub fn with_pruned_snapshots(mut self, pruned_snapshots: usize) -> Self {
        self.pruned_snapshots = Some(pruned_snapshots);
        self
    }

    pub fn with_duration_ms(mut self, duration_ms: u128) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Concrete semantic work derived from the index plan, including the manifest view and path
/// delta that embedding refresh should consume.
pub struct SemanticRefreshPlan {
    pub mode: SemanticRefreshMode,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub records_manifest: Vec<FileDigest>,
    pub changed_paths: Vec<String>,
    pub deleted_paths: Vec<String>,
    pub advance_from_snapshot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Precomputed refresh plan that ties manifest, projection, and semantic work into one explainable
/// unit before writes begin.
pub struct IndexPlan {
    pub repository_id: String,
    pub mode: IndexMode,
    pub previous_snapshot_id: Option<String>,
    pub current_manifest: Vec<FileDigest>,
    pub manifest_diff: ManifestDiff,
    pub changed_paths: Vec<String>,
    pub deleted_paths: Vec<String>,
    pub snapshot_plan: ManifestSnapshotPlan,
    pub semantic_refresh: SemanticRefreshPlan,
    pub diagnostics: IndexDiagnostics,
    pub files_scanned: usize,
    pub files_changed: usize,
    pub files_deleted: usize,
    pub retained_manifest_snapshots: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
/// Non-fatal issues collected during refresh planning so callers can surface degraded freshness
/// without inventing a separate error channel.
pub struct IndexDiagnostics {
    pub entries: Vec<ManifestBuildDiagnostic>,
}

impl IndexDiagnostics {
    /// Returns the total number of manifest-build diagnostics attached to a plan.
    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    /// Counts manifest-build diagnostics of one kind, such as walk or read failures.
    pub fn count_by_kind(&self, kind: ManifestDiagnosticKind) -> usize {
        self.entries
            .iter()
            .filter(|diagnostic| diagnostic.kind == kind)
            .count()
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_index_plan(
    repository_id: &str,
    workspace_root: &Path,
    mode: IndexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    previous_snapshot_id: Option<String>,
    had_previous_manifest: bool,
    current_manifest: Vec<FileDigest>,
    diagnostics: IndexDiagnostics,
    manifest_diff: ManifestDiff,
    dirty_path_hints: &[PathBuf],
    storage: Option<&dyn SemanticIndexStorage>,
) -> FriggResult<IndexPlan> {
    let files_scanned = current_manifest.len();
    let files_changed = match mode {
        IndexMode::Full => files_scanned,
        IndexMode::ChangedOnly => manifest_diff.added.len() + manifest_diff.modified.len(),
    };
    let files_deleted = manifest_diff.deleted.len();
    let snapshot_plan = build_manifest_snapshot_plan(
        repository_id,
        mode,
        files_changed,
        files_deleted,
        previous_snapshot_id.as_deref(),
        &current_manifest,
    )?;
    let (changed_paths, deleted_paths) =
        normalize_manifest_diff_paths(workspace_root, &manifest_diff)?;
    let semantic_refresh = build_semantic_refresh_plan(
        repository_id,
        mode,
        semantic_runtime,
        workspace_root,
        previous_snapshot_id.as_deref(),
        had_previous_manifest,
        snapshot_plan.snapshot_id(),
        &current_manifest,
        &manifest_diff,
        &changed_paths,
        &deleted_paths,
        dirty_path_hints,
        storage,
    )?;

    Ok(IndexPlan {
        repository_id: repository_id.to_owned(),
        mode,
        previous_snapshot_id,
        current_manifest,
        manifest_diff,
        changed_paths,
        deleted_paths,
        snapshot_plan,
        semantic_refresh,
        diagnostics,
        files_scanned,
        files_changed,
        files_deleted,
        retained_manifest_snapshots: DEFAULT_RETAINED_MANIFEST_SNAPSHOTS,
    })
}

fn normalize_manifest_diff_paths(
    workspace_root: &Path,
    manifest_diff: &ManifestDiff,
) -> FriggResult<(Vec<String>, Vec<String>)> {
    let changed_paths = manifest_diff
        .added
        .iter()
        .chain(manifest_diff.modified.iter())
        .map(|digest| normalize_repository_relative_path(workspace_root, &digest.path))
        .collect::<FriggResult<Vec<_>>>()?;

    let deleted_paths = manifest_diff
        .deleted
        .iter()
        .filter_map(|digest| {
            normalize_deleted_repository_relative_path(workspace_root, &digest.path).transpose()
        })
        .collect::<FriggResult<Vec<_>>>()?;

    Ok((dedup_paths(changed_paths), dedup_paths(deleted_paths)))
}

fn dedup_paths(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn build_manifest_snapshot_plan(
    repository_id: &str,
    mode: IndexMode,
    files_changed: usize,
    files_deleted: usize,
    previous_snapshot_id: Option<&str>,
    current_manifest: &[FileDigest],
) -> FriggResult<ManifestSnapshotPlan> {
    if mode == IndexMode::ChangedOnly
        && files_changed == 0
        && files_deleted == 0
        && previous_snapshot_id.is_some()
    {
        return Ok(ManifestSnapshotPlan::ReuseExisting {
            snapshot_id: previous_snapshot_id.map(ToOwned::to_owned).ok_or_else(|| {
                FriggError::Internal(
                    "failed to resolve previous snapshot identifier for unchanged manifest"
                        .to_owned(),
                )
            })?,
        });
    }

    let snapshot_id = deterministic_snapshot_id(repository_id, current_manifest);

    Ok(ManifestSnapshotPlan::PersistNew {
        rollback_on_semantic_failure: previous_snapshot_id
            .map(|previous| previous != snapshot_id)
            .unwrap_or(true),
        snapshot_id,
    })
}

#[cfg(test)]
pub(crate) fn build_index_plan_for_tests(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: IndexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    dirty_path_hints: &[PathBuf],
) -> FriggResult<IndexPlan> {
    let db_preexisted = db_path.exists();
    let manifest_store = ManifestStore::new(db_path);
    manifest_store.initialize_for_index(semantic_runtime.enabled)?;
    let previous_manifest = if mode == IndexMode::Full && !db_preexisted {
        None
    } else {
        manifest_store.load_latest_manifest_for_repository(repository_id)?
    };
    let previous_snapshot_id = previous_manifest
        .as_ref()
        .map(|manifest| manifest.snapshot_id.clone());
    let previous_entries = previous_manifest
        .as_ref()
        .map(|manifest| manifest.entries.as_slice())
        .unwrap_or(&[]);

    let manifest_builder = ManifestBuilder::default();
    let manifest_output = match mode {
        IndexMode::Full => manifest_builder.build_with_diagnostics(workspace_root)?,
        IndexMode::ChangedOnly if previous_manifest.is_some() => manifest_builder
            .build_changed_only_with_hints_and_diagnostics(
                workspace_root,
                previous_entries,
                dirty_path_hints,
            )?,
        IndexMode::ChangedOnly => manifest_builder.build_with_diagnostics(workspace_root)?,
    };
    let current_manifest = manifest_output.entries;
    let diagnostics = IndexDiagnostics {
        entries: manifest_output.diagnostics,
    };
    let manifest_diff = if mode == IndexMode::Full && previous_entries.is_empty() {
        ManifestDiff::default()
    } else {
        diff(previous_entries, &current_manifest)
    };
    let storage = semantic_runtime.enabled.then(|| Storage::new(db_path));
    let semantic_storage = storage
        .as_ref()
        .map(|storage| storage as &dyn SemanticIndexStorage);

    build_index_plan(
        repository_id,
        workspace_root,
        mode,
        semantic_runtime,
        previous_snapshot_id,
        previous_manifest.is_some(),
        current_manifest,
        diagnostics,
        manifest_diff,
        dirty_path_hints,
        semantic_storage,
    )
}
