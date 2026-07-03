//! Watch supervisor: leases repository roots to MCP sessions, dispatches debounced index work,
//! and uses per-repository epochs to drop stale completions after a lease is released.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};
use tracing::{info, warn};

use crate::domain::{FriggError, FriggResult};
use crate::indexer::{
    IndexMode, IndexPlan, IndexSummary,
    index_repository_with_runtime_config_and_dirty_paths_and_plan_callback,
};
use crate::manifest_validation::ValidatedManifestCandidateCache;
use crate::mcp::types::{RuntimeTaskKind, RuntimeTaskStatus};
use crate::mcp::workspace_registry::AttachedWorkspace;
use crate::mcp::{RuntimeTaskGuard, RuntimeTaskRegistry};
use crate::settings::{
    FriggConfig, RuntimeTransportKind, SemanticRuntimeConfig, SemanticRuntimeCredentials,
};

use super::repository::{
    WatchedRepository, event_kind_is_relevant, repository_id_for_path,
    repository_relative_watch_path, should_ignore_watch_path, startup_refresh_status,
    watched_repository_for_workspace,
};
use super::scheduler::{ScheduledRefresh, WatchRefreshClass, WatchSchedulerState};

const WATCH_TICK_MS: u64 = 50;

/// Callback invoked when watch-driven index work invalidates MCP repository caches.
pub type RepositoryCacheInvalidationCallback = Arc<dyn Fn(&str) + Send + Sync + 'static>;

/// Callback invoked for user-facing watch progress events.
pub type WatchEventReporter = Arc<dyn Fn(WatchEvent) + Send + Sync + 'static>;

/// User-facing watch lifecycle and index-progress events emitted by the shared supervisor.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    RuntimeStarted {
        watch_mode: String,
        transport: RuntimeTransportKind,
        debounce_ms: u64,
        retry_ms: u64,
    },
    RuntimeDisabled {
        watch_mode: String,
        transport: RuntimeTransportKind,
    },
    LeaseAcquired {
        repository_id: String,
        lease_count: usize,
        root: PathBuf,
    },
    LeaseReleased {
        repository_id: String,
        remaining: usize,
        root: Option<PathBuf>,
    },
    PathAccepted {
        repository_id: String,
        path: PathBuf,
    },
    StartupFresh {
        repository_id: String,
        root: PathBuf,
        snapshot_id: Option<String>,
    },
    RefreshQueued {
        repository_id: String,
        root: PathBuf,
        refresh_class: &'static str,
        reason: String,
        snapshot_id: Option<String>,
        debounce_ms: Option<u64>,
    },
    RefreshStarting {
        repository_id: String,
        root: PathBuf,
        refresh_class: &'static str,
        debounce_ms: u64,
        sampled_paths: usize,
    },
    RefreshDeferred {
        repository_id: String,
        root: PathBuf,
        refresh_class: &'static str,
        reason: &'static str,
        retry_ms: u128,
    },
    IndexPlan {
        repository_id: String,
        refresh_class: &'static str,
        plan: IndexPlan,
    },
    RefreshSucceeded {
        repository_id: String,
        root: PathBuf,
        refresh_class: &'static str,
        summary: IndexSummary,
    },
    RefreshFailed {
        repository_id: String,
        root: PathBuf,
        refresh_class: &'static str,
        retry_ms: u128,
        error: String,
    },
    RefreshBlocked {
        repository_id: String,
        root: PathBuf,
        refresh_class: &'static str,
        reason: &'static str,
        error: String,
    },
    StaleCompletionIgnored {
        repository_id: String,
        refresh_class: &'static str,
        completion_epoch: u64,
        current_epoch: u64,
    },
    NotifyDropped {
        error: String,
    },
}

fn report_watch_event(reporter: &Option<WatchEventReporter>, event: WatchEvent) {
    if let Some(reporter) = reporter {
        reporter(event);
    }
}

enum SupervisorCommand {
    Event(Event),
    NotifyDropped {
        error: String,
    },
    LeaseAcquired {
        repository: WatchedRepository,
    },
    LeaseReleased {
        repository_id: String,
    },
    IndexCompleted {
        repository_id: String,
        epoch: u64,
        class: WatchRefreshClass,
        result: Result<crate::indexer::IndexSummary, String>,
    },
}

