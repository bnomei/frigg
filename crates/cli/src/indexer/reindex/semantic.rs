use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::domain::{FriggError, FriggResult};
use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials};
use crate::storage::Storage;

use super::super::manifest::{diff, normalize_repository_relative_path};
use super::super::semantic::{
    RuntimeSemanticEmbeddingExecutor, SemanticRuntimeEmbeddingExecutor,
    build_semantic_embedding_records, resolve_semantic_runtime_config_from_env,
};
use super::super::{
    FileDigest, ManifestBuilder, ManifestDiff, ReindexDiagnostics, SemanticRefreshMode,
    SemanticRefreshPlan,
};
use super::execution::execute_reindex_plan;
use super::plan::{ReindexMode, ReindexSummary, build_reindex_plan};
use super::store::ManifestStore;

/// Runs the standard repository refresh path using semantic runtime settings resolved from the
/// current process environment.
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

#[allow(clippy::too_many_arguments)]
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
        changed_paths: plan.semantic_refresh.changed_paths,
        deleted_paths: plan.semantic_refresh.deleted_paths,
        diagnostics: plan.diagnostics,
        duration_ms: started_at.elapsed().as_millis(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_semantic_refresh_plan(
    repository_id: &str,
    workspace_root: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    previous_snapshot_id: Option<&str>,
    had_previous_manifest: bool,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
    manifest_diff: &super::super::ManifestDiff,
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
    storage: &Storage,
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
