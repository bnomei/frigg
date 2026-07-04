//! Repository index orchestration from plan execution through manifest and semantic writes.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::domain::{FriggError, FriggResult};
use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials};
use crate::storage::{Storage, StorageSession};

use super::super::manifest::{
    diff, manifest_entry_to_file_digest, normalize_deleted_repository_relative_path,
    normalize_repository_relative_path,
};
use super::super::semantic::{
    RuntimeSemanticEmbeddingExecutor, SemanticIndexStorage, SemanticRuntimeEmbeddingExecutor,
    build_semantic_embedding_records, resolve_semantic_runtime_config_from_env,
};
use super::super::{
    FileDigest, IndexDiagnostics, ManifestBuilder, ManifestDiff, SemanticRefreshMode,
    SemanticRefreshPlan,
};
use super::execution::execute_index_plan;
use super::plan::{
    IndexMode, IndexPlan, IndexProgressEvent, IndexProgressPhase, IndexProgressStatus,
    IndexSummary, build_index_plan,
};
use super::store::ManifestStore;

/// Runs the standard repository refresh path using semantic runtime settings resolved from the
/// current process environment.
pub fn index_repository(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: IndexMode,
) -> FriggResult<IndexSummary> {
    let semantic_runtime = resolve_semantic_runtime_config_from_env()?;
    let credentials = SemanticRuntimeCredentials::from_process_env();
    index_repository_with_runtime_config(
        repository_id,
        workspace_root,
        db_path,
        mode,
        &semantic_runtime,
        &credentials,
    )
}

/// Runs repository index with caller-supplied semantic runtime configuration.
pub fn index_repository_with_runtime_config(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: IndexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
) -> FriggResult<IndexSummary> {
    index_repository_with_runtime_config_and_dirty_paths(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        &[],
    )
}

/// Runs repository index and exposes the computed plan before writes or semantic refresh execute.
#[allow(clippy::too_many_arguments)]
pub fn index_repository_with_runtime_config_and_plan_callback(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: IndexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    on_plan: impl FnOnce(&IndexPlan) -> FriggResult<()>,
) -> FriggResult<IndexSummary> {
    index_repository_with_runtime_config_and_dirty_paths_and_plan_callback(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        &[],
        on_plan,
    )
}

/// Runs repository index with explicit dirty-path hints for changed-only manifest builds.
pub fn index_repository_with_runtime_config_and_dirty_paths(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: IndexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    dirty_path_hints: &[PathBuf],
) -> FriggResult<IndexSummary> {
    index_repository_with_runtime_config_and_dirty_paths_and_plan_callback(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        dirty_path_hints,
        |_| Ok(()),
    )
}

/// Runs repository index with dirty-path hints and exposes the computed plan before writes.
#[allow(clippy::too_many_arguments)]
pub fn index_repository_with_runtime_config_and_dirty_paths_and_plan_callback(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: IndexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    dirty_path_hints: &[PathBuf],
    on_plan: impl FnOnce(&IndexPlan) -> FriggResult<()>,
) -> FriggResult<IndexSummary> {
    index_repository_with_runtime_config_and_dirty_paths_and_progress_callback(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        dirty_path_hints,
        on_plan,
        |_| {},
    )
}

/// Runs repository index with dirty-path hints and exposes plan plus fire-and-forget progress.
#[allow(clippy::too_many_arguments)]
pub fn index_repository_with_runtime_config_and_dirty_paths_and_progress_callback(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: IndexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    dirty_path_hints: &[PathBuf],
    on_plan: impl FnOnce(&IndexPlan) -> FriggResult<()>,
    on_progress: impl FnMut(IndexProgressEvent),
) -> FriggResult<IndexSummary> {
    let executor = RuntimeSemanticEmbeddingExecutor::new(credentials.clone());
    index_repository_with_semantic_executor_and_dirty_paths(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        dirty_path_hints,
        &executor,
        on_plan,
        on_progress,
    )
}