#[derive(Debug, Default)]
struct RepositoryEpochs {
    epochs: BTreeMap<String, u64>,
}

impl RepositoryEpochs {
    fn ensure(&mut self, repository_id: &str) {
        self.epochs.entry(repository_id.to_owned()).or_insert(0);
    }

    // Epoch bump invalidates in-flight index completions after lease release.
    fn bump(&mut self, repository_id: &str) {
        self.epochs
            .entry(repository_id.to_owned())
            .and_modify(|epoch| *epoch = epoch.wrapping_add(1))
            .or_insert(0);
    }

    fn current(&self, repository_id: &str) -> u64 {
        self.epochs.get(repository_id).copied().unwrap_or(0)
    }
}

/// Lease snapshot for a repository root watched by the shared supervisor.
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
    reporter: Option<WatchEventReporter>,
}

impl WatchRuntime {
    /// Registers a watch lease for one attached workspace and queues startup refresh on first holder.
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
            report_watch_event(
                &self.reporter,
                WatchEvent::LeaseAcquired {
                    repository_id: repository.repository_id.clone(),
                    lease_count,
                    root: repository.root.clone(),
                },
            );
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
        let event = WatchEvent::LeaseAcquired {
            repository_id: repository.repository_id.clone(),
            lease_count,
            root: repository.root.clone(),
        };
        let _ = self
            .command_tx
            .send(SupervisorCommand::LeaseAcquired { repository });
        report_watch_event(&self.reporter, event);

        Ok(lease_count)
    }

    /// Releases one watch lease and unregisters the watcher when the last holder drops.
    pub(crate) fn release_lease(&self, repository_id: &str) -> usize {
        let root = self
            .repositories
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(repository_id)
            .map(|repository| repository.root.clone());
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
            report_watch_event(
                &self.reporter,
                WatchEvent::LeaseReleased {
                    repository_id: repository_id.to_owned(),
                    remaining,
                    root,
                },
            );
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
        report_watch_event(
            &self.reporter,
            WatchEvent::LeaseReleased {
                repository_id: repository_id.to_owned(),
                remaining,
                root,
            },
        );

        remaining
    }

    /// Returns whether a repository root currently has active watch leases.
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

    #[cfg(test)]
    pub(crate) fn inject_test_notify_dropped(&self, error: impl Into<String>) {
        let _ = self.command_tx.send(SupervisorCommand::NotifyDropped {
            error: error.into(),
        });
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
    maybe_start_watch_runtime_with_reporter(
        config,
        transport,
        task_registry,
        validated_manifest_candidate_cache,
        repository_cache_invalidation_callback,
        None,
    )
}

