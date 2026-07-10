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
    IndexMode, IndexPlan, IndexProgressEvent, IndexSummary,
    index_repository_with_runtime_config_and_dirty_paths_and_progress_and_commit_callback,
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
const WATCH_INDEX_WORKER_PANIC_MESSAGE: &str = "watch index worker panicked";
const WATCH_STALE_DISPATCH_EPOCH_MESSAGE: &str =
    "watch index dispatch epoch stale after lease release";

/// Callback invoked when watch-driven index work invalidates MCP repository caches.
///
/// Arguments: `(repository_id, dirty_paths)`.
/// - `Some(paths)` — known dirty set (repository-relative when available). Session result
///   handles for those paths are dropped; **empty `Some` skips handle invalidation** (known
///   noop / nothing changed) while other repository caches still invalidate.
/// - `None` — unknown dirty set → whole-repository session handle wipe (notify-dropped,
///   failed refresh).
pub type RepositoryCacheInvalidationCallback =
    Arc<dyn Fn(&str, Option<&[String]>) + Send + Sync + 'static>;

fn startup_refresh_error_blocks_retry(error: &FriggError) -> bool {
    matches!(error, FriggError::InvalidInput(_))
}

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
    IndexProgress {
        repository_id: String,
        refresh_class: &'static str,
        progress: IndexProgressEvent,
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
        result: Result<crate::indexer::IndexSummary, WatchRefreshFailure>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchRefreshFailureKind {
    Retryable,
    StorageSchemaIncompatible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WatchRefreshFailure {
    message: String,
    kind: WatchRefreshFailureKind,
}

impl WatchRefreshFailure {
    fn from_error(error: FriggError) -> Self {
        let kind = if matches!(&error, FriggError::StorageSchemaIncompatible { .. }) {
            WatchRefreshFailureKind::StorageSchemaIncompatible
        } else {
            WatchRefreshFailureKind::Retryable
        };
        Self {
            message: error.to_string(),
            kind,
        }
    }

    fn blocks_retry(&self) -> bool {
        self.kind == WatchRefreshFailureKind::StorageSchemaIncompatible
    }
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

fn watch_dispatch_epoch_is_stale(
    epochs: &RepositoryEpochs,
    repository_id: &str,
    dispatch_epoch: u64,
) -> bool {
    epochs.current(repository_id) != dispatch_epoch
}

fn abort_watch_index_plan_if_dispatch_epoch_stale(
    epochs: &RepositoryEpochs,
    repository_id: &str,
    dispatch_epoch: u64,
) -> FriggResult<()> {
    if watch_dispatch_epoch_is_stale(epochs, repository_id, dispatch_epoch) {
        return Err(FriggError::Internal(
            WATCH_STALE_DISPATCH_EPOCH_MESSAGE.to_owned(),
        ));
    }
    Ok(())
}

fn should_invalidate_watch_worker_cache(
    epochs: &RepositoryEpochs,
    repository_id: &str,
    dispatch_epoch: u64,
) -> bool {
    !watch_dispatch_epoch_is_stale(epochs, repository_id, dispatch_epoch)
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
            if let Some(count) = lease_counts.get_mut(&repository.repository_id)
                && *count > 0
            {
                *count = count.saturating_add(1);
                *count
            } else {
                self.watcher
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .watch(&repository.root, RecursiveMode::Recursive)
                    .map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to register watcher for root {}: {err}",
                            repository.root.display()
                        ))
                    })?;
                lease_counts.insert(repository.repository_id.clone(), 1);
                1
            }
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
    config.watch.validate()?;

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
    let mut scheduler = WatchSchedulerState::with_concurrency_limits(
        0,
        watch_config.manifest_fast_concurrency,
        watch_config.semantic_followup_concurrency,
    );
    let repository_epochs = Arc::new(Mutex::new(RepositoryEpochs::default()));
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
                        repository_epochs
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .ensure(&repository.repository_id);
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
                        repository_epochs
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .bump(&repository_id);
                        scheduler.remove_repository(&repository_id);
                    }
                    SupervisorCommand::IndexCompleted {
                        repository_id,
                        epoch,
                        class,
                        result,
                    } => {
                        let current_epoch = repository_epochs
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .current(&repository_id);
                        // Epoch mismatch: ignore stale scheduler follow-up from a prior lease.
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
                                &validated_manifest_candidate_cache,
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
            let repository_ids = watch_runtime_task_repository_aliases(
                &repository.repository_id,
                &stable_repository_id,
            );
            let repository_id_refs = repository_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            let task_guard = RuntimeTaskGuard::try_start_if_no_active_for_any_repository(
                Arc::clone(&task_registry),
                conflicting_task_kinds_for_class(class),
                &repository_id_refs,
                watch_task_kind_for_class(class),
                repository.repository_id.clone(),
                watch_task_phase_for_class(class),
                Some(format!(
                    "watch root {} class {}",
                    repository.root.display(),
                    class.as_str()
                )),
            );
            let mut task_guard = match task_guard {
                Ok(task_guard) => task_guard,
                Err(_) => {
                    let retry_delay = scheduler
                        .mark_failed(&repository_id, class, now, retry)
                        .unwrap_or(retry);
                    report_watch_event(
                        &reporter,
                        WatchEvent::RefreshDeferred {
                            repository_id: repository.repository_id.clone(),
                            root: repository.root.clone(),
                            refresh_class: class.as_str(),
                            reason: "active_task",
                            retry_ms: retry_delay.as_millis(),
                        },
                    );
                    continue;
                }
            };
            let recent_paths = scheduler.mark_started(&repository_id, class);
            // Capture dispatch epoch so worker side effects and completions can detect lease churn.
            let dispatch_epoch = repository_epochs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .current(&repository.repository_id);
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
            let completion_tx = command_tx.clone();
            let semantic_runtime = semantic_runtime.clone();
            let semantic_credentials = semantic_credentials.clone();
            let repository_cache_invalidation_callback =
                repository_cache_invalidation_callback.clone();
            let reporter = reporter.clone();
            let worker_epochs = Arc::clone(&repository_epochs);
            // Side effect: blocking index runs off the supervisor tick loop.
            tokio::task::spawn_blocking(move || {
                let invalidation_epochs = Arc::clone(&worker_epochs);
                let result = run_caught_watch_index_work(|| match class {
                    WatchRefreshClass::ManifestFast => {
                        let mut lexical_only_runtime = semantic_runtime.clone();
                        lexical_only_runtime.enabled = false;
                        let plan_reporter = reporter.clone();
                        let progress_reporter = reporter.clone();
                        let plan_repository_id = repository.repository_id.clone();
                        let commit_repository_id = repository.repository_id.clone();
                        let progress_repository_id = repository.repository_id.clone();
                        let plan_epochs = Arc::clone(&worker_epochs);
                        let commit_epochs = Arc::clone(&worker_epochs);
                        index_repository_with_runtime_config_and_dirty_paths_and_progress_and_commit_callback(
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
                                let epochs = plan_epochs
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                abort_watch_index_plan_if_dispatch_epoch_stale(
                                    &epochs,
                                    &plan_repository_id,
                                    dispatch_epoch,
                                )
                            },
                            move |_plan| {
                                let epochs = commit_epochs
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                abort_watch_index_plan_if_dispatch_epoch_stale(
                                    &epochs,
                                    &commit_repository_id,
                                    dispatch_epoch,
                                )
                            },
                            move |progress| {
                                report_watch_event(
                                    &progress_reporter,
                                    WatchEvent::IndexProgress {
                                        repository_id: progress_repository_id.clone(),
                                        refresh_class: class.as_str(),
                                        progress,
                                    },
                                );
                            },
                        )
                    }
                    WatchRefreshClass::SemanticFollowup => {
                        let plan_reporter = reporter.clone();
                        let progress_reporter = reporter.clone();
                        let plan_repository_id = repository.repository_id.clone();
                        let progress_repository_id = repository.repository_id.clone();
                        let plan_epochs = Arc::clone(&worker_epochs);
                        let index_mode = if recent_paths.is_empty() {
                            IndexMode::Full
                        } else {
                            IndexMode::ChangedOnly
                        };
                        let commit_repository_id = repository.repository_id.clone();
                        let commit_epochs = Arc::clone(&worker_epochs);
                        index_repository_with_runtime_config_and_dirty_paths_and_progress_and_commit_callback(
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
                                let epochs = plan_epochs
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                abort_watch_index_plan_if_dispatch_epoch_stale(
                                    &epochs,
                                    &plan_repository_id,
                                    dispatch_epoch,
                                )
                            },
                            move |_plan| {
                                let epochs = commit_epochs
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                abort_watch_index_plan_if_dispatch_epoch_stale(
                                    &epochs,
                                    &commit_repository_id,
                                    dispatch_epoch,
                                )
                            },
                            move |progress| {
                                report_watch_event(
                                    &progress_reporter,
                                    WatchEvent::IndexProgress {
                                        repository_id: progress_repository_id.clone(),
                                        refresh_class: class.as_str(),
                                        progress,
                                    },
                                );
                            },
                        )
                    }
                });
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
                    Err(err) => Some(format!(
                        "refresh_class={} error={}",
                        class.as_str(),
                        err.message
                    )),
                };
                let status = if result.is_ok() {
                    RuntimeTaskStatus::Succeeded
                } else {
                    RuntimeTaskStatus::Failed
                };
                let should_invalidate_worker_cache = {
                    let epochs = invalidation_epochs
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    should_invalidate_watch_worker_cache(
                        &epochs,
                        &repository.repository_id,
                        dispatch_epoch,
                    )
                };
                if should_invalidate_worker_cache
                    && let Some(callback) = repository_cache_invalidation_callback.as_ref()
                {
                    let dirty_paths = match &result {
                        Ok(summary) => {
                            let mut paths = summary.changed_paths.clone();
                            paths.extend(summary.deleted_paths.iter().cloned());
                            // Known set (may be empty noop) — do not whole-wipe handles.
                            Some(paths)
                        }
                        // Unknown dirty set on failure → whole-repo handle invalidation.
                        Err(_) => None,
                    };
                    callback(
                        &repository.repository_id,
                        dirty_paths.as_deref(),
                    );
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

fn run_caught_watch_index_work<F>(
    work: F,
) -> Result<crate::indexer::IndexSummary, WatchRefreshFailure>
where
    F: FnOnce() -> FriggResult<crate::indexer::IndexSummary>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(result) => result.map_err(WatchRefreshFailure::from_error),
        Err(_) => Err(WatchRefreshFailure {
            message: WATCH_INDEX_WORKER_PANIC_MESSAGE.to_owned(),
            kind: WatchRefreshFailureKind::Retryable,
        }),
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

#[cfg(test)]
fn active_refresh_task_running_for_repository(
    registry: &RuntimeTaskRegistry,
    class: WatchRefreshClass,
    repository_id: &str,
    stable_repository_id: &str,
) -> bool {
    let repository_ids = watch_runtime_task_repository_aliases(repository_id, stable_repository_id);
    let repository_ids = repository_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    !registry
        .active_tasks_for_kinds_and_any_repository(
            conflicting_task_kinds_for_class(class),
            &repository_ids,
        )
        .is_empty()
}

fn watch_runtime_task_repository_aliases(
    repository_id: &str,
    stable_repository_id: &str,
) -> Vec<String> {
    if repository_id == stable_repository_id {
        vec![repository_id.to_owned()]
    } else {
        vec![repository_id.to_owned(), stable_repository_id.to_owned()]
    }
}

fn watch_task_phase_for_class(class: WatchRefreshClass) -> &'static str {
    match class {
        WatchRefreshClass::ManifestFast => "watch_manifest_fast",
        WatchRefreshClass::SemanticFollowup => "watch_semantic_followup",
    }
}

