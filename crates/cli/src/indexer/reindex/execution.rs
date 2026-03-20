use std::path::Path;

use crate::domain::{FriggError, FriggResult};
use crate::searcher::build_retrieval_projection_bundle;
use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials};
use crate::storage::Storage;

use super::super::manifest::normalize_repository_relative_path;
use super::plan::{ManifestSnapshotPlan, ReindexPlan, SemanticRefreshMode};
use super::semantic::execute_semantic_refresh_plan;
use super::store::ManifestStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReindexExecutionPhase {
    PersistManifestSnapshot,
    RefreshRetrievalProjections,
    SemanticRefresh,
    RollbackManifestSnapshot,
    PruneManifestSnapshots,
}

impl ReindexExecutionPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::PersistManifestSnapshot => "persist_manifest_snapshot",
            Self::RefreshRetrievalProjections => "refresh_retrieval_projections",
            Self::SemanticRefresh => "semantic_refresh",
            Self::RollbackManifestSnapshot => "rollback_manifest_snapshot",
            Self::PruneManifestSnapshots => "prune_manifest_snapshots",
        }
    }
}

pub(super) fn execute_reindex_plan(
    manifest_store: &ManifestStore,
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    plan: &ReindexPlan,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn crate::indexer::semantic::SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<()> {
    let storage = Storage::new(db_path);
    execute_manifest_snapshot_phase(manifest_store, workspace_root, plan)?;
    execute_retrieval_projection_phase(
        manifest_store,
        repository_id,
        workspace_root,
        plan,
        &storage,
    )?;
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

fn execute_retrieval_projection_phase(
    manifest_store: &ManifestStore,
    repository_id: &str,
    workspace_root: &Path,
    plan: &ReindexPlan,
    storage: &Storage,
) -> FriggResult<()> {
    let snapshot_id = plan.snapshot_plan.snapshot_id();
    let should_refresh = match &plan.snapshot_plan {
        ManifestSnapshotPlan::PersistNew { .. } => true,
        ManifestSnapshotPlan::ReuseExisting { .. } => {
            storage
                .missing_retrieval_projection_families_for_repository_snapshot(
                    repository_id,
                    snapshot_id,
                )
                .map_err(|err| {
                    wrap_reindex_phase_error(
                        ReindexExecutionPhase::RefreshRetrievalProjections,
                        err,
                    )
                })?
                .is_empty()
                == false
        }
    };
    if !should_refresh {
        return Ok(());
    }

    let manifest_paths = plan
        .current_manifest
        .iter()
        .map(|entry| normalize_repository_relative_path(workspace_root, &entry.path))
        .collect::<FriggResult<Vec<_>>>()?;

    let projection_bundle =
        build_retrieval_projection_bundle(repository_id, workspace_root, &manifest_paths)
            .map_err(|err| {
                wrap_reindex_phase_error(ReindexExecutionPhase::RefreshRetrievalProjections, err)
            })
            .and_then(|bundle| {
                storage
                    .replace_retrieval_projection_bundle_for_repository_snapshot(
                        repository_id,
                        snapshot_id,
                        &bundle,
                    )
                    .map_err(|err| {
                        wrap_reindex_phase_error(
                            ReindexExecutionPhase::RefreshRetrievalProjections,
                            err,
                        )
                    })
            });

    if let Err(err) = projection_bundle {
        if matches!(plan.snapshot_plan, ManifestSnapshotPlan::PersistNew { .. }) {
            if let Err(rollback_err) = execute_snapshot_rollback_phase(manifest_store, snapshot_id)
            {
                return Err(FriggError::Internal(format!(
                    "{err}; {}",
                    wrap_reindex_phase_error(
                        ReindexExecutionPhase::RollbackManifestSnapshot,
                        rollback_err,
                    )
                )));
            }
        }
        return Err(err);
    }

    Ok(())
}

fn execute_semantic_refresh_phase(
    manifest_store: &ManifestStore,
    repository_id: &str,
    workspace_root: &Path,
    plan: &ReindexPlan,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn crate::indexer::semantic::SemanticRuntimeEmbeddingExecutor,
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
        let semantic_error = wrap_reindex_phase_error(ReindexExecutionPhase::SemanticRefresh, err);
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
        .map_err(|err| {
            wrap_reindex_phase_error(ReindexExecutionPhase::PruneManifestSnapshots, err)
        })?;
    Ok(())
}

fn wrap_reindex_phase_error(phase: ReindexExecutionPhase, err: FriggError) -> FriggError {
    let prefix = |message: String| {
        format!(
            "reindex execution failed phase={}: {message}",
            phase.as_str()
        )
    };

    match err {
        FriggError::InvalidInput(message) => FriggError::InvalidInput(prefix(message)),
        FriggError::NotFound(message) => FriggError::NotFound(prefix(message)),
        FriggError::AccessDenied(message) => FriggError::AccessDenied(prefix(message)),
        FriggError::Internal(message) => FriggError::Internal(prefix(message)),
        FriggError::StrictSemanticFailure { reason } => FriggError::StrictSemanticFailure {
            reason: prefix(reason),
        },
        FriggError::Io(err) => {
            FriggError::Io(std::io::Error::new(err.kind(), prefix(err.to_string())))
        }
    }
}