/// Starts the shared watch supervisor with an optional user-facing event reporter.
pub fn maybe_start_watch_runtime_with_reporter(
    config: &FriggConfig,
    transport: RuntimeTransportKind,
    task_registry: Arc<RwLock<RuntimeTaskRegistry>>,
    validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    repository_cache_invalidation_callback: Option<RepositoryCacheInvalidationCallback>,
    reporter: Option<WatchEventReporter>,
) -> FriggResult<Option<WatchRuntime>> {
    if !config.watch.enabled_for_transport(transport) {
        info!(
            watch_mode = %config.watch.mode.as_str(),
            transport = ?transport,
            "built-in watch mode disabled for resolved transport"
        );
        report_watch_event(
            &reporter,
            WatchEvent::RuntimeDisabled {
                watch_mode: config.watch.mode.as_str().to_owned(),
                transport,
            },
        );
        return Ok(None);
    }

    let repositories = Arc::new(RwLock::new(BTreeMap::new()));
    let lease_counts = Arc::new(RwLock::new(BTreeMap::new()));
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let callback_tx = command_tx.clone();
    let watcher = notify::recommended_watcher(move |result: notify::Result<Event>| match result {
        Ok(event) => {
            let _ = callback_tx.send(SupervisorCommand::Event(event));
        }
        Err(error) => {
            let _ = callback_tx.send(SupervisorCommand::NotifyDropped {
                error: error.to_string(),
            });
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
        reporter.clone(),
        command_rx,
        command_tx.clone(),
    ));
    report_watch_event(
        &reporter,
        WatchEvent::RuntimeStarted {
            watch_mode: config.watch.mode.as_str().to_owned(),
            transport,
            debounce_ms: config.watch.debounce_ms,
            retry_ms: config.watch.retry_ms,
        },
    );

    Ok(Some(WatchRuntime {
        watcher: Mutex::new(watcher),
        repositories,
        lease_counts,
        supervisor_handle,
        command_tx,
        reporter,
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
    reporter: Option<WatchEventReporter>,
    mut command_rx: mpsc::UnboundedReceiver<SupervisorCommand>,
    command_tx: mpsc::UnboundedSender<SupervisorCommand>,
) {
    let debounce = Duration::from_millis(watch_config.debounce_ms);
    let retry = Duration::from_millis(watch_config.retry_ms);
    let mut scheduler = WatchSchedulerState::new(0);
    let mut repository_epochs = RepositoryEpochs::default();
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
                        &reporter,
                    ),
                    SupervisorCommand::NotifyDropped { error } => handle_notify_dropped(
                        &repositories,
                        &mut scheduler,
                        &validated_manifest_candidate_cache,
                        repository_cache_invalidation_callback.as_ref(),
                        error,
                        now,
                        debounce,
                        &reporter,
                    ),
                    SupervisorCommand::LeaseAcquired { repository } => {
                        repository_epochs.ensure(&repository.repository_id);
                        scheduler.add_repository(&repository.repository_id);
                        queue_startup_refresh_if_needed(
                            &repository,
                            &mut scheduler,
                            now,
                            &semantic_runtime,
                            &semantic_credentials,
                            watch_config.debounce_ms,
                            &reporter,
                        );
                    }
                    SupervisorCommand::LeaseReleased { repository_id } => {
                        // Lease released: advance epoch before dropping scheduler state.
                        repository_epochs.bump(&repository_id);
                        scheduler.remove_repository(&repository_id);
                    }
                    SupervisorCommand::IndexCompleted {
                        repository_id,
                        epoch,
                        class,
                        result,
                    } => {
                        let current_epoch = repository_epochs.current(&repository_id);
                        // Epoch mismatch: ignore stale index side effects from a prior lease.
                        if epoch != current_epoch {
                            warn!(
                                repository_id = %repository_id,
                                refresh_class = %class.as_str(),
                                completion_epoch = epoch,
                                current_epoch,
                                "built-in watch mode ignoring stale index completion"
                            );
                            report_watch_event(
                                &reporter,
                                WatchEvent::StaleCompletionIgnored {
                                    repository_id: repository_id.clone(),
                                    refresh_class: class.as_str(),
                                    completion_epoch: epoch,
                                    current_epoch,
                                },
                            );
                            invalidate_caches_for_stale_completion(
                                &repository_id,
                                &result,
                                repository_cache_invalidation_callback.as_ref(),
                            );
                        } else {
                            handle_index_completed(
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
                                &reporter,
                            );
                        }
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
            let repository = repositories
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&repository_id)
                .cloned();
            let Some(repository) = repository else {
                continue;
            };
            let stable_repository_id =
                crate::domain::model::stable_repository_id_for_root(&repository.root).0;
            let refresh_task_active = active_refresh_task_running_for_repository(
                &task_registry
                    .read()
                    .expect("watch runtime task registry poisoned"),
                class,
                &repository.repository_id,
                &stable_repository_id,
            );
            if refresh_task_active {
                scheduler.mark_failed(&repository_id, class, now, retry);
                report_watch_event(
                    &reporter,
                    WatchEvent::RefreshDeferred {
                        repository_id: repository.repository_id.clone(),
                        root: repository.root.clone(),
                        refresh_class: class.as_str(),
                        reason: "active_task",
                        retry_ms: retry.as_millis(),
                    },
                );
                continue;
            }
            let recent_paths = scheduler.mark_started(&repository_id, class);
            // Capture dispatch epoch so completion handlers can detect lease churn.
            let dispatch_epoch = repository_epochs.current(&repository.repository_id);
            info!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                refresh_class = %class.as_str(),
                debounce_ms = watch_config.debounce_ms,
                "built-in watch mode starting refresh"
            );
            report_watch_event(
                &reporter,
                WatchEvent::RefreshStarting {
                    repository_id: repository.repository_id.clone(),
                    root: repository.root.clone(),
                    refresh_class: class.as_str(),
                    debounce_ms: watch_config.debounce_ms,
                    sampled_paths: recent_paths.len(),
                },
            );
            let mut task_guard = RuntimeTaskGuard::start(
                Arc::clone(&task_registry),
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
            let validated_manifest_candidate_cache =
                Arc::clone(&validated_manifest_candidate_cache);
            let repository_cache_invalidation_callback =
                repository_cache_invalidation_callback.clone();
            let reporter = reporter.clone();
            // Side effect: blocking index runs off the supervisor tick loop.
            tokio::task::spawn_blocking(move || {
                let result = match class {
                    WatchRefreshClass::ManifestFast => {
                        let mut lexical_only_runtime = semantic_runtime.clone();
                        lexical_only_runtime.enabled = false;
                        let plan_reporter = reporter.clone();
                        let plan_repository_id = repository.repository_id.clone();
                        index_repository_with_runtime_config_and_dirty_paths_and_plan_callback(
                            &repository.repository_id,
                            &repository.root,
                            &repository.db_path,
                            IndexMode::ChangedOnly,
                            &lexical_only_runtime,
                            &semantic_credentials,
                            &recent_paths,
                            move |plan| {
                                report_watch_event(
                                    &plan_reporter,
                                    WatchEvent::IndexPlan {
                                        repository_id: plan_repository_id.clone(),
                                        refresh_class: class.as_str(),
                                        plan: plan.clone(),
                                    },
                                );
                                Ok(())
                            },
                        )
                    }
                    WatchRefreshClass::SemanticFollowup => {
                        let plan_reporter = reporter.clone();
                        let plan_repository_id = repository.repository_id.clone();
                        let index_mode = if recent_paths.is_empty() {
                            IndexMode::Full
                        } else {
                            IndexMode::ChangedOnly
                        };
                        index_repository_with_runtime_config_and_dirty_paths_and_plan_callback(
                            &repository.repository_id,
                            &repository.root,
                            &repository.db_path,
                            index_mode,
                            &semantic_runtime,
                            &semantic_credentials,
                            &recent_paths,
                            move |plan| {
                                report_watch_event(
                                    &plan_reporter,
                                    WatchEvent::IndexPlan {
                                        repository_id: plan_repository_id.clone(),
                                        refresh_class: class.as_str(),
                                        plan: plan.clone(),
                                    },
                                );
                                Ok(())
                            },
                        )
                    }
                }
                .map_err(|err| err.to_string());
                let detail = match &result {
                    Ok(summary) => Some(format!(
                        "refresh_class={} snapshot={} files_changed={} files_deleted={} changed_paths={} deleted_paths={} duration_ms={}",
                        class.as_str(),
                        summary.snapshot_id,
                        summary.files_changed,
                        summary.files_deleted,
                        summary.changed_paths.len(),
                        summary.deleted_paths.len(),
                        summary.duration_ms
                    )),
                    Err(err) => Some(format!("refresh_class={} error={}", class.as_str(), err)),
                };
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
                if let Some(callback) = repository_cache_invalidation_callback.as_ref() {
                    callback(&repository.repository_id);
                }
                task_guard.finish(status, detail);
                let _ = completion_tx.send(SupervisorCommand::IndexCompleted {
                    repository_id: repository.repository_id.clone(),
                    epoch: dispatch_epoch,
                    class,
                    result,
                });
            });
        }
    }
}

fn watch_task_kind_for_class(class: WatchRefreshClass) -> RuntimeTaskKind {
    match class {
        WatchRefreshClass::ManifestFast => RuntimeTaskKind::ChangedIndex,
        WatchRefreshClass::SemanticFollowup => RuntimeTaskKind::SemanticRefresh,
    }
}

fn conflicting_task_kinds_for_class(class: WatchRefreshClass) -> &'static [RuntimeTaskKind] {
    match class {
        WatchRefreshClass::ManifestFast => &[
            RuntimeTaskKind::ChangedIndex,
            RuntimeTaskKind::WorkspaceIndex,
            RuntimeTaskKind::WorkspacePrepare,
        ],
        WatchRefreshClass::SemanticFollowup => &[
            RuntimeTaskKind::SemanticRefresh,
            RuntimeTaskKind::WorkspaceIndex,
            RuntimeTaskKind::WorkspacePrepare,
        ],
    }
}

