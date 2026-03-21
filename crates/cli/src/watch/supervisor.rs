use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};
use tracing::{info, warn};

use crate::domain::{FriggError, FriggResult};
use crate::indexer::{
    ReindexMode, reindex_repository_with_runtime_config,
    reindex_repository_with_runtime_config_and_dirty_paths,
};
use crate::manifest_validation::ValidatedManifestCandidateCache;
use crate::mcp::RuntimeTaskRegistry;
use crate::mcp::types::{RuntimeTaskKind, RuntimeTaskStatus};
use crate::mcp::workspace_registry::AttachedWorkspace;
use crate::settings::{
    FriggConfig, RuntimeTransportKind, SemanticRuntimeConfig, SemanticRuntimeCredentials,
};

use super::repository::{
    WatchedRepository, event_kind_is_relevant, repository_id_for_path, should_ignore_watch_path,
    startup_refresh_status, watched_repository_for_workspace,
};
use super::scheduler::{ScheduledRefresh, WatchRefreshClass, WatchSchedulerState};

const WATCH_TICK_MS: u64 = 50;

pub type RepositoryCacheInvalidationCallback = Arc<dyn Fn(&str) + Send + Sync + 'static>;

enum SupervisorCommand {
    Event(Event),
    LeaseAcquired {
        repository: WatchedRepository,
    },
    LeaseReleased {
        repository_id: String,
    },
    ReindexCompleted {
        repository_id: String,
        class: WatchRefreshClass,
        result: Result<crate::indexer::ReindexSummary, String>,
    },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WatchLeaseStatus {
    pub active: bool,
    pub lease_count: usize,
}

/// Shared filesystem watch supervisor for long-lived runtimes. It leases repository watches to
/// sessions so multiple clients can benefit from incremental freshness without duplicate watchers.
pub struct WatchRuntime {
    watcher: Mutex<RecommendedWatcher>,
    repositories: Arc<RwLock<BTreeMap<String, WatchedRepository>>>,
    lease_counts: Arc<RwLock<BTreeMap<String, usize>>>,
    supervisor_handle: JoinHandle<()>,
    command_tx: mpsc::UnboundedSender<SupervisorCommand>,
}

impl WatchRuntime {
    pub(crate) fn acquire_lease(&self, workspace: &AttachedWorkspace) -> FriggResult<usize> {
        let repository = watched_repository_for_workspace(workspace)?;

        let lease_count = {
            let mut lease_counts = self
                .lease_counts
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let count = lease_counts
                .entry(repository.repository_id.clone())
                .or_insert(0);
            *count = count.saturating_add(1);
            *count
        };

        if lease_count > 1 {
            return Ok(lease_count);
        }

        self.watcher
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .watch(&repository.root, RecursiveMode::Recursive)
            .map_err(|err| {
                let mut lease_counts = self
                    .lease_counts
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                lease_counts.remove(&repository.repository_id);
                FriggError::Internal(format!(
                    "failed to register watcher for root {}: {err}",
                    repository.root.display()
                ))
            })?;

        self.repositories
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(repository.repository_id.clone(), repository.clone());
        let _ = self
            .command_tx
            .send(SupervisorCommand::LeaseAcquired { repository });

        Ok(lease_count)
    }

    pub(crate) fn release_lease(&self, repository_id: &str) -> usize {
        let remaining = {
            let mut lease_counts = self
                .lease_counts
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(count) = lease_counts.get_mut(repository_id) else {
                return 0;
            };
            *count = count.saturating_sub(1);
            let remaining = *count;
            if remaining == 0 {
                lease_counts.remove(repository_id);
            }
            remaining
        };

        if remaining > 0 {
            return remaining;
        }

        if let Some(repository) = self
            .repositories
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(repository_id)
            && let Err(error) = self
                .watcher
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .unwatch(&repository.root)
        {
            warn!(
                repository_id,
                root = %repository.root.display(),
                error = %error,
                "built-in watch mode failed to unregister workspace root"
            );
        }

        let _ = self.command_tx.send(SupervisorCommand::LeaseReleased {
            repository_id: repository_id.to_owned(),
        });

        remaining
    }