#[allow(clippy::too_many_arguments)]
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

    // Collect dirty relative paths per repo, then one invalidation callback per repo.
    let mut dirty_paths_by_repo: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
        let relative_path =
            repository_relative_watch_path(&repository, &path).unwrap_or_else(|| path.clone());
        let relative_display = relative_path.display().to_string().replace('\\', "/");
        dirty_paths_by_repo
            .entry(repository.repository_id.clone())
            .or_default()
            .push(relative_display.clone());
        info!(
            repository_id = %repository.repository_id,
            path = %path.display(),
            "built-in watch mode accepted path change"
        );
        report_watch_event(
            reporter,
            WatchEvent::PathAccepted {
                repository_id: repository.repository_id,
                path: relative_path,
            },
        );
    }
    if let Some(callback) = repository_cache_invalidation_callback {
        for (repository_id, dirty_paths) in dirty_paths_by_repo {
            callback(&repository_id, Some(&dirty_paths));
        }
    }
}

#[allow(clippy::too_many_arguments)]
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
            // Unknown path set after notify drop → whole-repo handle invalidation.
            callback(&repository.repository_id, None);
        }
        info!(
            repository_id = %repository.repository_id,
            root = %repository.root.display(),
            "built-in watch mode conservatively invalidated repository after dropped notify event"
        );
    }
}

