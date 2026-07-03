//! Ordered index execution across manifest persistence, retrieval projections, and semantic refresh.

use std::path::Path;
use std::time::Instant;

use crate::domain::{FriggError, FriggResult};
use crate::searcher::{build_retrieval_projection_bundle, required_retrieval_projection_versions};
use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials};
use crate::storage::StorageSession;
use tracing::warn;

use super::super::manifest::{file_digest_to_manifest_entry, normalize_repository_relative_path};
use super::plan::{
    IndexPlan, IndexProgressEvent, IndexProgressPhase, IndexProgressStatus, ManifestSnapshotPlan,
    SemanticRefreshMode,
};
use super::semantic::execute_semantic_refresh_plan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexExecutionPhase {
    PersistManifestSnapshot,
    RefreshRetrievalProjections,
    SemanticRefresh,
    RollbackManifestSnapshot,
    PruneManifestSnapshots,
}

impl IndexExecutionPhase {
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

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_index_plan(
    storage_session: &mut StorageSession,
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    plan: &IndexPlan,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn crate::indexer::semantic::SemanticRuntimeEmbeddingExecutor,
    on_progress: &mut impl FnMut(IndexProgressEvent),
) -> FriggResult<()> {
    emit_index_execution_progress(
        on_progress,
        plan,
        IndexProgressPhase::PersistManifestSnapshot,
        IndexProgressStatus::Starting,
    );
    let manifest_snapshot_started_at = Instant::now();
    let manifest_written = execute_manifest_snapshot_phase(storage_session, workspace_root, plan)?;
    on_progress(
        index_execution_progress_event(
            plan,
            IndexProgressPhase::PersistManifestSnapshot,
            if manifest_written {
                IndexProgressStatus::Ok
            } else {
                IndexProgressStatus::Skipped
            },
        )
        .with_duration_ms(manifest_snapshot_started_at.elapsed().as_millis()),
    );

    emit_index_execution_progress(
        on_progress,
        plan,
        IndexProgressPhase::RefreshRetrievalProjections,
        IndexProgressStatus::Starting,
    );
    let retrieval_projection_started_at = Instant::now();
    let projections_written =
        execute_retrieval_projection_phase(repository_id, workspace_root, plan, storage_session)?;
    on_progress(
        index_execution_progress_event(
            plan,
            IndexProgressPhase::RefreshRetrievalProjections,
            if projections_written {
                IndexProgressStatus::Ok
            } else {
                IndexProgressStatus::Skipped
            },
        )
        .with_duration_ms(retrieval_projection_started_at.elapsed().as_millis()),
    );

    let semantic_started_at = Instant::now();
    if matches!(
        plan.semantic_refresh.mode,
        SemanticRefreshMode::Disabled | SemanticRefreshMode::ReuseExisting
    ) {
        on_progress(
            index_execution_progress_event(
                plan,
                IndexProgressPhase::SemanticRefresh,
                IndexProgressStatus::Skipped,
            )
            .with_duration_ms(semantic_started_at.elapsed().as_millis()),
        );
    } else {
        emit_index_execution_progress(
            on_progress,
            plan,
            IndexProgressPhase::SemanticRefresh,
            IndexProgressStatus::Starting,
        );
    }
    let semantics_written = execute_semantic_refresh_phase(
        storage_session,
        repository_id,
        workspace_root,
        plan,
        semantic_runtime,
        credentials,
        executor,
    )?;
    if semantics_written {
        on_progress(
            index_execution_progress_event(
                plan,
                IndexProgressPhase::SemanticRefresh,
                IndexProgressStatus::Ok,
            )
            .with_duration_ms(semantic_started_at.elapsed().as_millis()),
        );
    }

    emit_index_execution_progress(
        on_progress,
        plan,
        IndexProgressPhase::PruneManifestSnapshots,
        IndexProgressStatus::Starting,
    );
    let retention_started_at = Instant::now();
    let pruned_snapshots = execute_retention_phase(storage_session, repository_id, plan)?;
    let retention_status = if pruned_snapshots == 0 {
        IndexProgressStatus::Skipped
    } else {
        IndexProgressStatus::Ok
    };
    on_progress(
        index_execution_progress_event(
            plan,
            IndexProgressPhase::PruneManifestSnapshots,
            retention_status,
        )
        .with_pruned_snapshots(pruned_snapshots)
        .with_duration_ms(retention_started_at.elapsed().as_millis()),
    );
    if manifest_written || projections_written || semantics_written || pruned_snapshots > 0 {
        emit_index_execution_progress(
            on_progress,
            plan,
            IndexProgressPhase::CheckpointWal,
            IndexProgressStatus::Starting,
        );
        let checkpoint_started_at = Instant::now();
        match storage_session.checkpoint_wal_truncate() {
            Ok(()) => on_progress(
                index_execution_progress_event(
                    plan,
                    IndexProgressPhase::CheckpointWal,
                    IndexProgressStatus::Ok,
                )
                .with_duration_ms(checkpoint_started_at.elapsed().as_millis()),
            ),
            Err(err) => {
                warn!(
                    db_path = %db_path.display(),
                    error = %err,
                    "sqlite wal checkpoint after index run failed"
                );
                on_progress(
                    index_execution_progress_event(
                        plan,
                        IndexProgressPhase::CheckpointWal,
                        IndexProgressStatus::Warning,
                    )
                    .with_duration_ms(checkpoint_started_at.elapsed().as_millis()),
                );
            }
        }
    }
    Ok(())
}

fn emit_index_execution_progress(
    on_progress: &mut impl FnMut(IndexProgressEvent),
    plan: &IndexPlan,
    phase: IndexProgressPhase,
    status: IndexProgressStatus,
) {
    on_progress(index_execution_progress_event(plan, phase, status));
}

fn index_execution_progress_event(
    plan: &IndexPlan,
    phase: IndexProgressPhase,
    status: IndexProgressStatus,
) -> IndexProgressEvent {
    let changed_paths = if phase == IndexProgressPhase::SemanticRefresh {
        plan.semantic_refresh.changed_paths.len()
    } else {
        plan.changed_paths.len()
    };
    let deleted_paths = if phase == IndexProgressPhase::SemanticRefresh {
        plan.semantic_refresh.deleted_paths.len()
    } else {
        plan.deleted_paths.len()
    };
    IndexProgressEvent::new(&plan.repository_id, plan.mode, phase, status)
        .with_snapshot(plan.snapshot_plan.snapshot_id())
        .with_previous_snapshot(plan.previous_snapshot_id.as_deref())
        .with_file_counts(plan.files_scanned, plan.files_changed, plan.files_deleted)
        .with_diagnostics(plan.diagnostics.total_count())
        .with_records(plan.semantic_refresh.records_manifest.len())
        .with_path_counts(changed_paths, deleted_paths)
}

fn execute_manifest_snapshot_phase(
    storage_session: &mut StorageSession,
    workspace_root: &Path,
    plan: &IndexPlan,
) -> FriggResult<bool> {
    match &plan.snapshot_plan {
        ManifestSnapshotPlan::ReuseExisting { .. } => Ok(false),
        ManifestSnapshotPlan::PersistNew { snapshot_id, .. } => {
            storage_session
                .upsert_repository(&plan.repository_id, workspace_root, &plan.repository_id)
                .map_err(|err| {
                    wrap_index_phase_error(IndexExecutionPhase::PersistManifestSnapshot, err)
                })?;
            let manifest_entries = plan
                .current_manifest
                .iter()
                .map(file_digest_to_manifest_entry)
                .collect::<Vec<_>>();
            storage_session
                .upsert_manifest(&plan.repository_id, snapshot_id, &manifest_entries)
                .map_err(|err| {
                    wrap_index_phase_error(IndexExecutionPhase::PersistManifestSnapshot, err)
                })?;
            Ok(true)
        }
    }
}

fn execute_retrieval_projection_phase(
    repository_id: &str,
    workspace_root: &Path,
    plan: &IndexPlan,
    storage_session: &mut StorageSession,
) -> FriggResult<bool> {
    let snapshot_id = plan.snapshot_plan.snapshot_id();
    let should_refresh = match &plan.snapshot_plan {
        ManifestSnapshotPlan::PersistNew { .. } => true,
        ManifestSnapshotPlan::ReuseExisting { .. } => !storage_session
            .stale_or_missing_retrieval_projection_families_for_repository_snapshot(
                repository_id,
                snapshot_id,
                &required_retrieval_projection_versions(),
            )
            .map_err(|err| {
                wrap_index_phase_error(IndexExecutionPhase::RefreshRetrievalProjections, err)
            })?
            .is_empty(),
    };
    if !should_refresh {
        return Ok(false);
    }

    let manifest_paths = plan
        .current_manifest
        .iter()
        .map(|entry| normalize_repository_relative_path(workspace_root, &entry.path))
        .collect::<FriggResult<Vec<_>>>()?;

    let projection_bundle =
        build_retrieval_projection_bundle(repository_id, workspace_root, &manifest_paths)
            .map_err(|err| {
                wrap_index_phase_error(IndexExecutionPhase::RefreshRetrievalProjections, err)
            })
            .and_then(|bundle| {
                storage_session
                    .replace_retrieval_projection_bundle_for_repository_snapshot(
                        repository_id,
                        snapshot_id,
                        bundle,
                    )
                    .map_err(|err| {
                        wrap_index_phase_error(
                            IndexExecutionPhase::RefreshRetrievalProjections,
                            err,
                        )
                    })
            });

    if let Err(err) = projection_bundle {
        if matches!(plan.snapshot_plan, ManifestSnapshotPlan::PersistNew { .. }) {
            if let Err(rollback_err) = execute_snapshot_rollback_phase(storage_session, snapshot_id)
            {
                return Err(FriggError::Internal(format!(
                    "{err}; {}",
                    wrap_index_phase_error(
                        IndexExecutionPhase::RollbackManifestSnapshot,
                        rollback_err,
                    )
                )));
            }
        }
        return Err(err);
    }

    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn execute_semantic_refresh_phase(
    storage_session: &mut StorageSession,
    repository_id: &str,
    workspace_root: &Path,
    plan: &IndexPlan,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn crate::indexer::semantic::SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<bool> {
    if plan.semantic_refresh.mode == SemanticRefreshMode::Disabled {
        return Ok(false);
    }

    let semantic_result = execute_semantic_refresh_plan(
        repository_id,
        workspace_root,
        plan.semantic_refresh.advance_from_snapshot_id.as_deref(),
        plan.snapshot_plan.snapshot_id(),
        &plan.semantic_refresh,
        semantic_runtime,
        credentials,
        executor,
        storage_session,
    );

    if let Err(err) = semantic_result {
        let semantic_error = wrap_index_phase_error(IndexExecutionPhase::SemanticRefresh, err);
        if plan.snapshot_plan.rollback_on_semantic_failure() {
            if let Err(rollback_err) =
                execute_snapshot_rollback_phase(storage_session, plan.snapshot_plan.snapshot_id())
            {
                return Err(FriggError::Internal(format!(
                    "{semantic_error}; {}",
                    wrap_index_phase_error(
                        IndexExecutionPhase::RollbackManifestSnapshot,
                        rollback_err,
                    )
                )));
            }
        }
        return Err(semantic_error);
    }

    Ok(!matches!(
        plan.semantic_refresh.mode,
        SemanticRefreshMode::Disabled | SemanticRefreshMode::ReuseExisting
    ))
}

fn execute_snapshot_rollback_phase(
    storage_session: &mut StorageSession,
    snapshot_id: &str,
) -> FriggResult<()> {
    storage_session.delete_snapshot(snapshot_id)
}

fn execute_retention_phase(
    storage_session: &mut StorageSession,
    repository_id: &str,
    plan: &IndexPlan,
) -> FriggResult<usize> {
    let deleted = storage_session
        .prune_repository_snapshots(repository_id, plan.retained_manifest_snapshots)
        .map_err(|err| wrap_index_phase_error(IndexExecutionPhase::PruneManifestSnapshots, err))?;
    Ok(deleted)
}

fn wrap_index_phase_error(phase: IndexExecutionPhase, err: FriggError) -> FriggError {
    let prefix =
        |message: String| format!("index execution failed phase={}: {message}", phase.as_str());

    match err {
        FriggError::InvalidInput(message) => FriggError::InvalidInput(prefix(message)),
        FriggError::NotFound(message) => FriggError::NotFound(prefix(message)),
        FriggError::AccessDenied(message) => FriggError::AccessDenied(prefix(message)),
        FriggError::Internal(message) => FriggError::Internal(prefix(message)),
        schema_error @ FriggError::StorageSchemaIncompatible { .. } => schema_error,
        FriggError::StrictSemanticFailure { reason } => FriggError::StrictSemanticFailure {
            reason: prefix(reason),
        },
        FriggError::Io(err) => {
            FriggError::Io(std::io::Error::new(err.kind(), prefix(err.to_string())))
        }
    }
}
