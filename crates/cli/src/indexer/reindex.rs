use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::domain::{FriggError, FriggResult};
use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials};
use crate::storage::{DEFAULT_RETAINED_MANIFEST_SNAPSHOTS, Storage};
use serde::{Deserialize, Serialize};

use super::manifest::{
    deterministic_snapshot_id, diff, file_digest_to_manifest_entry, manifest_entry_to_file_digest,
    normalize_repository_relative_path,
};
use super::semantic::{
    RuntimeSemanticEmbeddingExecutor, SemanticRuntimeEmbeddingExecutor,
    build_semantic_embedding_records, resolve_semantic_runtime_config_from_env,
};
use super::{
    FileDigest, ManifestBuildDiagnostic, ManifestBuilder, ManifestDiagnosticKind, ManifestDiff,
    RepositoryManifest,
};

#[derive(Debug, Clone)]
pub struct ManifestStore {
    storage: Storage,
}

impl ManifestStore {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            storage: Storage::new(db_path),
        }
    }

    pub fn initialize(&self) -> FriggResult<()> {
        self.storage.initialize()
    }

    pub fn upsert_repository(
        &self,
        repository_id: &str,
        root_path: &Path,
        display_name: &str,
    ) -> FriggResult<()> {
        self.storage
            .upsert_repository(repository_id, root_path, display_name)
    }

    pub(crate) fn initialize_for_reindex(&self, semantic_enabled: bool) -> FriggResult<()> {
        if semantic_enabled {
            self.storage.initialize()
        } else {
            self.storage.initialize_without_vector_store()
        }
    }

    pub fn persist_snapshot_manifest(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        entries: &[FileDigest],
    ) -> FriggResult<()> {
        let manifest_entries = entries
            .iter()
            .map(file_digest_to_manifest_entry)
            .collect::<Vec<_>>();
        self.storage
            .upsert_manifest(repository_id, snapshot_id, &manifest_entries)
    }

    pub fn load_snapshot_manifest(&self, snapshot_id: &str) -> FriggResult<Vec<FileDigest>> {
        self.storage
            .load_manifest_for_snapshot(snapshot_id)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(manifest_entry_to_file_digest)
                    .collect()
            })
    }

    pub fn load_latest_manifest_for_repository(
        &self,
        repository_id: &str,
    ) -> FriggResult<Option<RepositoryManifest>> {
        self.storage
            .load_latest_manifest_for_repository(repository_id)
            .map(|snapshot| {
                snapshot.map(|snapshot| RepositoryManifest {
                    repository_id: snapshot.repository_id,
                    snapshot_id: snapshot.snapshot_id,
                    entries: snapshot
                        .entries
                        .into_iter()
                        .map(manifest_entry_to_file_digest)
                        .collect(),
                })
            })
    }

    pub fn delete_snapshot(&self, snapshot_id: &str) -> FriggResult<()> {
        self.storage.delete_snapshot(snapshot_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReindexMode {
    Full,
    ChangedOnly,
}

impl ReindexMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::ChangedOnly => "changed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReindexSummary {
    pub repository_id: String,
    pub snapshot_id: String,
    pub files_scanned: usize,
    pub files_changed: usize,
    pub files_deleted: usize,
    pub diagnostics: ReindexDiagnostics,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticRefreshMode {
    Disabled,
    FullRebuild,
    FullRebuildFromChangedOnly,
    IncrementalAdvance,
    ReuseExisting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRefreshPlan {
    pub mode: SemanticRefreshMode,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub records_manifest: Vec<FileDigest>,
    pub changed_paths: Vec<String>,
    pub deleted_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReindexPlan {
    pub repository_id: String,
    pub mode: ReindexMode,
    pub previous_snapshot_id: Option<String>,
    pub current_manifest: Vec<FileDigest>,
    pub manifest_diff: ManifestDiff,
    pub snapshot_plan: ManifestSnapshotPlan,
    pub semantic_refresh: SemanticRefreshPlan,
    pub diagnostics: ReindexDiagnostics,
    pub files_scanned: usize,
    pub files_changed: usize,
    pub files_deleted: usize,
    pub retained_manifest_snapshots: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReindexExecutionPhase {
    PersistManifestSnapshot,
    SemanticRefresh,
    RollbackManifestSnapshot,
    PruneManifestSnapshots,
}

impl ReindexExecutionPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::PersistManifestSnapshot => "persist_manifest_snapshot",
            Self::SemanticRefresh => "semantic_refresh",
            Self::RollbackManifestSnapshot => "rollback_manifest_snapshot",
            Self::PruneManifestSnapshots => "prune_manifest_snapshots",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReindexDiagnostics {
    pub entries: Vec<ManifestBuildDiagnostic>,
}

impl ReindexDiagnostics {
    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    pub fn count_by_kind(&self, kind: ManifestDiagnosticKind) -> usize {
        self.entries
            .iter()
            .filter(|diagnostic| diagnostic.kind == kind)
            .count()
    }
}

pub fn reindex_repository(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
) -> FriggResult<ReindexSummary> {
    let semantic_runtime = resolve_semantic_runtime_config_from_env()?;
    let credentials = SemanticRuntimeCredentials::from_process_env();
    reindex_repository_with_runtime_config(
        repository_id,
        workspace_root,
        db_path,
        mode,
        &semantic_runtime,
        &credentials,
    )
}

pub fn reindex_repository_with_runtime_config(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
) -> FriggResult<ReindexSummary> {
    reindex_repository_with_runtime_config_and_dirty_paths(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        &[],
    )
}

pub fn reindex_repository_with_runtime_config_and_dirty_paths(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    dirty_path_hints: &[PathBuf],
) -> FriggResult<ReindexSummary> {
    let executor = RuntimeSemanticEmbeddingExecutor::new(credentials.clone());
    reindex_repository_with_semantic_executor_and_dirty_paths(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        dirty_path_hints,
        &executor,
    )
}

#[cfg(test)]
pub(crate) fn reindex_repository_with_semantic_executor(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<ReindexSummary> {
    reindex_repository_with_semantic_executor_and_dirty_paths(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        &[],
        executor,
    )
}

fn reindex_repository_with_semantic_executor_and_dirty_paths(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    dirty_path_hints: &[PathBuf],
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<ReindexSummary> {
    let started_at = Instant::now();
    let db_preexisted = db_path.exists();
    let manifest_store = ManifestStore::new(db_path);
    manifest_store.initialize_for_reindex(semantic_runtime.enabled)?;
    let previous_manifest = if mode == ReindexMode::Full && !db_preexisted {
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
        ReindexMode::Full => manifest_builder.build_with_diagnostics(workspace_root)?,
        ReindexMode::ChangedOnly if previous_manifest.is_some() => manifest_builder
            .build_changed_only_with_hints_and_diagnostics(
                workspace_root,
                previous_entries,
                dirty_path_hints,
            )?,
        ReindexMode::ChangedOnly => manifest_builder.build_with_diagnostics(workspace_root)?,
    };
    let current_manifest = manifest_output.entries;
    let diagnostics = ReindexDiagnostics {
        entries: manifest_output.diagnostics,
    };
    let manifest_diff = if mode == ReindexMode::Full && previous_entries.is_empty() {
        ManifestDiff::default()
    } else {
        diff(previous_entries, &current_manifest)
    };
    let storage = semantic_runtime.enabled.then(|| Storage::new(db_path));
    let plan = build_reindex_plan(
        repository_id,
        workspace_root,
        mode,
        semantic_runtime,
        previous_snapshot_id.clone(),
        previous_manifest.is_some(),
        current_manifest,
        diagnostics,
        manifest_diff,
        storage.as_ref(),
    )?;

    execute_reindex_plan(
        &manifest_store,
        repository_id,
        workspace_root,
        db_path,
        &plan,
        semantic_runtime,
        credentials,
        executor,
    )?;

    Ok(ReindexSummary {
        repository_id: repository_id.to_owned(),
        snapshot_id: plan.snapshot_plan.snapshot_id().to_owned(),
        files_scanned: plan.files_scanned,
        files_changed: plan.files_changed,
        files_deleted: plan.files_deleted,
        diagnostics: plan.diagnostics,
        duration_ms: started_at.elapsed().as_millis(),
    })
}

fn execute_reindex_plan(
    manifest_store: &ManifestStore,
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    plan: &ReindexPlan,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<()> {
    let storage = Storage::new(db_path);
    execute_manifest_snapshot_phase(manifest_store, workspace_root, plan)?;
    execute_semantic_refresh_phase(
        manifest_store,
        repository_id,
        workspace_root,
        plan,
        semantic_runtime,
        credentials,
        executor,
        &storage,
    )?;
    execute_retention_phase(&storage, repository_id, plan)?;
    Ok(())
}

fn execute_manifest_snapshot_phase(
    manifest_store: &ManifestStore,
    workspace_root: &Path,
    plan: &ReindexPlan,
) -> FriggResult<()> {
    match &plan.snapshot_plan {
        ManifestSnapshotPlan::ReuseExisting { .. } => Ok(()),
        ManifestSnapshotPlan::PersistNew { snapshot_id, .. } => {
            manifest_store
                .upsert_repository(&plan.repository_id, workspace_root, &plan.repository_id)
                .map_err(|err| {
                    wrap_reindex_phase_error(ReindexExecutionPhase::PersistManifestSnapshot, err)
                })?;
            manifest_store
                .persist_snapshot_manifest(&plan.repository_id, snapshot_id, &plan.current_manifest)
                .map_err(|err| {
                    wrap_reindex_phase_error(ReindexExecutionPhase::PersistManifestSnapshot, err)
                })
        }
    }
}

fn execute_semantic_refresh_phase(
    manifest_store: &ManifestStore,
    repository_id: &str,
    workspace_root: &Path,
    plan: &ReindexPlan,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
    storage: &Storage,
) -> FriggResult<()> {
    if plan.semantic_refresh.mode == SemanticRefreshMode::Disabled {
        return Ok(());
    }

    let semantic_result = execute_semantic_refresh_plan(
        repository_id,
        workspace_root,
        plan.previous_snapshot_id.as_deref(),
        plan.snapshot_plan.snapshot_id(),
        &plan.semantic_refresh,
        semantic_runtime,
        credentials,
        executor,
        storage,
    );

    if let Err(err) = semantic_result {
        let semantic_error =
            wrap_reindex_phase_error(ReindexExecutionPhase::SemanticRefresh, err);
        if plan.snapshot_plan.rollback_on_semantic_failure() {
            if let Err(rollback_err) =
                execute_snapshot_rollback_phase(manifest_store, plan.snapshot_plan.snapshot_id())
            {
                return Err(FriggError::Internal(format!(
                    "{semantic_error}; {}",
                    wrap_reindex_phase_error(
                        ReindexExecutionPhase::RollbackManifestSnapshot,
                        rollback_err,
                    )
                )));
            }
        }
        return Err(semantic_error);
    }

    Ok(())
}

fn execute_snapshot_rollback_phase(
    manifest_store: &ManifestStore,
    snapshot_id: &str,
) -> FriggResult<()> {
    manifest_store.delete_snapshot(snapshot_id)
}

fn execute_retention_phase(
    storage: &Storage,
    repository_id: &str,
    plan: &ReindexPlan,
) -> FriggResult<()> {
    storage
        .prune_repository_snapshots(repository_id, plan.retained_manifest_snapshots)
        .map_err(|err| wrap_reindex_phase_error(ReindexExecutionPhase::PruneManifestSnapshots, err))?;
    Ok(())
}

fn wrap_reindex_phase_error(phase: ReindexExecutionPhase, err: FriggError) -> FriggError {
    let prefix = |message: String| format!("reindex execution failed phase={}: {message}", phase.as_str());

    match err {
        FriggError::InvalidInput(message) => FriggError::InvalidInput(prefix(message)),
        FriggError::NotFound(message) => FriggError::NotFound(prefix(message)),
        FriggError::AccessDenied(message) => FriggError::AccessDenied(prefix(message)),
        FriggError::Internal(message) => FriggError::Internal(prefix(message)),
        FriggError::StrictSemanticFailure { reason } => FriggError::StrictSemanticFailure {
            reason: prefix(reason),
        },
        FriggError::Io(err) => FriggError::Io(std::io::Error::new(err.kind(), prefix(err.to_string()))),
    }
}

fn build_reindex_plan(
    repository_id: &str,
    workspace_root: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    previous_snapshot_id: Option<String>,
    had_previous_manifest: bool,
    current_manifest: Vec<FileDigest>,
    diagnostics: ReindexDiagnostics,
    manifest_diff: ManifestDiff,
    storage: Option<&Storage>,
) -> FriggResult<ReindexPlan> {
    let files_scanned = current_manifest.len();
    let files_changed = match mode {
        ReindexMode::Full => files_scanned,
        ReindexMode::ChangedOnly => manifest_diff.added.len() + manifest_diff.modified.len(),
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
    let semantic_refresh = build_semantic_refresh_plan(
        repository_id,
        workspace_root,
        mode,
        semantic_runtime,
        previous_snapshot_id.as_deref(),
        had_previous_manifest,
        snapshot_plan.snapshot_id(),
        &current_manifest,
        &manifest_diff,
        storage,
    )?;

    Ok(ReindexPlan {
        repository_id: repository_id.to_owned(),
        mode,
        previous_snapshot_id,
        current_manifest,
        manifest_diff,
        snapshot_plan,
        semantic_refresh,
        diagnostics,
        files_scanned,
        files_changed,
        files_deleted,
        retained_manifest_snapshots: DEFAULT_RETAINED_MANIFEST_SNAPSHOTS,
    })
}

fn build_manifest_snapshot_plan(
    repository_id: &str,
    mode: ReindexMode,
    files_changed: usize,
    files_deleted: usize,
    previous_snapshot_id: Option<&str>,
    current_manifest: &[FileDigest],
) -> FriggResult<ManifestSnapshotPlan> {
    if mode == ReindexMode::ChangedOnly
        && files_changed == 0
        && files_deleted == 0
        && previous_snapshot_id.is_some()
    {
        return Ok(ManifestSnapshotPlan::ReuseExisting {
            snapshot_id: previous_snapshot_id
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
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

fn build_semantic_refresh_plan(
    repository_id: &str,
    workspace_root: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    previous_snapshot_id: Option<&str>,
    had_previous_manifest: bool,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
    manifest_diff: &ManifestDiff,
    storage: Option<&Storage>,
) -> FriggResult<SemanticRefreshPlan> {
    if !semantic_runtime.enabled {
        return Ok(SemanticRefreshPlan {
            mode: SemanticRefreshMode::Disabled,
            provider: None,
            model: None,
            records_manifest: Vec::new(),
            changed_paths: Vec::new(),
            deleted_paths: Vec::new(),
        });
    }

    let provider = semantic_runtime
        .provider
        .ok_or_else(|| {
            FriggError::Internal("semantic runtime provider missing after validation".to_owned())
        })?
        .as_str()
        .to_owned();
    let model = semantic_runtime
        .normalized_model()
        .ok_or_else(|| {
            FriggError::Internal("semantic runtime model missing after validation".to_owned())
        })?
        .to_owned();
    let semantic_head_snapshot_id = match storage {
        Some(storage) => storage
            .load_semantic_head_for_repository_model(repository_id, &provider, &model)?
            .map(|head| head.covered_snapshot_id),
        None => None,
    };
    let requires_full_semantic_refresh = semantic_head_snapshot_id.as_deref() != Some(snapshot_id)
        && semantic_head_snapshot_id.as_deref() != previous_snapshot_id;

    let changed_paths = manifest_diff
        .added
        .iter()
        .chain(manifest_diff.modified.iter())
        .map(|digest| normalize_repository_relative_path(workspace_root, &digest.path))
        .collect::<FriggResult<Vec<_>>>()?;
    let deleted_paths = manifest_diff
        .deleted
        .iter()
        .map(|digest| normalize_repository_relative_path(workspace_root, &digest.path))
        .collect::<FriggResult<Vec<_>>>()?;

    let (mode, records_manifest) = match mode {
        ReindexMode::Full => (SemanticRefreshMode::FullRebuild, current_manifest.to_vec()),
        ReindexMode::ChangedOnly if requires_full_semantic_refresh => (
            SemanticRefreshMode::FullRebuildFromChangedOnly,
            current_manifest.to_vec(),
        ),
        ReindexMode::ChangedOnly
            if !changed_paths.is_empty() || !deleted_paths.is_empty() || !had_previous_manifest =>
        {
            (
                SemanticRefreshMode::IncrementalAdvance,
                manifest_diff
                    .added
                    .iter()
                    .chain(manifest_diff.modified.iter())
                    .cloned()
                    .collect(),
            )
        }
        ReindexMode::ChangedOnly => (SemanticRefreshMode::ReuseExisting, Vec::new()),
    };

    Ok(SemanticRefreshPlan {
        mode,
        provider: Some(provider),
        model: Some(model),
        records_manifest,
        changed_paths,
        deleted_paths,
    })
}

fn execute_semantic_refresh_plan(
    repository_id: &str,
    workspace_root: &Path,
    previous_snapshot_id: Option<&str>,
    snapshot_id: &str,
    semantic_refresh: &SemanticRefreshPlan,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
    storage: &Storage,
) -> FriggResult<()> {
    let provider = semantic_refresh.provider.as_deref().ok_or_else(|| {
        FriggError::Internal("semantic refresh plan missing provider".to_owned())
    })?;
    let model = semantic_refresh
        .model
        .as_deref()
        .ok_or_else(|| FriggError::Internal("semantic refresh plan missing model".to_owned()))?;

    match semantic_refresh.mode {
        SemanticRefreshMode::Disabled | SemanticRefreshMode::ReuseExisting => Ok(()),
        SemanticRefreshMode::FullRebuild | SemanticRefreshMode::FullRebuildFromChangedOnly => {
            build_semantic_embedding_records(
                repository_id,
                workspace_root,
                snapshot_id,
                &semantic_refresh.records_manifest,
                semantic_runtime,
                credentials,
                executor,
            )
            .and_then(|semantic_records| {
                storage.replace_semantic_embeddings_for_repository(
                    repository_id,
                    snapshot_id,
                    provider,
                    model,
                    &semantic_records,
                )
            })
        }
        SemanticRefreshMode::IncrementalAdvance => build_semantic_embedding_records(
            repository_id,
            workspace_root,
            snapshot_id,
            &semantic_refresh.records_manifest,
            semantic_runtime,
            credentials,
            executor,
        )
        .and_then(|semantic_records| {
            storage.advance_semantic_embeddings_for_repository(
                repository_id,
                previous_snapshot_id,
                snapshot_id,
                provider,
                model,
                &semantic_refresh.changed_paths,
                &semantic_refresh.deleted_paths,
                &semantic_records,
            )
        }),
    }
}

#[cfg(test)]
pub(crate) fn build_reindex_plan_for_tests(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    dirty_path_hints: &[PathBuf],
) -> FriggResult<ReindexPlan> {
    let db_preexisted = db_path.exists();
    let manifest_store = ManifestStore::new(db_path);
    manifest_store.initialize_for_reindex(semantic_runtime.enabled)?;
    let previous_manifest = if mode == ReindexMode::Full && !db_preexisted {
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
        ReindexMode::Full => manifest_builder.build_with_diagnostics(workspace_root)?,
        ReindexMode::ChangedOnly if previous_manifest.is_some() => manifest_builder
            .build_changed_only_with_hints_and_diagnostics(
                workspace_root,
                previous_entries,
                dirty_path_hints,
            )?,
        ReindexMode::ChangedOnly => manifest_builder.build_with_diagnostics(workspace_root)?,
    };
    let current_manifest = manifest_output.entries;
    let diagnostics = ReindexDiagnostics {
        entries: manifest_output.diagnostics,
    };
    let manifest_diff = if mode == ReindexMode::Full && previous_entries.is_empty() {
        ManifestDiff::default()
    } else {
        diff(previous_entries, &current_manifest)
    };
    let storage = semantic_runtime.enabled.then(|| Storage::new(db_path));

    build_reindex_plan(
        repository_id,
        workspace_root,
        mode,
        semantic_runtime,
        previous_snapshot_id,
        previous_manifest.is_some(),
        current_manifest,
        diagnostics,
        manifest_diff,
        storage.as_ref(),
    )
}