#[cfg(test)]
pub(crate) fn index_repository_with_semantic_executor(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: IndexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<IndexSummary> {
    index_repository_with_semantic_executor_and_dirty_paths(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        &[],
        executor,
        |_| Ok(()),
        |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
fn index_repository_with_semantic_executor_and_dirty_paths(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: IndexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    dirty_path_hints: &[PathBuf],
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
    on_plan: impl FnOnce(&IndexPlan) -> FriggResult<()>,
    mut on_progress: impl FnMut(IndexProgressEvent),
) -> FriggResult<IndexSummary> {
    let started_at = Instant::now();
    let db_preexisted = db_path.exists();
    let initialize_storage_started_at = Instant::now();
    on_progress(IndexProgressEvent::new(
        repository_id,
        mode,
        IndexProgressPhase::InitializeStorage,
        IndexProgressStatus::Starting,
    ));
    let manifest_store = ManifestStore::new(db_path);
    manifest_store.initialize_for_index(semantic_runtime.enabled)?;
    let storage = Storage::new(db_path);
    let mut storage_session = storage.open_session()?;
    on_progress(
        IndexProgressEvent::new(
            repository_id,
            mode,
            IndexProgressPhase::InitializeStorage,
            IndexProgressStatus::Ok,
        )
        .with_duration_ms(initialize_storage_started_at.elapsed().as_millis()),
    );

    let load_manifest_started_at = Instant::now();
    on_progress(IndexProgressEvent::new(
        repository_id,
        mode,
        IndexProgressPhase::LoadManifest,
        IndexProgressStatus::Starting,
    ));
    let previous_manifest = if mode == IndexMode::Full && !db_preexisted {
        None
    } else {
        storage_session
            .load_latest_manifest_for_repository(repository_id)?
            .map(|snapshot| super::super::RepositoryManifest {
                repository_id: snapshot.repository_id,
                snapshot_id: snapshot.snapshot_id,
                entries: snapshot
                    .entries
                    .into_iter()
                    .map(manifest_entry_to_file_digest)
                    .collect(),
            })
    };
    let previous_snapshot_id = previous_manifest
        .as_ref()
        .map(|manifest| manifest.snapshot_id.clone());
    let previous_entries = previous_manifest
        .as_ref()
        .map(|manifest| manifest.entries.as_slice())
        .unwrap_or(&[]);
    on_progress(
        IndexProgressEvent::new(
            repository_id,
            mode,
            IndexProgressPhase::LoadManifest,
            IndexProgressStatus::Ok,
        )
        .with_previous_snapshot(previous_snapshot_id.as_deref())
        .with_file_counts(previous_entries.len(), 0, 0)
        .with_duration_ms(load_manifest_started_at.elapsed().as_millis()),
    );

    let manifest_builder = ManifestBuilder::default();
    let build_manifest_started_at = Instant::now();
    on_progress(IndexProgressEvent::new(
        repository_id,
        mode,
        IndexProgressPhase::BuildManifest,
        IndexProgressStatus::Starting,
    ));
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
    on_progress(
        IndexProgressEvent::new(
            repository_id,
            mode,
            IndexProgressPhase::BuildManifest,
            IndexProgressStatus::Ok,
        )
        .with_file_counts(current_manifest.len(), 0, 0)
        .with_diagnostics(diagnostics.total_count())
        .with_duration_ms(build_manifest_started_at.elapsed().as_millis()),
    );
    let build_plan_started_at = Instant::now();
    on_progress(IndexProgressEvent::new(
        repository_id,
        mode,
        IndexProgressPhase::BuildPlan,
        IndexProgressStatus::Starting,
    ));
    let manifest_diff = if mode == IndexMode::Full && previous_entries.is_empty() {
        ManifestDiff::default()
    } else {
        diff(previous_entries, &current_manifest)
    };
    let semantic_storage = semantic_runtime
        .enabled
        .then_some(&storage_session as &dyn SemanticIndexStorage);
    let plan = build_index_plan(
        repository_id,
        workspace_root,
        mode,
        semantic_runtime,
        previous_snapshot_id.clone(),
        previous_manifest.is_some(),
        current_manifest,
        diagnostics,
        manifest_diff,
        dirty_path_hints,
        semantic_storage,
    )?;
    on_progress(
        IndexProgressEvent::new(
            repository_id,
            mode,
            IndexProgressPhase::BuildPlan,
            IndexProgressStatus::Ok,
        )
        .with_snapshot(plan.snapshot_plan.snapshot_id())
        .with_previous_snapshot(previous_snapshot_id.as_deref())
        .with_file_counts(plan.files_scanned, plan.files_changed, plan.files_deleted)
        .with_diagnostics(plan.diagnostics.total_count())
        .with_records(plan.semantic_refresh.records_manifest.len())
        .with_path_counts(plan.changed_paths.len(), plan.deleted_paths.len())
        .with_duration_ms(build_plan_started_at.elapsed().as_millis()),
    );

    on_plan(&plan)?;

    execute_index_plan(
        &mut storage_session,
        repository_id,
        workspace_root,
        db_path,
        &plan,
        semantic_runtime,
        credentials,
        executor,
        &mut on_progress,
    )?;

    Ok(IndexSummary {
        repository_id: repository_id.to_owned(),
        snapshot_id: plan.snapshot_plan.snapshot_id().to_owned(),
        previous_snapshot_id,
        snapshot_plan: plan.snapshot_plan.as_str().to_owned(),
        files_scanned: plan.files_scanned,
        files_changed: plan.files_changed,
        files_deleted: plan.files_deleted,
        changed_paths: plan.changed_paths,
        deleted_paths: plan.deleted_paths,
        semantic_refresh_mode: plan.semantic_refresh.mode,
        semantic_provider: plan.semantic_refresh.provider.clone(),
        semantic_model: plan.semantic_refresh.model.clone(),
        semantic_records: plan.semantic_refresh.records_manifest.len(),
        diagnostics: plan.diagnostics,
        duration_ms: started_at.elapsed().as_millis(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_semantic_refresh_plan(
    repository_id: &str,
    mode: IndexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    workspace_root: &Path,
    previous_snapshot_id: Option<&str>,
    had_previous_manifest: bool,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
    manifest_diff: &super::super::ManifestDiff,
    changed_paths: &[String],
    deleted_paths: &[String],
    dirty_path_hints: &[PathBuf],
    storage: Option<&dyn SemanticIndexStorage>,
) -> FriggResult<SemanticRefreshPlan> {
    if !semantic_runtime.enabled {
        return Ok(SemanticRefreshPlan {
            mode: SemanticRefreshMode::Disabled,
            provider: None,
            model: None,
            records_manifest: Vec::new(),
            changed_paths: Vec::new(),
            deleted_paths: Vec::new(),
            advance_from_snapshot_id: None,
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
    let mut semantic_manifest_diff = manifest_diff.clone();
    let mut semantic_changed_paths = changed_paths.to_vec();
    let mut semantic_deleted_paths = deleted_paths.to_vec();
    let mut advance_from_snapshot_id = previous_snapshot_id.map(ToOwned::to_owned);
    let mut requires_full_semantic_refresh = false;

    if mode == IndexMode::ChangedOnly {
        match semantic_head_snapshot_id.as_deref() {
            Some(head_snapshot_id) if head_snapshot_id == snapshot_id => {
                advance_from_snapshot_id = Some(head_snapshot_id.to_owned());
            }
            Some(head_snapshot_id) if Some(head_snapshot_id) == previous_snapshot_id => {
                advance_from_snapshot_id = Some(head_snapshot_id.to_owned());
            }
            None if previous_snapshot_id.is_none() => {
                advance_from_snapshot_id = None;
            }
            Some(head_snapshot_id) if !dirty_path_hints.is_empty() => match storage {
                Some(storage) => {
                    match semantic_delta_from_head_manifest(
                        workspace_root,
                        storage,
                        head_snapshot_id,
                        current_manifest,
                    )? {
                        Some((head_manifest_diff, head_changed_paths, head_deleted_paths)) => {
                            semantic_manifest_diff = head_manifest_diff;
                            semantic_changed_paths = head_changed_paths;
                            semantic_deleted_paths = head_deleted_paths;
                            advance_from_snapshot_id = Some(head_snapshot_id.to_owned());
                        }
                        None => {
                            requires_full_semantic_refresh = true;
                        }
                    }
                }
                None => {
                    requires_full_semantic_refresh = true;
                }
            },
            _ => {
                requires_full_semantic_refresh = true;
            }
        }
    }

    let has_unresolved_deleted_paths =
        semantic_manifest_diff.deleted.len() != semantic_deleted_paths.len();

    let (mode, records_manifest, changed_paths, deleted_paths, advance_from_snapshot_id) =
        match mode {
            IndexMode::Full => (
                SemanticRefreshMode::FullRebuild,
                current_manifest.to_vec(),
                Vec::new(),
                Vec::new(),
                None,
            ),
            IndexMode::ChangedOnly
                if requires_full_semantic_refresh || has_unresolved_deleted_paths =>
            {
                (
                    SemanticRefreshMode::FullRebuildFromChangedOnly,
                    current_manifest.to_vec(),
                    Vec::new(),
                    Vec::new(),
                    None,
                )
            }
            IndexMode::ChangedOnly
                if !semantic_changed_paths.is_empty()
                    || !semantic_deleted_paths.is_empty()
                    || !had_previous_manifest =>
            {
                (
                    SemanticRefreshMode::IncrementalAdvance,
                    semantic_manifest_diff
                        .added
                        .iter()
                        .chain(semantic_manifest_diff.modified.iter())
                        .cloned()
                        .collect(),
                    semantic_changed_paths,
                    semantic_deleted_paths,
                    advance_from_snapshot_id,
                )
            }
            IndexMode::ChangedOnly => (
                SemanticRefreshMode::ReuseExisting,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
            ),
        };

    Ok(SemanticRefreshPlan {
        mode,
        provider: Some(provider),
        model: Some(model),
        records_manifest,
        changed_paths,
        deleted_paths,
        advance_from_snapshot_id,
    })
}

fn semantic_delta_from_head_manifest(
    workspace_root: &Path,
    storage: &dyn SemanticIndexStorage,
    semantic_head_snapshot_id: &str,
    current_manifest: &[FileDigest],
) -> FriggResult<Option<(ManifestDiff, Vec<String>, Vec<String>)>> {
    let head_manifest = storage
        .load_manifest_for_snapshot(semantic_head_snapshot_id)?
        .into_iter()
        .map(manifest_entry_to_file_digest)
        .collect::<Vec<_>>();
    let manifest_diff = diff(&head_manifest, current_manifest);
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

    if manifest_diff.deleted.len() != deleted_paths.len() {
        return Ok(None);
    }

    Ok(Some((
        manifest_diff,
        dedup_semantic_paths(changed_paths),
        dedup_semantic_paths(deleted_paths),
    )))
}

fn dedup_semantic_paths(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_semantic_refresh_plan(
    repository_id: &str,
    workspace_root: &Path,
    previous_snapshot_id: Option<&str>,
    snapshot_id: &str,
    semantic_refresh: &SemanticRefreshPlan,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
    storage: &mut StorageSession,
) -> FriggResult<()> {
    let provider = semantic_refresh
        .provider
        .as_deref()
        .ok_or_else(|| FriggError::Internal("semantic refresh plan missing provider".to_owned()))?;
    let model = semantic_refresh
        .model
        .as_deref()
        .ok_or_else(|| FriggError::Internal("semantic refresh plan missing model".to_owned()))?;

    match semantic_refresh.mode {
        SemanticRefreshMode::Disabled | SemanticRefreshMode::ReuseExisting => Ok(()),
        SemanticRefreshMode::FullRebuild | SemanticRefreshMode::FullRebuildFromChangedOnly => {
            let semantic_build = build_semantic_embedding_records(
                repository_id,
                workspace_root,
                snapshot_id,
                &semantic_refresh.records_manifest,
                semantic_runtime,
                credentials,
                executor,
                Some(&*storage as &dyn SemanticIndexStorage),
            )?;
            storage.replace_semantic_embeddings_for_repository(
                repository_id,
                snapshot_id,
                provider,
                model,
                &semantic_build.records,
            )
        }
        SemanticRefreshMode::IncrementalAdvance => {
            let semantic_build = build_semantic_embedding_records(
                repository_id,
                workspace_root,
                snapshot_id,
                &semantic_refresh.records_manifest,
                semantic_runtime,
                credentials,
                executor,
                Some(&*storage as &dyn SemanticIndexStorage),
            )?;
            storage.advance_semantic_embeddings_for_repository(
                repository_id,
                previous_snapshot_id,
                snapshot_id,
                provider,
                model,
                &semantic_refresh.changed_paths,
                &semantic_refresh.deleted_paths,
                &semantic_build.records,
            )
        }
    }
}