fn active_refresh_task_running_for_repository(
    registry: &RuntimeTaskRegistry,
    class: WatchRefreshClass,
    repository_id: &str,
    stable_repository_id: &str,
) -> bool {
    let repository_ids = if repository_id == stable_repository_id {
        vec![repository_id]
    } else {
        vec![repository_id, stable_repository_id]
    };
    conflicting_task_kinds_for_class(class)
        .iter()
        .any(|kind| registry.has_active_task_for_any_repository(*kind, &repository_ids))
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
    reporter: &Option<WatchEventReporter>,
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
        let relative_path =
            repository_relative_watch_path(&repository, &path).unwrap_or_else(|| path.clone());
        report_watch_event(
            reporter,
            WatchEvent::PathAccepted {
                repository_id: repository.repository_id,
                path: relative_path,
            },
        );
    }
}

fn handle_notify_dropped(
    repositories: &Arc<RwLock<BTreeMap<String, WatchedRepository>>>,
    scheduler: &mut WatchSchedulerState,
    validated_manifest_candidate_cache: &Arc<RwLock<ValidatedManifestCandidateCache>>,
    repository_cache_invalidation_callback: Option<&RepositoryCacheInvalidationCallback>,
    error: String,
    now: Instant,
    debounce: Duration,
    reporter: &Option<WatchEventReporter>,
) {
    warn!(error = %error, "built-in watch mode dropped notify event");
    report_watch_event(
        reporter,
        WatchEvent::NotifyDropped {
            error: error.clone(),
        },
    );

    let repositories = repositories
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for repository in repositories {
        scheduler.record_path_change(
            &repository.repository_id,
            repository.root.clone(),
            now,
            debounce,
        );
        validated_manifest_candidate_cache
            .write()
            .expect("validated manifest candidate cache poisoned")
            .mark_dirty_root(&repository.root);
        if let Some(callback) = repository_cache_invalidation_callback {
            callback(&repository.repository_id);
        }
        info!(
            repository_id = %repository.repository_id,
            root = %repository.root.display(),
            "built-in watch mode conservatively invalidated repository after dropped notify event"
        );
    }
}

