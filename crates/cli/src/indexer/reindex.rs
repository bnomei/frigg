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
    let files_scanned = current_manifest.len();
    let files_changed = match mode {
        ReindexMode::Full => files_scanned,
        ReindexMode::ChangedOnly => manifest_diff.added.len() + manifest_diff.modified.len(),
    };
    let files_deleted = manifest_diff.deleted.len();

    let snapshot_id = if mode == ReindexMode::ChangedOnly
        && files_changed == 0
        && files_deleted == 0
        && previous_manifest.is_some()
    {
        previous_manifest
            .as_ref()
            .map(|manifest| manifest.snapshot_id.clone())
            .ok_or_else(|| {
                FriggError::Internal(
                    "failed to resolve previous snapshot identifier for unchanged manifest"
                        .to_owned(),
                )
            })?
    } else {
        let snapshot_id = deterministic_snapshot_id(repository_id, &current_manifest);
        manifest_store.persist_snapshot_manifest(repository_id, &snapshot_id, &current_manifest)?;
        snapshot_id
    };
    let rollback_snapshot_on_semantic_failure = previous_snapshot_id
        .as_deref()
        .map(|previous| previous != snapshot_id)
        .unwrap_or(true);

    if semantic_runtime.enabled {
        let storage = Storage::new(db_path);
        let provider = semantic_runtime.provider.ok_or_else(|| {
            FriggError::Internal("semantic runtime provider missing after validation".to_owned())
        })?;
        let model = semantic_runtime.normalized_model().ok_or_else(|| {
            FriggError::Internal("semantic runtime model missing after validation".to_owned())
        })?;
        let current_semantic_head = storage.load_semantic_head_for_repository_model(
            repository_id,
            provider.as_str(),
            model,
        )?;
        let semantic_head_snapshot_id = current_semantic_head
            .as_ref()
            .map(|head| head.covered_snapshot_id.as_str());
        let requires_full_semantic_refresh = semantic_head_snapshot_id
            != Some(snapshot_id.as_str())
            && semantic_head_snapshot_id != previous_snapshot_id.as_deref();
        let semantic_result = match mode {
            ReindexMode::Full => build_semantic_embedding_records(
                repository_id,
                workspace_root,
                &snapshot_id,
                &current_manifest,
                semantic_runtime,
                credentials,
                executor,
            )
            .and_then(|semantic_records| {
                storage.replace_semantic_embeddings_for_repository(
                    repository_id,
                    &snapshot_id,
                    provider.as_str(),
                    model,
                    &semantic_records,
                )
            }),
            ReindexMode::ChangedOnly => {
                if requires_full_semantic_refresh {
                    build_semantic_embedding_records(
                        repository_id,
                        workspace_root,
                        &snapshot_id,
                        &current_manifest,
                        semantic_runtime,
                        credentials,
                        executor,
                    )
                    .and_then(|semantic_records| {
                        storage.replace_semantic_embeddings_for_repository(
                            repository_id,
                            &snapshot_id,
                            provider.as_str(),
                            model,
                            &semantic_records,
                        )
                    })
                } else if files_changed > 0 || files_deleted > 0 || previous_manifest.is_none() {
                    let semantic_manifest = manifest_diff
                        .added
                        .iter()
                        .chain(manifest_diff.modified.iter())
                        .cloned()
                        .collect::<Vec<_>>();
                    build_semantic_embedding_records(
                        repository_id,
                        workspace_root,
                        &snapshot_id,
                        &semantic_manifest,
                        semantic_runtime,
                        credentials,
                        executor,
                    )
                    .and_then(|semantic_records| {
                        let changed_paths = manifest_diff
                            .added
                            .iter()
                            .chain(manifest_diff.modified.iter())
                            .map(|digest| {
                                normalize_repository_relative_path(workspace_root, &digest.path)
                            })
                            .collect::<FriggResult<Vec<_>>>()?;
                        let deleted_paths = manifest_diff
                            .deleted
                            .iter()
                            .map(|digest| {
                                normalize_repository_relative_path(workspace_root, &digest.path)
                            })
                            .collect::<FriggResult<Vec<_>>>()?;
                        let previous_snapshot_id = previous_manifest
                            .as_ref()
                            .map(|manifest| manifest.snapshot_id.as_str());
                        storage.advance_semantic_embeddings_for_repository(
                            repository_id,
                            previous_snapshot_id,
                            &snapshot_id,
                            provider.as_str(),
                            model,
                            &changed_paths,
                            &deleted_paths,
                            &semantic_records,
                        )
                    })
                } else {
                    Ok(())
                }
            }
        };
        if let Err(err) = semantic_result {
            if rollback_snapshot_on_semantic_failure {
                if let Err(rollback_err) = manifest_store.delete_snapshot(&snapshot_id) {
                    return Err(FriggError::Internal(format!(
                        "{err}; failed to roll back snapshot '{snapshot_id}' after semantic reindex failure: {rollback_err}"
                    )));
                }
            }
            return Err(err);
        }
    }

    Storage::new(db_path)
        .prune_repository_snapshots(repository_id, DEFAULT_RETAINED_MANIFEST_SNAPSHOTS)?;

    Ok(ReindexSummary {
        repository_id: repository_id.to_owned(),
        snapshot_id,
        files_scanned,
        files_changed,
        files_deleted,
        diagnostics,
        duration_ms: started_at.elapsed().as_millis(),
    })
}