pub(crate) fn reconcile_validated_manifest_cache_after_manifest_fast_success(
    scheduler: &WatchSchedulerState,
    repository_id: &str,
    root: &std::path::Path,
    validated_manifest_candidate_cache: &Arc<RwLock<ValidatedManifestCandidateCache>>,
) {
    let mut cache = validated_manifest_candidate_cache
        .write()
        .expect("validated manifest candidate cache poisoned");
    if scheduler.repository_pending(repository_id, WatchRefreshClass::ManifestFast) {
        cache.mark_dirty_root(root);
    } else {
        cache.invalidate_root(root);
    }
}

fn invalidate_caches_for_stale_completion(
    repository_id: &str,
    result: &Result<crate::indexer::IndexSummary, WatchRefreshFailure>,
    repository_cache_invalidation_callback: Option<&RepositoryCacheInvalidationCallback>,
) -> bool {
    if let Some(callback) = repository_cache_invalidation_callback {
        let dirty_paths = match result {
            Ok(summary) => {
                let mut paths = summary.changed_paths.clone();
                paths.extend(summary.deleted_paths.iter().cloned());
                Some(paths)
            }
            Err(_) => None,
        };
        callback(repository_id, dirty_paths.as_deref());
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
    result: Result<crate::indexer::IndexSummary, WatchRefreshFailure>,
    validated_manifest_candidate_cache: &Arc<RwLock<ValidatedManifestCandidateCache>>,
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
        let dirty_paths = match &result {
            Ok(summary) => {
                let mut paths = summary.changed_paths.clone();
                paths.extend(summary.deleted_paths.iter().cloned());
                Some(paths)
            }
            Err(_) => None,
        };
        callback(&repository.repository_id, dirty_paths.as_deref());
    }

    match result {
        Ok(summary) => {
            scheduler.mark_succeeded(repository_id, class, now);
            if class == WatchRefreshClass::ManifestFast {
                reconcile_validated_manifest_cache_after_manifest_fast_success(
                    scheduler,
                    repository_id,
                    &repository.root,
                    validated_manifest_candidate_cache,
                );
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
        Err(failure) => {
            if failure.blocks_retry() {
                scheduler.mark_blocked(repository_id, class);
                if reporter.is_none() {
                    warn!(
                        repository_id = %repository.repository_id,
                        root = %repository.root.display(),
                        refresh_class = %class.as_str(),
                        error = %failure.message,
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
                        error: failure.message,
                    },
                );
            } else {
                let retry_delay = scheduler
                    .mark_failed(repository_id, class, now, retry)
                    .unwrap_or(retry);
                if reporter.is_none() {
                    warn!(
                        repository_id = %repository.repository_id,
                        root = %repository.root.display(),
                        refresh_class = %class.as_str(),
                        retry_ms = retry_delay.as_millis(),
                        error = %failure.message,
                        "built-in watch mode refresh failed; retry scheduled"
                    );
                }
                report_watch_event(
                    reporter,
                    WatchEvent::RefreshFailed {
                        repository_id: repository.repository_id,
                        root: repository.root,
                        refresh_class: class.as_str(),
                        retry_ms: retry_delay.as_millis(),
                        error: failure.message,
                    },
                );
            }
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
    reporter: &Option<WatchEventReporter>,
) {
    let startup_status = match startup_refresh_status(
        repository,
        semantic_runtime,
        semantic_credentials,
    ) {
        Ok(status) => status,
        Err(error) => {
            if startup_refresh_error_blocks_retry(&error) {
                warn!(
                    repository_id = %repository.repository_id,
                    root = %repository.root.display(),
                    error = %error,
                    "built-in watch mode failed startup freshness validation; skipping doomed refresh"
                );
                return;
            }
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

pub(super) fn queue_semantic_followup_if_needed(
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
            if startup_refresh_error_blocks_retry(&error) {
                warn!(
                    repository_id = %repository.repository_id,
                    root = %repository.root.display(),
                    error = %error,
                    "built-in watch mode failed semantic follow-up validation; skipping doomed refresh"
                );
                return;
            }
            warn!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                error = %error,
                "built-in watch mode failed to evaluate semantic follow-up freshness; queueing semantic follow-up conservatively"
            );
            scheduler.enqueue_semantic_followup(&repository.repository_id, now);
            info!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                "built-in watch mode queued semantic follow-up after manifest refresh freshness probe error"
            );
            report_watch_event(
                reporter,
                WatchEvent::RefreshQueued {
                    repository_id: repository.repository_id.clone(),
                    root: repository.root.clone(),
                    refresh_class: WatchRefreshClass::SemanticFollowup.as_str(),
                    reason: "semantic_followup_freshness_error".to_owned(),
                    snapshot_id: None,
                    debounce_ms: None,
                },
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
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

    fn temp_supervisor_db_path(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "frigg-watch-supervisor-{test_name}-{}-{unique}.sqlite3",
            std::process::id()
        ))
    }

    fn seed_incompatible_schema_version(db_path: &std::path::Path) {
        let conn =
            rusqlite::Connection::open(db_path).expect("test incompatible sqlite db should open");
        conn.execute_batch(
            r#"
            CREATE TABLE schema_version (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              version INTEGER NOT NULL,
              updated_at TEXT NOT NULL
            );
            INSERT INTO schema_version (id, version, updated_at)
            VALUES (1, 10, CURRENT_TIMESTAMP);
            "#,
        )
        .expect("test incompatible schema version should seed");
    }

    #[test]
    fn stale_successful_completion_invalidates_repository_caches() {
        let invalidated: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = Arc::clone(&invalidated);
        let callback: RepositoryCacheInvalidationCallback = Arc::new(move |repository_id: &str, _dirty_paths: Option<&[String]>| {
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
        let callback: RepositoryCacheInvalidationCallback = Arc::new(move |repository_id: &str, _dirty_paths: Option<&[String]>| {
            recorded
                .lock()
                .expect("invalidation record poisoned")
                .push(repository_id.to_owned());
        });

        let invoked = invalidate_caches_for_stale_completion(
            "repo-001",
            &Err(WatchRefreshFailure {
                message: "boom".to_owned(),
                kind: WatchRefreshFailureKind::Retryable,
            }),
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

        assert_eq!(
            scheduler.mark_failed(&ready.repository_id, ready.class, now, retry),
            Some(retry)
        );
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
    fn startup_refresh_validation_failure_does_not_queue_doomed_manifest_fast() {
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
            !scheduler.repository_pending("repo-001", WatchRefreshClass::ManifestFast),
            "a startup validation failure must not queue a doomed manifest-fast refresh"
        );
    }

    #[test]
    fn incompatible_storage_schema_errors_do_not_retry_forever() {
        let failure = WatchRefreshFailure::from_error(FriggError::StorageSchemaIncompatible {
            found_version: 10,
            expected_version: 11,
            db_path: "/tmp/frigg.db".into(),
        });

        assert_eq!(
            failure.kind,
            WatchRefreshFailureKind::StorageSchemaIncompatible
        );
        assert!(failure.blocks_retry());
        assert!(failure.message.contains("storage schema is incompatible"));

        let transient =
            WatchRefreshFailure::from_error(FriggError::Internal("transient read failed".into()));
        assert_eq!(transient.kind, WatchRefreshFailureKind::Retryable);
        assert!(!transient.blocks_retry());
    }

    #[test]
    fn incompatible_storage_schema_completion_blocks_watch_refresh() {
        use super::super::repository::watched_repository_for_root;

        let db_path = temp_supervisor_db_path("incompatible-completion");
        seed_incompatible_schema_version(&db_path);
        let storage_error = crate::storage::Storage::new(&db_path)
            .require_current_schema()
            .expect_err("incompatible schema should reject current-schema requirement");
        let failure = WatchRefreshFailure::from_error(storage_error);

        let root = std::env::temp_dir().join("frigg-watch-supervisor-incompatible-root");
        let repository =
            watched_repository_for_root("repo-001".to_owned(), root.clone(), db_path.clone())
                .expect("watched repository should build for test root");
        let repositories = Arc::new(RwLock::new(BTreeMap::from([(
            repository.repository_id.clone(),
            repository,
        )])));
        let mut scheduler = WatchSchedulerState::new(0);
        scheduler.add_repository("repo-001");
        let now = Instant::now();
        let retry = Duration::from_millis(250);
        scheduler.enqueue_initial_sync("repo-001", WatchRefreshClass::ManifestFast, now);
        scheduler.mark_started("repo-001", WatchRefreshClass::ManifestFast);

        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded_events = Arc::clone(&events);
        let reporter: WatchEventReporter = Arc::new(move |event| {
            recorded_events
                .lock()
                .expect("watch event log poisoned")
                .push(event);
        });
        let validated_manifest_candidate_cache =
            Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
        handle_index_completed(
            &repositories,
            &mut scheduler,
            "repo-001",
            WatchRefreshClass::ManifestFast,
            Err(failure),
            &validated_manifest_candidate_cache,
            None,
            now,
            retry,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
            &Some(reporter),
        );

        assert!(!scheduler.repository_pending("repo-001", WatchRefreshClass::ManifestFast));
        assert_eq!(scheduler.next_ready_refresh(now + retry), None);
        let events = events.lock().expect("watch event log poisoned");
        assert!(
            events.iter().any(|event| matches!(
                event,
                WatchEvent::RefreshBlocked {
                    repository_id,
                    refresh_class,
                    reason,
                    error,
                    ..
                } if repository_id == "repo-001"
                    && *refresh_class == WatchRefreshClass::ManifestFast.as_str()
                    && *reason == "storage_schema_incompatible"
                    && error.contains("storage schema is incompatible")
            )),
            "typed storage schema errors should emit RefreshBlocked"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, WatchEvent::RefreshFailed { .. })),
            "typed storage schema errors must not schedule RefreshFailed retry events"
        );

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    #[allow(clippy::panic)]
    fn watch_panic_worker_maps_to_retryable_refresh_failure() {
        let result = run_caught_watch_index_work(|| {
            panic!("simulate watch index worker panic");
        });

        let failure = result.expect_err("panicked watch worker should become a refresh failure");
        assert_eq!(failure.message, WATCH_INDEX_WORKER_PANIC_MESSAGE);
        assert_eq!(failure.kind, WatchRefreshFailureKind::Retryable);
        assert!(!failure.blocks_retry());
    }

    #[test]
    fn watch_panic_orphan_scheduler_clears_latch_via_synthetic_failure() {
        use super::super::repository::watched_repository_for_root;

        let root = std::env::temp_dir().join("frigg-watch-supervisor-panic-orphan-root");
        let db_path = temp_supervisor_db_path("panic-orphan-completion");
        let repository =
            watched_repository_for_root("repo-001".to_owned(), root.clone(), db_path.clone())
                .expect("watched repository should build for test root");
        let repositories = Arc::new(RwLock::new(BTreeMap::from([(
            repository.repository_id.clone(),
            repository,
        )])));
        let mut scheduler = WatchSchedulerState::new(0);
        scheduler.add_repository("repo-001");
        let now = Instant::now();
        let retry = Duration::from_millis(250);
        scheduler.enqueue_initial_sync("repo-001", WatchRefreshClass::ManifestFast, now);
        scheduler.mark_started("repo-001", WatchRefreshClass::ManifestFast);

        assert!(
            scheduler.next_ready_refresh(now).is_none(),
            "latched scheduler should not dispatch while active_class is set"
        );

        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded_events = Arc::clone(&events);
        let reporter: WatchEventReporter = Arc::new(move |event| {
            recorded_events
                .lock()
                .expect("watch event log poisoned")
                .push(event);
        });
        let validated_manifest_candidate_cache =
            Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
        handle_index_completed(
            &repositories,
            &mut scheduler,
            "repo-001",
            WatchRefreshClass::ManifestFast,
            Err(WatchRefreshFailure {
                message: WATCH_INDEX_WORKER_PANIC_MESSAGE.to_owned(),
                kind: WatchRefreshFailureKind::Retryable,
            }),
            &validated_manifest_candidate_cache,
            None,
            now,
            retry,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
            &Some(reporter),
        );

        assert!(
            scheduler.repository_pending("repo-001", WatchRefreshClass::ManifestFast),
            "panic completion should schedule a retry instead of leaving the latch stuck"
        );
        assert_eq!(
            scheduler.next_ready_refresh(now + retry),
            Some(ScheduledRefresh {
                root_idx: 0,
                repository_id: "repo-001".to_owned(),
                class: WatchRefreshClass::ManifestFast,
            })
        );
        let events = events.lock().expect("watch event log poisoned");
        assert!(
            events.iter().any(|event| matches!(
                event,
                WatchEvent::RefreshFailed {
                    repository_id,
                    refresh_class,
                    error,
                    ..
                } if repository_id == "repo-001"
                    && *refresh_class == WatchRefreshClass::ManifestFast.as_str()
                    && error == WATCH_INDEX_WORKER_PANIC_MESSAGE
            )),
            "synthetic panic completion should emit RefreshFailed"
        );

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn stale_dispatch_epoch_blocks_worker_side_effects_before_completion() {
        use std::fs;

        use crate::indexer::index_repository_with_runtime_config_and_dirty_paths_and_progress_callback;
        use crate::settings::SemanticRuntimeCredentials;
        use crate::storage::Storage;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let workspace_root = std::env::temp_dir().join(format!(
            "frigg-watch-stale-epoch-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(workspace_root.join("src")).expect("workspace src should be creatable");
        fs::write(
            workspace_root.join("src/lib.rs"),
            "pub fn stale_epoch_guard() {}\n",
        )
        .expect("workspace source should be writable");

        let db_path = temp_supervisor_db_path("stale-dispatch-side-effects");
        index_repository_with_runtime_config_and_dirty_paths_and_progress_callback(
            "repo-001",
            &workspace_root,
            &db_path,
            IndexMode::ChangedOnly,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
            &[],
            |_| Ok(()),
            |_| {},
        )
        .expect("baseline index should succeed");

        let baseline_snapshot = Storage::new(&db_path)
            .load_latest_manifest_for_repository("repo-001")
            .expect("baseline manifest should load")
            .expect("baseline snapshot should exist")
            .snapshot_id;

        fs::write(
            workspace_root.join("src/lib.rs"),
            "pub fn stale_epoch_guard() { 1 }\n",
        )
        .expect("workspace source should be writable for refresh attempt");

        let epochs = Arc::new(Mutex::new(RepositoryEpochs::default()));
        epochs
            .lock()
            .expect("repository epochs poisoned")
            .ensure("repo-001");
        let dispatch_epoch = epochs
            .lock()
            .expect("repository epochs poisoned")
            .current("repo-001");
        epochs
            .lock()
            .expect("repository epochs poisoned")
            .bump("repo-001");

        let worker_epochs = Arc::clone(&epochs);
        let invalidated: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&invalidated);
        let callback: RepositoryCacheInvalidationCallback = Arc::new(move |repository_id: &str, _dirty_paths: Option<&[String]>| {
            recorded
                .lock()
                .expect("invalidation record poisoned")
                .push(repository_id.to_owned());
        });

        let dirty_path = workspace_root.join("src/lib.rs");
        let result = index_repository_with_runtime_config_and_dirty_paths_and_progress_callback(
            "repo-001",
            &workspace_root,
            &db_path,
            IndexMode::ChangedOnly,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
            &[dirty_path],
            move |_plan| {
                let epochs = worker_epochs
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                abort_watch_index_plan_if_dispatch_epoch_stale(&epochs, "repo-001", dispatch_epoch)
            },
            |_| {},
        );

        assert!(
            result.is_err(),
            "stale dispatch epoch must abort index before storage commit"
        );
        assert!(
            result
                .expect_err("stale dispatch epoch should return an error")
                .to_string()
                .contains(WATCH_STALE_DISPATCH_EPOCH_MESSAGE),
            "stale dispatch epoch should surface the watch abort message"
        );

        let after_snapshot = Storage::new(&db_path)
            .load_latest_manifest_for_repository("repo-001")
            .expect("manifest should still load after aborted refresh")
            .expect("baseline snapshot should remain")
            .snapshot_id;
        assert_eq!(
            after_snapshot, baseline_snapshot,
            "stale dispatch epoch must not commit sqlite"
        );

        let epochs_guard = epochs.lock().expect("repository epochs poisoned");
        if should_invalidate_watch_worker_cache(&epochs_guard, "repo-001", dispatch_epoch) {
            callback("repo-001", None);
        }
        assert!(
            invalidated
                .lock()
                .expect("invalidation record poisoned")
                .is_empty(),
            "stale dispatch epoch must skip worker-side cache invalidation"
        );

        let _ = fs::remove_dir_all(&workspace_root);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn stale_dispatch_epoch_blocks_worker_side_effects_after_plan_before_commit() {
        use std::fs;

        use crate::indexer::{
            index_repository_with_runtime_config_and_dirty_paths_and_progress_and_commit_callback,
            index_repository_with_runtime_config_and_dirty_paths_and_progress_callback,
        };
        use crate::settings::SemanticRuntimeCredentials;
        use crate::storage::Storage;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let workspace_root = std::env::temp_dir().join(format!(
            "frigg-watch-stale-commit-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(workspace_root.join("src")).expect("workspace src should be creatable");
        fs::write(
            workspace_root.join("src/lib.rs"),
            "pub fn stale_commit_guard() {}\n",
        )
        .expect("workspace source should be writable");

        let db_path = temp_supervisor_db_path("stale-dispatch-commit-side-effects");
        index_repository_with_runtime_config_and_dirty_paths_and_progress_callback(
            "repo-001",
            &workspace_root,
            &db_path,
            IndexMode::ChangedOnly,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
            &[],
            |_| Ok(()),
            |_| {},
        )
        .expect("baseline index should succeed");

        let baseline_snapshot = Storage::new(&db_path)
            .load_latest_manifest_for_repository("repo-001")
            .expect("baseline manifest should load")
            .expect("baseline snapshot should exist")
            .snapshot_id;

        fs::write(
            workspace_root.join("src/lib.rs"),
            "pub fn stale_commit_guard() { 1 }\n",
        )
        .expect("workspace source should be writable for refresh attempt");

        let epochs = Arc::new(Mutex::new(RepositoryEpochs::default()));
        epochs
            .lock()
            .expect("repository epochs poisoned")
            .ensure("repo-001");
        let dispatch_epoch = epochs
            .lock()
            .expect("repository epochs poisoned")
            .current("repo-001");

        let commit_epochs = Arc::clone(&epochs);
        let dirty_path = workspace_root.join("src/lib.rs");
        let result =
            index_repository_with_runtime_config_and_dirty_paths_and_progress_and_commit_callback(
                "repo-001",
                &workspace_root,
                &db_path,
                IndexMode::ChangedOnly,
                &SemanticRuntimeConfig::default(),
                &SemanticRuntimeCredentials::default(),
                &[dirty_path],
                |_plan| Ok(()),
                move |_plan| {
                    let mut epochs = commit_epochs
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    epochs.bump("repo-001");
                    abort_watch_index_plan_if_dispatch_epoch_stale(
                        &epochs,
                        "repo-001",
                        dispatch_epoch,
                    )
                },
                |_| {},
            );

        assert!(
            result.is_err(),
            "stale dispatch epoch must abort immediately before storage commit"
        );
        assert!(
            result
                .expect_err("stale dispatch epoch should return an error")
                .to_string()
                .contains(WATCH_STALE_DISPATCH_EPOCH_MESSAGE),
            "stale dispatch epoch should surface the watch abort message"
        );

        let after_snapshot = Storage::new(&db_path)
            .load_latest_manifest_for_repository("repo-001")
            .expect("manifest should still load after aborted refresh")
            .expect("baseline snapshot should remain")
            .snapshot_id;
        assert_eq!(
            after_snapshot, baseline_snapshot,
            "stale dispatch epoch must not commit sqlite after a successful plan callback"
        );

        let _ = fs::remove_dir_all(&workspace_root);
        let _ = std::fs::remove_file(&db_path);
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