    pub(crate) fn lease_status(&self, repository_id: &str) -> WatchLeaseStatus {
        let lease_count = self
            .lease_counts
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(repository_id)
            .copied()
            .unwrap_or(0);
        WatchLeaseStatus {
            active: lease_count > 0,
            lease_count,
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_test_event(&self, event: Event) {
        let _ = self.command_tx.send(SupervisorCommand::Event(event));
    }
}

impl Drop for WatchRuntime {
    fn drop(&mut self) {
        self.supervisor_handle.abort();
    }
}

/// Starts the shared watch supervisor only when the resolved runtime profile makes incremental
/// freshness worthwhile.
pub fn maybe_start_watch_runtime(
    config: &FriggConfig,
    transport: RuntimeTransportKind,
    task_registry: Arc<RwLock<RuntimeTaskRegistry>>,
    validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    repository_cache_invalidation_callback: Option<RepositoryCacheInvalidationCallback>,
) -> FriggResult<Option<WatchRuntime>> {
    if !config.watch.enabled_for_transport(transport) {
        info!(
            watch_mode = %config.watch.mode.as_str(),
            transport = ?transport,
            "built-in watch mode disabled for resolved transport"
        );
        return Ok(None);
    }

    let repositories = Arc::new(RwLock::new(BTreeMap::new()));
    let lease_counts = Arc::new(RwLock::new(BTreeMap::new()));
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let callback_tx = command_tx.clone();
    let watcher = notify::recommended_watcher(move |result| match result {
        Ok(event) => {
            let _ = callback_tx.send(SupervisorCommand::Event(event));
        }
        Err(error) => {
            warn!(error = %error, "built-in watch mode dropped notify event");
        }
    })
    .map_err(|err| FriggError::Internal(format!("failed to create filesystem watcher: {err}")))?;

    let watch_config = config.watch.clone();
    let semantic_runtime = config.semantic_runtime.clone();
    let semantic_credentials = SemanticRuntimeCredentials::from_process_env();
    let supervisor_handle = tokio::spawn(run_supervisor(
        Arc::clone(&repositories),
        watch_config,
        semantic_runtime,
        semantic_credentials,
        task_registry,
        validated_manifest_candidate_cache,
        repository_cache_invalidation_callback,
        command_rx,
        command_tx.clone(),
    ));

    Ok(Some(WatchRuntime {
        watcher: Mutex::new(watcher),
        repositories,
        lease_counts,
        supervisor_handle,
        command_tx,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn run_supervisor(
    repositories: Arc<RwLock<BTreeMap<String, WatchedRepository>>>,
    watch_config: crate::settings::WatchConfig,
    semantic_runtime: SemanticRuntimeConfig,
    semantic_credentials: SemanticRuntimeCredentials,
    task_registry: Arc<RwLock<RuntimeTaskRegistry>>,
    validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    repository_cache_invalidation_callback: Option<RepositoryCacheInvalidationCallback>,
    mut command_rx: mpsc::UnboundedReceiver<SupervisorCommand>,
    command_tx: mpsc::UnboundedSender<SupervisorCommand>,
) {
    let debounce = Duration::from_millis(watch_config.debounce_ms);
    let retry = Duration::from_millis(watch_config.retry_ms);
    let mut scheduler = WatchSchedulerState::new(0);
    let mut ticker = tokio::time::interval(Duration::from_millis(WATCH_TICK_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            maybe_command = command_rx.recv() => {
                let Some(command) = maybe_command else {
                    break;
                };
                let now = Instant::now();
                match command {
                    SupervisorCommand::Event(event) => handle_notify_event(
                        &repositories,
                        &mut scheduler,
                        &validated_manifest_candidate_cache,
                        repository_cache_invalidation_callback.as_ref(),
                        event,
                        now,
                        debounce,
                    ),
                    SupervisorCommand::LeaseAcquired { repository } => {
                        scheduler.add_repository(&repository.repository_id);
                        queue_startup_refresh_if_needed(
                            &repository,
                            &mut scheduler,
                            now,
                            &semantic_runtime,
                            &semantic_credentials,
                            watch_config.debounce_ms,
                        );
                    }
                    SupervisorCommand::LeaseReleased { repository_id } => {
                        scheduler.remove_repository(&repository_id);
                    }
                    SupervisorCommand::ReindexCompleted {
                        repository_id,
                        class,
                        result,
                    } => {
                        handle_reindex_completed(
                            &repositories,
                            &mut scheduler,
                            &repository_id,
                            class,
                            result,
                            repository_cache_invalidation_callback.as_ref(),
                            now,
                            retry,
                            &semantic_runtime,
                            &semantic_credentials,
                        );
                    }
                }
            }
            _ = ticker.tick() => {}
        }

        let now = Instant::now();
        if let Some(ScheduledRefresh {
            repository_id,
            class,
            ..
        }) = scheduler.next_ready_refresh(now)
        {
            let recent_paths = scheduler.mark_started(&repository_id, class);
            let repository = repositories
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&repository_id)
                .cloned();
            let Some(repository) = repository else {
                scheduler.mark_succeeded(&repository_id, class, now);
                continue;
            };
            if class == WatchRefreshClass::SemanticFollowup
                && task_registry
                    .read()
                    .expect("watch runtime task registry poisoned")
                    .has_active_task_for_repository(
                        RuntimeTaskKind::SemanticRefresh,
                        &repository.repository_id,
                    )
            {
                scheduler.mark_succeeded(&repository_id, class, now);
                continue;
            }
            info!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                refresh_class = %class.as_str(),
                debounce_ms = watch_config.debounce_ms,
                "built-in watch mode starting refresh"
            );
            let task_id = task_registry
                .write()
                .expect("watch runtime task registry poisoned")
                .start_task(
                    watch_task_kind_for_class(class),
                    repository.repository_id.clone(),
                    watch_task_phase_for_class(class),
                    Some(format!(
                        "watch root {} class {}",
                        repository.root.display(),
                        class.as_str()
                    )),
                );
            let completion_tx = command_tx.clone();
            let semantic_runtime = semantic_runtime.clone();
            let semantic_credentials = semantic_credentials.clone();
            let task_registry: Arc<RwLock<RuntimeTaskRegistry>> = Arc::clone(&task_registry);
            let validated_manifest_candidate_cache =
                Arc::clone(&validated_manifest_candidate_cache);
            tokio::task::spawn_blocking(move || {
                let result = match class {
                    WatchRefreshClass::ManifestFast => {
                        let mut lexical_only_runtime = semantic_runtime.clone();
                        lexical_only_runtime.enabled = false;
                        reindex_repository_with_runtime_config_and_dirty_paths(
                            &repository.repository_id,
                            &repository.root,
                            &repository.db_path,
                            ReindexMode::ChangedOnly,
                            &lexical_only_runtime,
                            &semantic_credentials,
                            &recent_paths,
                        )
                    }
                    WatchRefreshClass::SemanticFollowup => reindex_repository_with_runtime_config(
                        &repository.repository_id,
                        &repository.root,
                        &repository.db_path,
                        ReindexMode::Full,
                        &semantic_runtime,
                        &semantic_credentials,
                    ),
                }
                .map_err(|err| err.to_string());
                let detail = result.as_ref().err().cloned();
                let status = if result.is_ok() {
                    RuntimeTaskStatus::Succeeded
                } else {
                    RuntimeTaskStatus::Failed
                };
                if result.is_ok() && class == WatchRefreshClass::ManifestFast {
                    validated_manifest_candidate_cache
                        .write()
                        .expect("validated manifest candidate cache poisoned")
                        .invalidate_root(&repository.root);
                }
                task_registry
                    .write()
                    .expect("watch runtime task registry poisoned")
                    .finish_task(&task_id, status, detail);
                let _ = completion_tx.send(SupervisorCommand::ReindexCompleted {
                    repository_id: repository.repository_id.clone(),
                    class,
                    result,
                });
            });
        }
    }
}

fn watch_task_kind_for_class(class: WatchRefreshClass) -> RuntimeTaskKind {
    match class {
        WatchRefreshClass::ManifestFast => RuntimeTaskKind::ChangedReindex,
        WatchRefreshClass::SemanticFollowup => RuntimeTaskKind::SemanticRefresh,
    }
}

fn watch_task_phase_for_class(class: WatchRefreshClass) -> &'static str {
    match class {
        WatchRefreshClass::ManifestFast => "watch_manifest_fast",
        WatchRefreshClass::SemanticFollowup => "watch_semantic_followup",
    }
}

fn handle_notify_event(
    repositories: &Arc<RwLock<BTreeMap<String, WatchedRepository>>>,
    scheduler: &mut WatchSchedulerState,
    validated_manifest_candidate_cache: &Arc<RwLock<ValidatedManifestCandidateCache>>,
    repository_cache_invalidation_callback: Option<&RepositoryCacheInvalidationCallback>,
    event: Event,
    now: Instant,
    debounce: Duration,
) {
    if !event_kind_is_relevant(&event.kind) {
        return;
    }

    let mut invalidated_repository_ids = Vec::new();
    for path in event.paths {
        let repository = {
            let repositories_guard = repositories
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let repository_id = repository_id_for_path(
                repositories_guard.values().cloned().collect::<Vec<_>>(),
                &path,
            );
            repository_id.and_then(|repository_id| repositories_guard.get(&repository_id).cloned())
        };
        let Some(repository) = repository else {
            continue;
        };
        if should_ignore_watch_path(&repository, &path) {
            continue;
        }

        scheduler.record_path_change(&repository.repository_id, path.clone(), now, debounce);
        validated_manifest_candidate_cache
            .write()
            .expect("validated manifest candidate cache poisoned")
            .mark_dirty_root(&repository.root);
        if !invalidated_repository_ids.contains(&repository.repository_id) {
            if let Some(callback) = repository_cache_invalidation_callback {
                callback(&repository.repository_id);
            }
            invalidated_repository_ids.push(repository.repository_id.clone());
        }
        info!(
            repository_id = %repository.repository_id,
            path = %path.display(),
            "built-in watch mode accepted path change"
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_reindex_completed(
    repositories: &Arc<RwLock<BTreeMap<String, WatchedRepository>>>,
    scheduler: &mut WatchSchedulerState,
    repository_id: &str,
    class: WatchRefreshClass,
    result: Result<crate::indexer::ReindexSummary, String>,
    repository_cache_invalidation_callback: Option<&RepositoryCacheInvalidationCallback>,
    now: Instant,
    retry: Duration,
    semantic_runtime: &SemanticRuntimeConfig,
    semantic_credentials: &SemanticRuntimeCredentials,
) {
    let repository = repositories
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(repository_id)
        .cloned();
    let Some(repository) = repository else {
        return;
    };

    match result {
        Ok(summary) => {
            scheduler.mark_succeeded(repository_id, class, now);
            if let Some(callback) = repository_cache_invalidation_callback {
                callback(&repository.repository_id);
            }
            info!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                refresh_class = %class.as_str(),
                snapshot_id = %summary.snapshot_id,
                files_scanned = summary.files_scanned,
                files_changed = summary.files_changed,
                files_deleted = summary.files_deleted,
                duration_ms = summary.duration_ms,
                "built-in watch mode refresh succeeded"
            );
            if class == WatchRefreshClass::ManifestFast {
                queue_semantic_followup_if_needed(
                    &repository,
                    scheduler,
                    now,
                    semantic_runtime,
                    semantic_credentials,
                );
            }
        }
        Err(error) => {
            scheduler.mark_failed(repository_id, class, now, retry);
            warn!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                refresh_class = %class.as_str(),
                retry_ms = retry.as_millis(),
                error = %error,
                "built-in watch mode refresh failed; retry scheduled"
            );
        }
    }
}

fn queue_startup_refresh_if_needed(
    repository: &WatchedRepository,
    scheduler: &mut WatchSchedulerState,
    now: Instant,
    semantic_runtime: &SemanticRuntimeConfig,
    semantic_credentials: &SemanticRuntimeCredentials,
    debounce_ms: u64,
) {
    let Ok(startup_status) =
        startup_refresh_status(repository, semantic_runtime, semantic_credentials)
    else {
        return;
    };
    if !startup_status.should_refresh {
        info!(
            repository_id = %repository.repository_id,
            root = %repository.root.display(),
            snapshot_id = startup_status.snapshot_id.as_deref().unwrap_or("-"),
            "built-in watch mode found refreshable startup state already satisfied"
        );
        return;
    }

    let class = startup_status
        .refresh_class
        .unwrap_or(WatchRefreshClass::ManifestFast);
    scheduler.enqueue_initial_sync(&repository.repository_id, class, now);
    info!(
        repository_id = %repository.repository_id,
        root = %repository.root.display(),
        refresh_class = %class.as_str(),
        startup_reason = %startup_status.reason,
        snapshot_id = startup_status.snapshot_id.as_deref().unwrap_or("-"),
        debounce_ms,
        "built-in watch mode queued initial refresh"
    );
}

fn queue_semantic_followup_if_needed(
    repository: &WatchedRepository,
    scheduler: &mut WatchSchedulerState,
    now: Instant,
    semantic_runtime: &SemanticRuntimeConfig,
    semantic_credentials: &SemanticRuntimeCredentials,
) {
    let Ok(status) = startup_refresh_status(repository, semantic_runtime, semantic_credentials)
    else {
        return;
    };
    if status.refresh_class != Some(WatchRefreshClass::SemanticFollowup) {
        return;
    }

    scheduler.enqueue_semantic_followup(&repository.repository_id, now);
    info!(
        repository_id = %repository.repository_id,
        root = %repository.root.display(),
        startup_reason = %status.reason,
        snapshot_id = status.snapshot_id.as_deref().unwrap_or("-"),
        "built-in watch mode queued semantic follow-up after manifest refresh"
    );
}