fn invalidate_caches_for_stale_completion(
    repository_id: &str,
    _result: &Result<crate::indexer::IndexSummary, String>,
    repository_cache_invalidation_callback: Option<&RepositoryCacheInvalidationCallback>,
) -> bool {
    if let Some(callback) = repository_cache_invalidation_callback {
        callback(repository_id);
        return true;
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn handle_index_completed(
    repositories: &Arc<RwLock<BTreeMap<String, WatchedRepository>>>,
    scheduler: &mut WatchSchedulerState,
    repository_id: &str,
    class: WatchRefreshClass,
    result: Result<crate::indexer::IndexSummary, String>,
    repository_cache_invalidation_callback: Option<&RepositoryCacheInvalidationCallback>,
    now: Instant,
    retry: Duration,
    semantic_runtime: &SemanticRuntimeConfig,
    semantic_credentials: &SemanticRuntimeCredentials,
    reporter: &Option<WatchEventReporter>,
) {
    let repository = repositories
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(repository_id)
        .cloned();
    let Some(repository) = repository else {
        return;
    };
    if let Some(callback) = repository_cache_invalidation_callback {
        callback(&repository.repository_id);
    }

    match result {
        Ok(summary) => {
            scheduler.mark_succeeded(repository_id, class, now);
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
            report_watch_event(
                reporter,
                WatchEvent::RefreshSucceeded {
                    repository_id: repository.repository_id.clone(),
                    root: repository.root.clone(),
                    refresh_class: class.as_str(),
                    summary: summary.clone(),
                },
            );
            if class == WatchRefreshClass::ManifestFast {
                queue_semantic_followup_if_needed(
                    &repository,
                    scheduler,
                    now,
                    semantic_runtime,
                    semantic_credentials,
                    reporter,
                );
            }
        }
        Err(error) => {
            if is_non_retryable_watch_refresh_error(&error) {
                scheduler.mark_blocked(repository_id, class);
                if reporter.is_none() {
                    warn!(
                        repository_id = %repository.repository_id,
                        root = %repository.root.display(),
                        refresh_class = %class.as_str(),
                        error = %error,
                        "built-in watch mode refresh blocked; manual repair required"
                    );
                }
                report_watch_event(
                    reporter,
                    WatchEvent::RefreshBlocked {
                        repository_id: repository.repository_id,
                        root: repository.root,
                        refresh_class: class.as_str(),
                        reason: "storage_schema_incompatible",
                        error,
                    },
                );
            } else {
                scheduler.mark_failed(repository_id, class, now, retry);
                if reporter.is_none() {
                    warn!(
                        repository_id = %repository.repository_id,
                        root = %repository.root.display(),
                        refresh_class = %class.as_str(),
                        retry_ms = retry.as_millis(),
                        error = %error,
                        "built-in watch mode refresh failed; retry scheduled"
                    );
                }
                report_watch_event(
                    reporter,
                    WatchEvent::RefreshFailed {
                        repository_id: repository.repository_id,
                        root: repository.root,
                        refresh_class: class.as_str(),
                        retry_ms: retry.as_millis(),
                        error,
                    },
                );
            }
        }
    }
}

fn is_non_retryable_watch_refresh_error(error: &str) -> bool {
    error.contains("storage schema is incompatible")
        && error.contains("automatic schema migrations are disabled")
}

fn queue_startup_refresh_if_needed(
    repository: &WatchedRepository,
    scheduler: &mut WatchSchedulerState,
    now: Instant,
    semantic_runtime: &SemanticRuntimeConfig,
    semantic_credentials: &SemanticRuntimeCredentials,
    debounce_ms: u64,
    reporter: &Option<WatchEventReporter>,
) {
    let startup_status = match startup_refresh_status(
        repository,
        semantic_runtime,
        semantic_credentials,
    ) {
        Ok(status) => status,
        Err(error) => {
            warn!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                error = %error,
                "built-in watch mode failed to evaluate startup freshness; queueing manifest-fast refresh"
            );
            scheduler.enqueue_initial_sync(
                &repository.repository_id,
                WatchRefreshClass::ManifestFast,
                now,
            );
            report_watch_event(
                reporter,
                WatchEvent::RefreshQueued {
                    repository_id: repository.repository_id.clone(),
                    root: repository.root.clone(),
                    refresh_class: WatchRefreshClass::ManifestFast.as_str(),
                    reason: "startup_freshness_error".to_owned(),
                    snapshot_id: None,
                    debounce_ms: Some(debounce_ms),
                },
            );
            return;
        }
    };
    if !startup_status.should_refresh {
        info!(
            repository_id = %repository.repository_id,
            root = %repository.root.display(),
            snapshot_id = startup_status.snapshot_id.as_deref().unwrap_or("-"),
            "built-in watch mode found refreshable startup state already satisfied"
        );
        report_watch_event(
            reporter,
            WatchEvent::StartupFresh {
                repository_id: repository.repository_id.clone(),
                root: repository.root.clone(),
                snapshot_id: startup_status.snapshot_id,
            },
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
    report_watch_event(
        reporter,
        WatchEvent::RefreshQueued {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            refresh_class: class.as_str(),
            reason: startup_status.reason.to_owned(),
            snapshot_id: startup_status.snapshot_id,
            debounce_ms: Some(debounce_ms),
        },
    );
}

fn queue_semantic_followup_if_needed(
    repository: &WatchedRepository,
    scheduler: &mut WatchSchedulerState,
    now: Instant,
    semantic_runtime: &SemanticRuntimeConfig,
    semantic_credentials: &SemanticRuntimeCredentials,
    reporter: &Option<WatchEventReporter>,
) {
    let status = match startup_refresh_status(repository, semantic_runtime, semantic_credentials) {
        Ok(status) => status,
        Err(error) => {
            warn!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                error = %error,
                "built-in watch mode failed to evaluate semantic follow-up freshness; skipping"
            );
            return;
        }
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
    report_watch_event(
        reporter,
        WatchEvent::RefreshQueued {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            refresh_class: WatchRefreshClass::SemanticFollowup.as_str(),
            reason: status.reason.to_owned(),
            snapshot_id: status.snapshot_id,
            debounce_ms: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::Instant;

    use super::*;

    #[test]
    fn repository_epochs_invalidate_stale_completion_across_detach_reattach() {
        let mut epochs = RepositoryEpochs::default();

        epochs.ensure("repo-001");
        let dispatch_epoch = epochs.current("repo-001");
        assert_eq!(dispatch_epoch, 0);

        epochs.bump("repo-001");

        epochs.ensure("repo-001");
        assert_eq!(epochs.current("repo-001"), 1);

        assert_ne!(dispatch_epoch, epochs.current("repo-001"));

        let fresh_dispatch_epoch = epochs.current("repo-001");
        assert_eq!(fresh_dispatch_epoch, epochs.current("repo-001"));
    }

    fn sample_index_summary() -> crate::indexer::IndexSummary {
        crate::indexer::IndexSummary {
            repository_id: "repo-001".to_owned(),
            snapshot_id: "snap-1".to_owned(),
            previous_snapshot_id: None,
            snapshot_plan: "persist_new".to_owned(),
            files_scanned: 1,
            files_changed: 1,
            files_deleted: 0,
            changed_paths: vec!["src/lib.rs".to_owned()],
            deleted_paths: Vec::new(),
            semantic_refresh_mode: crate::indexer::SemanticRefreshMode::Disabled,
            semantic_provider: None,
            semantic_model: None,
            semantic_records: 0,
            diagnostics: Default::default(),
            duration_ms: 1,
        }
    }

    #[test]
    fn stale_successful_completion_invalidates_repository_caches() {
        let invalidated: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = Arc::clone(&invalidated);
        let callback: RepositoryCacheInvalidationCallback = Arc::new(move |repository_id: &str| {
            recorded
                .lock()
                .expect("invalidation record poisoned")
                .push(repository_id.to_owned());
        });

        let invoked = invalidate_caches_for_stale_completion(
            "repo-001",
            &Ok(sample_index_summary()),
            Some(&callback),
        );

        assert!(invoked, "stale success must invalidate repository caches");
        assert_eq!(
            *invalidated.lock().expect("invalidation record poisoned"),
            vec!["repo-001".to_owned()],
        );
    }

    #[test]
    fn stale_failed_completion_invalidates_repository_caches() {
        let invalidated: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = Arc::clone(&invalidated);
        let callback: RepositoryCacheInvalidationCallback = Arc::new(move |repository_id: &str| {
            recorded
                .lock()
                .expect("invalidation record poisoned")
                .push(repository_id.to_owned());
        });

        let invoked = invalidate_caches_for_stale_completion(
            "repo-001",
            &Err("boom".to_owned()),
            Some(&callback),
        );

        assert!(
            invoked,
            "stale failure must conservatively invalidate caches"
        );
        assert_eq!(
            *invalidated.lock().expect("invalidation record poisoned"),
            vec!["repo-001".to_owned()],
        );
    }

    #[test]
    fn repository_epochs_match_for_undisturbed_lifecycle() {
        let mut epochs = RepositoryEpochs::default();
        epochs.ensure("repo-001");
        let dispatch_epoch = epochs.current("repo-001");
        assert_eq!(dispatch_epoch, epochs.current("repo-001"));
    }

    #[test]
    fn repository_epochs_default_to_zero_for_unknown_repository() {
        let epochs = RepositoryEpochs::default();
        assert_eq!(epochs.current("never-seen"), 0);
    }

    #[test]
    fn active_changed_index_defers_manifest_fast_without_draining_scheduler() {
        let now = Instant::now();
        let retry = Duration::from_millis(250);
        let mut scheduler = WatchSchedulerState::new(0);
        scheduler.add_repository("repo-001");
        scheduler.enqueue_initial_sync("repo-001", WatchRefreshClass::ManifestFast, now);

        let mut registry = RuntimeTaskRegistry::new();
        let task_id = registry.start_task(
            RuntimeTaskKind::ChangedIndex,
            "repo-001".to_owned(),
            "watch_manifest_fast",
            None,
        );

        let ready = scheduler
            .next_ready_refresh(now)
            .expect("manifest refresh should be ready");
        assert_eq!(ready.class, WatchRefreshClass::ManifestFast);
        assert!(active_refresh_task_running_for_repository(
            &registry,
            ready.class,
            &ready.repository_id,
            "stable-repo-001",
        ));

        scheduler.mark_failed(&ready.repository_id, ready.class, now, retry);
        assert!(scheduler.repository_pending("repo-001", WatchRefreshClass::ManifestFast));
        assert!(
            scheduler.next_ready_refresh(now).is_none(),
            "pending manifest refresh should back off while old task is active"
        );

        registry.finish_task(&task_id, RuntimeTaskStatus::Succeeded, None);
        assert!(!active_refresh_task_running_for_repository(
            &registry,
            ready.class,
            &ready.repository_id,
            "stable-repo-001",
        ));
        assert!(
            scheduler.next_ready_refresh(now + retry).is_some(),
            "manifest refresh should become ready after the active task finishes and retry elapses"
        );
    }

    #[test]
    fn refresh_task_dedup_uses_stable_and_runtime_aliases() {
        let mut registry = RuntimeTaskRegistry::new();
        let _task_id = registry.start_task(
            RuntimeTaskKind::SemanticRefresh,
            "stable-repo".to_owned(),
            "semantic_attach_refresh",
            None,
        );

        assert!(active_refresh_task_running_for_repository(
            &registry,
            WatchRefreshClass::SemanticFollowup,
            "repo-001",
            "stable-repo",
        ));
        assert!(
            !active_refresh_task_running_for_repository(
                &registry,
                WatchRefreshClass::ManifestFast,
                "repo-001",
                "stable-repo",
            ),
            "semantic refresh should not block manifest-fast changed index"
        );
    }

    #[test]
    fn manifest_fast_defers_to_active_mcp_workspace_index() {
        let mut registry = RuntimeTaskRegistry::new();
        let _task_id = registry.start_task(
            RuntimeTaskKind::WorkspaceIndex,
            "stable-repo".to_owned(),
            "workspace_index",
            None,
        );

        assert!(
            active_refresh_task_running_for_repository(
                &registry,
                WatchRefreshClass::ManifestFast,
                "repo-001",
                "stable-repo",
            ),
            "manifest-fast must defer to an active MCP workspace_index on the same db"
        );
        assert!(
            active_refresh_task_running_for_repository(
                &registry,
                WatchRefreshClass::SemanticFollowup,
                "repo-001",
                "stable-repo",
            ),
            "semantic followup must also defer to an active MCP workspace_index"
        );
    }

    #[test]
    fn startup_refresh_eval_failure_queues_manifest_fast_instead_of_silent_drop() {
        use super::super::repository::watched_repository_for_root;
        use crate::settings::SemanticRuntimeProvider;

        let now = Instant::now();
        let repository = watched_repository_for_root(
            "repo-001".to_owned(),
            std::path::PathBuf::from("/nonexistent/frigg-watch-startup-eval"),
            std::path::PathBuf::from("/nonexistent/frigg-watch-startup-eval/frigg.db"),
        )
        .expect("watched repository should build for a missing root");

        let semantic_runtime = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: Some("text-embedding-3-small".to_owned()),
            strict_mode: false,
        };
        let credentials = SemanticRuntimeCredentials::default();

        let mut scheduler = WatchSchedulerState::new(0);
        scheduler.add_repository("repo-001");
        queue_startup_refresh_if_needed(
            &repository,
            &mut scheduler,
            now,
            &semantic_runtime,
            &credentials,
            0,
            &None,
        );

        assert!(
            scheduler.repository_pending("repo-001", WatchRefreshClass::ManifestFast),
            "a startup freshness evaluation failure must still queue a manifest-fast refresh"
        );
    }

    #[test]
    fn incompatible_storage_schema_errors_do_not_retry_forever() {
        let error = "internal error: storage schema is incompatible (found 10, expected 11); automatic schema migrations are disabled for Frigg's regenerable local index";

        assert!(is_non_retryable_watch_refresh_error(error));
        assert!(!is_non_retryable_watch_refresh_error(
            "transient filesystem read failed"
        ));
    }

    #[test]
    fn manifest_fast_defers_to_active_mcp_workspace_prepare() {
        let mut registry = RuntimeTaskRegistry::new();
        let _task_id = registry.start_task(
            RuntimeTaskKind::WorkspacePrepare,
            "repo-001".to_owned(),
            "workspace_prepare",
            None,
        );

        assert!(
            active_refresh_task_running_for_repository(
                &registry,
                WatchRefreshClass::ManifestFast,
                "repo-001",
                "repo-001",
            ),
            "manifest-fast must defer to an active MCP workspace_prepare on the same db"
        );
    }
}
